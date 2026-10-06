//! Access management: who may use YASA. Invite-only; admins invite, promote and disable.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AdminAccount, ApiError, Method, db_error, login_key},
};

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Member,
    Admin,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Member => "member",
            Role::Admin => "admin",
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Disabled,
}

/// One row of the Access page. Never includes the NIF/NIN keys, only which kinds exist.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    id: Uuid,
    label: String,
    role: String,
    status: String,
    has_nif: bool,
    has_nin: bool,
    /// Unix seconds; the frontend formats dates.
    created_at: i64,
    last_login_at: Option<i64>,
    access_level: String,
    /// How many agrupamentos, núcleos and regiões SIIE showed them at their last login
    /// (the lists themselves are in /me, for the account's own session).
    agrupamentos: i64,
    nucleos: i64,
    regioes: i64,
}

/// GET /api/admin/accounts
pub async fn list(
    State(s): State<AppState>,
    _: AdminAccount,
) -> Result<Json<Vec<AccountRow>>, ApiError> {
    let rows = sqlx::query_as(
        "select a.id, a.label, a.role, a.status,
                a.nif_key is not null as has_nif, a.nin_key is not null as has_nin,
                extract(epoch from a.created_at)::bigint as created_at,
                extract(epoch from a.last_login_at)::bigint as last_login_at,
                a.access_level,
                cardinality(a.agrupamentos)::bigint as agrupamentos,
                cardinality(a.nucleos)::bigint as nucleos,
                cardinality(a.regioes)::bigint as regioes
         from accounts a
         order by lower(a.label)",
    )
    .fetch_all(&s.db)
    .await
    .map_err(db_error)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct InviteBody {
    method: Method,
    identifier: String,
    label: String,
    role: Role,
}

/// POST /api/admin/accounts: the identifier is hashed here and never stored as typed.
pub async fn invite(
    State(s): State<AppState>,
    AdminAccount(admin): AdminAccount,
    Json(b): Json<InviteBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = create_account(
        &s.db,
        s.login_key_secret.expose_secret(),
        b.method,
        &b.identifier,
        &b.label,
        b.role,
        Some(admin.id),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

/// Shared by the API and `cargo run --bin invite` (which bootstraps the first admin).
pub async fn create_account(
    db: &PgPool,
    secret: &[u8],
    method: Method,
    identifier: &str,
    label: &str,
    role: Role,
    invited_by: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    let (identifier, label) = (identifier.trim(), label.trim());
    if identifier.is_empty() || label.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Indique o NIF/NIN e o nome.",
        ));
    }
    let col = method.key_column();
    let row: Result<(Uuid,), _> = sqlx::query_as(&format!(
        "insert into accounts ({col}, label, role, invited_by) values ($1, $2, $3, $4) returning id"
    ))
    .bind(login_key(secret, identifier))
    .bind(label)
    .bind(role.as_str())
    .bind(invited_by)
    .fetch_one(db)
    .await;
    match row {
        Ok((id,)) => Ok(id),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(ApiError::new(
            StatusCode::CONFLICT,
            "Esse NIF/NIN já tem conta.",
        )),
        Err(e) => Err(db_error(e)),
    }
}

#[derive(Deserialize)]
pub struct UpdateBody {
    status: Option<Status>,
    role: Option<Role>,
}

/// PATCH /api/admin/accounts/{id}: enable/disable and promote/demote.
pub async fn update(
    State(s): State<AppState>,
    _: AdminAccount,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateBody>,
) -> Result<StatusCode, ApiError> {
    let mut tx = s.db.begin().await.map_err(db_error)?;
    // Lock the current admins first, so two admins demoting each other at the same time can't
    // both pass the "at least one admin" check below.
    sqlx::query("select id from accounts where role = 'admin' and status = 'active' for update")
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

    let status = b.status.map(|st| match st {
        Status::Active => "active",
        Status::Disabled => "disabled",
    });
    let updated = sqlx::query(
        "update accounts set status = coalesce($2, status), role = coalesce($3, role) where id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(b.role.map(Role::as_str))
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Conta não encontrada.",
        ));
    }

    let (admins,): (i64,) =
        sqlx::query_as("select count(*) from accounts where role = 'admin' and status = 'active'")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
    if admins == 0 {
        // Dropping `tx` without commit rolls the change back.
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "O YASA precisa de pelo menos um administrador ativo.",
        ));
    }

    // Disabled = locked out now, not at the end of their session.
    if matches!(b.status, Some(Status::Disabled)) {
        sqlx::query("delete from sessions where account_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
    }
    tx.commit().await.map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}
