//! Login and sessions. SIIE is the identity provider: YASA has no passwords of its own.
//!
//! Login order (see README "Access model"):
//! 1. SIIE checks the password. Someone removed from SIIE fails here.
//! 2. HMAC(identifier) must match an active, invited account. SIIE goes first so this endpoint
//!    can't be used to probe which NIFs are invited. Nothing is stored about uninvited people.
//! 3. The members grid is read with the person's SIIE session: the agrupamentos in it become
//!    their access, and how widely those spread gives their access level.
//! 4. A session starts. The cookie ends with the browser; the DB caps it at SESSION_HOURS.

use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    AppState,
    siie::{self, SiieError},
};

const COOKIE_NAME: &str = "yasa_session";
// Server-side cap. The cookie has no Max-Age, so it normally dies with the browser first.
const SESSION_HOURS: i32 = 12;
const TOKEN_BYTES: usize = 32;
const NO_ACCESS: &str = "O início de sessão no SIIE funcionou, mas ainda não lhe foi dado acesso ao YASA. Peça acesso a um administrador.";

/// JSON error body: `{"error": "..."}`. Messages never contain credentials.
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn new(status: StatusCode, msg: &str) -> Self {
        Self(status, msg.to_owned())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

pub fn db_error(_: sqlx::Error) -> ApiError {
    // Details stay server-side: they can include SQL and bound values.
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Erro na base de dados.")
}

fn siie_error(e: SiieError) -> ApiError {
    match e {
        SiieError::LoginRejected(_) => ApiError(StatusCode::UNAUTHORIZED, e.to_string()),
        // Can't verify access = no access. SiieError messages never contain credentials.
        _ => ApiError(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

/// Same choice as the radio buttons on the SIIE login page.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Loja Escutista: NIF + password.
    Loja,
    /// SIIE: NIN + password.
    Siie,
}

impl Method {
    /// The `accounts` column holding this identifier's HMAC. Static strings only, so it is safe
    /// to put in SQL.
    pub fn key_column(self) -> &'static str {
        match self {
            Method::Loja => "nif_key",
            Method::Siie => "nin_key",
        }
    }
}

/// Login body. No `Debug` on purpose: it holds the password.
#[derive(Deserialize)]
pub struct LoginBody {
    identifier: String,
    password: SecretString,
    method: Method,
}

/// POST /api/auth/login
pub async fn login(
    State(s): State<AppState>,
    Json(b): Json<LoginBody>,
) -> Result<Response, ApiError> {
    if b.identifier.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Indique o NIF/NIN."));
    }
    // Computed before `identifier` moves into the credentials (dropped once SIIE answers).
    let key = login_key(s.login_key_secret.expose_secret(), &b.identifier);

    // 1. SIIE decides whether the password is right.
    let client = siie::new_client().map_err(siie_error)?;
    let creds = siie::Credentials {
        nif: b.identifier.trim().to_owned(),
        password: b.password,
        loja_escutista: matches!(b.method, Method::Loja),
    };
    let siie_login = siie::login(&client, creds).await.map_err(siie_error)?;

    // 2. Invite list. Not invited and disabled get the same answer.
    let col = b.method.key_column();
    let account: Option<(Uuid,)> = sqlx::query_as(&format!(
        "select id from accounts where {col} = $1 and status = 'active'"
    ))
    .bind(&key)
    .fetch_optional(&s.db)
    .await
    .map_err(db_error)?;
    let Some((account_id,)) = account else {
        return Err(ApiError::new(StatusCode::FORBIDDEN, NO_ACCESS));
    };

    // 3. Access = what SIIE shows this person today. The SIIE session ends right after.
    let vis = siie::visibility(&client, &siie_login.token)
        .await
        .map_err(siie_error)?;
    drop(siie_login);
    drop(client);
    let level = vis.level();
    // Sets -> Vecs for binding as text[] (already sorted, since they are BTreeSets).
    let agrupamentos: Vec<String> = vis.agrupamentos.into_iter().collect();
    let nucleos: Vec<String> = vis.nucleos.into_iter().collect();
    let regioes: Vec<String> = vis.regioes.into_iter().collect();

    // 4. Access refresh + session in one transaction, so a half-applied login can't exist.
    let token = new_token();
    let mut tx = s.db.begin().await.map_err(db_error)?;
    sqlx::query(
        "update accounts set last_login_at = now(), access_refreshed_at = now(),
                access_level = $2, agrupamentos = $3, nucleos = $4, regioes = $5
         where id = $1",
    )
    .bind(account_id)
    .bind(level.as_str())
    .bind(&agrupamentos)
    .bind(&nucleos)
    .bind(&regioes)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    sqlx::query(
        "insert into sessions (token_hash, account_id, expires_at)
         values ($1, $2, now() + make_interval(hours => $3))",
    )
    .bind(token_hash(&token))
    .bind(account_id)
    .bind(SESSION_HOURS)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    // Housekeeping: expired sessions are useless, clear them while we're here.
    sqlx::query("delete from sessions where expires_at < now()")
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;

    let cookie = session_cookie(&token, None, s.secure_cookies);
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

/// GET /api/auth/me: who is logged in, or 401 (the frontend then shows only the login page).
pub async fn me(State(s): State<AppState>, acc: CurrentAccount) -> Result<Response, ApiError> {
    let (access_level, agrupamentos, nucleos, regioes): (
        String,
        Vec<String>,
        Vec<String>,
        Vec<String>,
    ) = sqlx::query_as(
        "select access_level, agrupamentos, nucleos, regioes from accounts where id = $1",
    )
    .bind(acc.id)
    .fetch_one(&s.db)
    .await
    .map_err(db_error)?;
    // Full lists: codes of organisational units only, no personal data.
    let body = json!({
        "accountId": acc.id,
        "label": acc.label,
        "role": acc.role,
        "accessLevel": access_level,
        "agrupamentos": agrupamentos,
        "nucleos": nucleos,
        "regioes": regioes,
    });
    Ok(Json(body).into_response())
}

/// POST /api/auth/logout: always clears the cookie, even if the session was already gone.
pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = cookie_token(&headers) {
        sqlx::query("delete from sessions where token_hash = $1")
            .bind(token_hash(token))
            .execute(&s.db)
            .await
            .map_err(db_error)?;
    }
    let cookie = session_cookie("", Some(0), s.secure_cookies);
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

/// Extractor for any route that needs a logged-in user: add `acc: CurrentAccount` to the handler.
/// A disabled account stops working on its next request, even if a session row survived.
pub struct CurrentAccount {
    pub id: Uuid,
    pub label: String,
    pub role: String,
}

impl FromRequestParts<AppState> for CurrentAccount {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, ApiError> {
        let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, "Sessão não iniciada.");
        let token = cookie_token(&parts.headers).ok_or_else(unauthorized)?;
        let row: Option<(Uuid, String, String)> = sqlx::query_as(
            "select a.id, a.label, a.role
             from sessions s join accounts a on a.id = s.account_id
             where s.token_hash = $1 and s.expires_at > now() and a.status = 'active'",
        )
        .bind(token_hash(token))
        .fetch_optional(&s.db)
        .await
        .map_err(db_error)?;
        let (id, label, role) = row.ok_or_else(unauthorized)?;
        Ok(Self { id, label, role })
    }
}

/// Like `CurrentAccount`, but 403 unless the account is an admin.
pub struct AdminAccount(pub CurrentAccount);

impl FromRequestParts<AppState> for AdminAccount {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, ApiError> {
        let acc = CurrentAccount::from_request_parts(parts, s).await?;
        if acc.role != "admin" {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "Apenas para administradores.",
            ));
        }
        Ok(Self(acc))
    }
}

/// HMAC-SHA256(secret, normalized identifier). Same person -> same key on every login, but
/// without the secret (env vars only, not the DB) the key can't be linked back to a NIF/NIN.
/// Normalized = no whitespace, uppercase, so "123 456 789" and "123456789" match.
pub fn login_key(secret: &[u8], identifier: &str) -> Vec<u8> {
    let normalized: String = identifier
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(normalized.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// 32 random bytes as hex (256 bits of entropy), used as the cookie value.
fn new_token() -> String {
    hex::encode(rand::random::<[u8; TOKEN_BYTES]>())
}

/// What the DB stores instead of the cookie value. Plain sha256 is enough: the token is random,
/// so there is nothing to brute-force.
fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// HttpOnly: JS can't read it. SameSite=Lax: not sent on cross-site POSTs (CSRF).
/// `max_age: None` = browser-session cookie (gone when the browser closes).
fn session_cookie(token: &str, max_age: Option<i32>, secure: bool) -> HeaderValue {
    let max_age = max_age
        .map(|s| format!("; Max-Age={s}"))
        .unwrap_or_default();
    let secure = if secure { "; Secure" } else { "" };
    let value = format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax{max_age}{secure}");
    HeaderValue::from_str(&value).expect("cookie is plain ASCII")
}

/// Finds `yasa_session=...` in the Cookie header(s).
fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(COOKIE_NAME)?.strip_prefix('='))
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_key_is_stable_and_depends_on_secret() {
        let a = login_key(&[1; 32], "123456789");
        assert_eq!(a, login_key(&[1; 32], "123456789"));
        assert_ne!(a, login_key(&[2; 32], "123456789"));
        assert_ne!(a, login_key(&[1; 32], "987654321"));
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn login_key_ignores_spacing_and_case() {
        let a = login_key(&[1; 32], "123456789");
        assert_eq!(a, login_key(&[1; 32], " 123 456 789 "));
        assert_eq!(login_key(&[1; 32], "ab12"), login_key(&[1; 32], "AB12"));
    }

    #[test]
    fn token_hash_differs_from_token() {
        let t = new_token();
        assert_eq!(t.len(), TOKEN_BYTES * 2);
        assert_eq!(token_hash(&t), token_hash(&t));
        assert_ne!(token_hash(&t), t.as_bytes());
        assert_ne!(new_token(), t);
    }

    #[test]
    fn session_cookie_ends_with_browser() {
        let c = session_cookie("abc", None, true);
        let c = c.to_str().unwrap();
        assert!(!c.contains("Max-Age"));
        assert!(c.contains("HttpOnly") && c.contains("Secure"));
        let cleared = session_cookie("", Some(0), false);
        assert!(cleared.to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn finds_session_cookie_among_others() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; yasa_session=abc; b=2"),
        );
        assert_eq!(cookie_token(&h), Some("abc"));

        h.insert(
            header::COOKIE,
            HeaderValue::from_static("yasa_sessionx=abc; yasa_session="),
        );
        assert_eq!(cookie_token(&h), None);
    }
}
