//! Access management: who may use YASA. Invite-only; admins invite, promote and disable.
//! Every change is recorded in `account_events`, with who did it.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgExecutor, PgPool, types::Json as DbJson};
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
    /// "never" | "running" | "failed" | "expired" | "ready", as in /api/auth/me.
    access_status: String,
    /// Unix seconds; null until the first successful read.
    access_valid_until: Option<i64>,
    /// What their grants cover (the lists themselves are in /me, for the account's own session).
    agrupamentos: i64,
    nucleos: i64,
    regioes: i64,
}

/// GET /api/admin/accounts
pub async fn list(
    State(s): State<AppState>,
    _: AdminAccount,
) -> Result<Json<Vec<AccountRow>>, ApiError> {
    // Status uses the same rules as access::summary: running/failed win, then never/expired/ready.
    let rows = sqlx::query_as(
        "select a.id, a.label, a.role, a.status,
                a.nif_key is not null as has_nif, a.nin_key is not null as has_nin,
                extract(epoch from a.created_at)::bigint as created_at,
                extract(epoch from a.last_login_at)::bigint as last_login_at,
                coalesce(x.level, 'none') as access_level,
                case
                    when x.refresh_status in ('running', 'failed') then x.refresh_status
                    when x.refreshed_at is null then 'never'
                    when x.refreshed_at + make_interval(days => $1) < now() then 'expired'
                    else 'ready'
                end as access_status,
                extract(epoch from x.refreshed_at + make_interval(days => $1))::bigint
                    as access_valid_until,
                coalesce(c.agrupamentos, 0) as agrupamentos,
                coalesce(c.nucleos, 0) as nucleos,
                coalesce(c.regioes, 0) as regioes
         from accounts a
         left join account_access x on x.account_id = a.id
         left join (
             select g.account_id,
                    count(distinct g.agrupamento) as agrupamentos,
                    count(distinct ag.nucleo) as nucleos,
                    count(distinct ag.regiao) as regioes
             from account_grants g join agrupamentos ag on ag.code = g.agrupamento
             group by g.account_id
         ) c on c.account_id = a.id
         order by lower(a.label)",
    )
    .bind(s.access_validity_days)
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
    // Account + "invited" event in one statement, so neither exists without the other.
    let detail = json!({ "kind": method.kind(), "role": role.as_str() });
    let row: Result<(Uuid,), _> = sqlx::query_as(&format!(
        "with a as (
            insert into accounts ({col}, label, role, invited_by) values ($1, $2, $3, $4)
            returning id
         ), e as (
            insert into account_events (actor_id, account_id, action, detail)
            select $4, id, 'invited', $5 from a
         )
         select id from a"
    ))
    .bind(login_key(secret, identifier))
    .bind(label)
    .bind(role.as_str())
    .bind(invited_by)
    .bind(DbJson(detail))
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
    AdminAccount(admin): AdminAccount,
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

    // The target's current values, to log only what actually changes.
    let before: Option<(String, String)> =
        sqlx::query_as("select status, role from accounts where id = $1 for update")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
    let Some((old_status, old_role)) = before else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Conta não encontrada.",
        ));
    };

    let status = b.status.map(|st| match st {
        Status::Active => "active",
        Status::Disabled => "disabled",
    });
    let role = b.role.map(Role::as_str);
    sqlx::query(
        "update accounts set status = coalesce($2, status), role = coalesce($3, role) where id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(role)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;

    if let Some(new) = status.filter(|new| *new != old_status) {
        let action = if new == "active" {
            "enabled"
        } else {
            "disabled"
        };
        log_event(&mut *tx, Some(admin.id), id, action, json!({}))
            .await
            .map_err(db_error)?;
    }
    if let Some(new) = role.filter(|new| *new != old_role) {
        let detail = json!({ "from": old_role, "to": new });
        log_event(&mut *tx, Some(admin.id), id, "role_changed", detail)
            .await
            .map_err(db_error)?;
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

/// Records one event. `actor` = who did it (None = command line or the system). `detail` holds
/// small facts only (role names, counts), never identifiers or personal data.
pub async fn log_event<'e>(
    db: impl PgExecutor<'e>,
    actor: Option<Uuid>,
    account: Uuid,
    action: &str,
    detail: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into account_events (actor_id, account_id, action, detail) values ($1, $2, $3, $4)",
    )
    .bind(actor)
    .bind(account)
    .bind(action)
    .bind(DbJson(detail))
    .execute(db)
    .await?;
    Ok(())
}

// Enough to see recent activity; older events stay in the table.
const EVENTS_SHOWN: i64 = 100;

/// One line of the Access page history.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    id: i64,
    /// Unix seconds.
    at: i64,
    action: String,
    detail: DbJson<Value>,
    account_id: Uuid,
    account_label: String,
    /// None = command line or the system.
    actor_label: Option<String>,
}

/// GET /api/admin/events: the latest events, newest first.
pub async fn events(
    State(s): State<AppState>,
    _: AdminAccount,
) -> Result<Json<Vec<EventRow>>, ApiError> {
    let rows = sqlx::query_as(
        "select e.id, extract(epoch from e.at)::bigint as at, e.action, e.detail,
                e.account_id, t.label as account_label, actor.label as actor_label
         from account_events e
         join accounts t on t.id = e.account_id
         left join accounts actor on actor.id = e.actor_id
         order by e.at desc, e.id desc
         limit $1",
    )
    .bind(EVENTS_SHOWN)
    .fetch_all(&s.db)
    .await
    .map_err(db_error)?;
    Ok(Json(rows))
}

/// One member import, as the Access page history shows it.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    id: i64,
    agrupamento: i16,
    agrupamento_name: Option<String>,
    /// Whose login queued it. None = that account was deleted.
    account_label: Option<String>,
    status: String,
    /// Unix seconds.
    queued_at: i64,
    finished_at: Option<i64>,
    members_seen: Option<i32>,
    changes: Option<i32>,
    exits: Option<i32>,
    error: Option<String>,
}

/// GET /api/admin/imports: the latest member imports, newest first. Counts only, no members.
pub async fn imports(
    State(s): State<AppState>,
    _: AdminAccount,
) -> Result<Json<Vec<ImportRow>>, ApiError> {
    let rows = sqlx::query_as(
        "select i.id, i.agrupamento, g.name as agrupamento_name, a.label as account_label,
                i.status, extract(epoch from i.queued_at)::bigint as queued_at,
                extract(epoch from i.finished_at)::bigint as finished_at,
                i.members_seen, i.changes, i.exits, i.error
         from member_imports i
         left join agrupamentos g on g.code = i.agrupamento
         left join accounts a on a.id = i.account_id
         order by i.id desc
         limit $1",
    )
    .bind(EVENTS_SHOWN)
    .fetch_all(&s.db)
    .await
    .map_err(db_error)?;
    Ok(Json(rows))
}
