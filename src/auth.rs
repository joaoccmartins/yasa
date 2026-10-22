//! Login and sessions. SIIE is the identity provider: YASA has no passwords of its own.
//!
//! Login order (see README "Access model"):
//! 0. Too many recent SIIE failures for this identifier or IP: refused before SIIE is contacted,
//!    so YASA can't be used to guess SIIE passwords.
//! 1. SIIE checks the password. Someone removed from SIIE fails here.
//! 2. HMAC(identifier) must match an active, invited account. SIIE goes first so this endpoint
//!    can't be used to probe which NIFs are invited. Nothing is stored about uninvited people.
//! 3. Access: if never read or older than ACCESS_VALIDITY_DAYS, the members grid is read in the
//!    background with this same SIIE session (see access.rs). Then the account's agrupamentos
//!    whose members are older than MEMBERS_SYNC_DAYS are imported, one at a time (members.rs).
//!    The login doesn't wait for either.
//! 4. A session starts. The cookie ends with the browser; the DB caps it at SESSION_HOURS.
//!    The login is recorded in account_events.

use axum::{
    Extension, Json,
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
    access::{self, Claim},
    admin::log_event,
    in_background, members,
    siie::{self, SiieError},
};

const COOKIE_NAME: &str = "yasa_session";
// Server-side cap. The cookie has no Max-Age, so it normally dies with the browser first.
const SESSION_HOURS: i32 = 12;
const TOKEN_BYTES: usize = 32;
// Login limits: failed SIIE logins counted over a sliding window, per identifier and per IP.
// Per IP is looser, since a scout group may share one connection.
const FAILURE_WINDOW_MINUTES: i32 = 15;
const MAX_FAILURES_PER_IDENTIFIER: i64 = 5;
const MAX_FAILURES_PER_IP: i64 = 20;
const TOO_MANY: &str = "Demasiadas tentativas falhadas. Tente novamente dentro de 15 minutos.";
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

    /// The identifier the other login option uses.
    pub fn other(self) -> Self {
        match self {
            Method::Loja => Method::Siie,
            Method::Siie => Method::Loja,
        }
    }

    /// For event details: which kind of identifier, never its value.
    pub fn kind(self) -> &'static str {
        match self {
            Method::Loja => "nif",
            Method::Siie => "nin",
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
    // Present on Vercel (see api/yasa.rs): lets the access read outlive the response.
    vercel: Option<Extension<vercel_runtime::AppState>>,
    headers: HeaderMap,
    Json(b): Json<LoginBody>,
) -> Result<Response, ApiError> {
    let id = siie::normalize_identifier(&b.identifier);
    if id.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Indique o NIF/NIN."));
    }
    let secret = s.login_key_secret.expose_secret();
    let key = login_key(secret, &id);

    // 0. Login limits. Keys are HMACs, so the table holds no identifiers or IPs.
    let id_limit = keyed_hash(secret, &format!("fail-id:{id}"));
    let ip_limit = client_ip(&headers).map(|ip| keyed_hash(secret, &format!("fail-ip:{ip}")));
    check_login_limits(&s, &id_limit, ip_limit.as_deref()).await?;

    // 1. SIIE decides whether the password is right. Only its "no" counts as a failure; a SIIE
    //    outage must not lock people out.
    let client = siie::new_client().map_err(siie_error)?;
    let creds = siie::Credentials {
        nif: b.identifier.trim().to_owned(),
        password: b.password,
        loja_escutista: matches!(b.method, Method::Loja),
    };
    let siie_login = match siie::login(&client, creds).await {
        Ok(l) => l,
        Err(e @ SiieError::LoginRejected(_)) => {
            record_login_failure(&s, &id_limit, ip_limit.as_deref()).await?;
            return Err(siie_error(e));
        }
        Err(e) => return Err(siie_error(e)),
    };
    // A correct password clears this identifier's failures (the IP's stay).
    sqlx::query("delete from login_failures where key = $1")
        .bind(&id_limit)
        .execute(&s.db)
        .await
        .map_err(db_error)?;

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

    // 3. Access and member imports, in the background. The access is re-read only if never
    //    read or expired. Imports are queued now when the access stands (the Painel shows them
    //    straight away), or after the read otherwise, since a first read is what creates the
    //    grants. The SIIE client and token move into that task; if there is nothing to do, they
    //    are dropped at the end of this request.
    let claim = access::claim_refresh(&s.db, account_id, s.access_validity_days)
        .await
        .map_err(db_error)?;
    let reading = matches!(claim, Claim::Started);
    let queued = if reading {
        Vec::new()
    } else {
        members::queue(&s.db, account_id, s.members_sync_days)
            .await
            .map_err(db_error)?
    };
    let imports = queued.len();
    if reading || !queued.is_empty() {
        let own = reading.then_some(siie::OwnId {
            loja_escutista: matches!(b.method, Method::Loja),
            identifier: id,
        });
        let task = members::sync(
            s.clone(),
            account_id,
            b.method,
            client,
            siie_login,
            own,
            queued,
        );
        in_background(vercel.as_ref().map(|Extension(v)| v), task);
    }

    // 4. Session + login event in one transaction, then answer right away.
    let token = new_token();
    let mut tx = s.db.begin().await.map_err(db_error)?;
    let detail = json!({ "access": claim.as_str(), "imports": imports });
    log_event(&mut *tx, Some(account_id), account_id, "login", detail)
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

/// 429 if this identifier or IP has too many recent SIIE failures.
async fn check_login_limits(
    s: &AppState,
    id_limit: &[u8],
    ip_limit: Option<&[u8]>,
) -> Result<(), ApiError> {
    let (by_id, by_ip): (i64, i64) = sqlx::query_as(
        "select count(*) filter (where key = $1), count(*) filter (where key = $2)
         from login_failures
         where key in ($1, $2) and at > now() - make_interval(mins => $3)",
    )
    .bind(id_limit)
    .bind(ip_limit)
    .bind(FAILURE_WINDOW_MINUTES)
    .fetch_one(&s.db)
    .await
    .map_err(db_error)?;
    if by_id >= MAX_FAILURES_PER_IDENTIFIER || by_ip >= MAX_FAILURES_PER_IP {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, TOO_MANY));
    }
    Ok(())
}

/// One SIIE rejection: a row per key, plus cleanup of rows past the window.
async fn record_login_failure(
    s: &AppState,
    id_limit: &[u8],
    ip_limit: Option<&[u8]>,
) -> Result<(), ApiError> {
    sqlx::query(
        "with old as (
            delete from login_failures where at < now() - make_interval(mins => $3)
         )
         insert into login_failures (key) select k from unnest(array[$1, $2]) as k where k is not null",
    )
    .bind(id_limit)
    .bind(ip_limit)
    .bind(FAILURE_WINDOW_MINUTES)
    .execute(&s.db)
    .await
    .map_err(db_error)?;
    Ok(())
}

/// The caller's IP. On Vercel, x-forwarded-for is set by the platform (client-sent values are
/// overwritten), so its first entry is the real client. Locally there is none.
fn client_ip(headers: &HeaderMap) -> Option<String> {
    let first = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))?
        .to_str()
        .ok()?
        .split(',')
        .next()?
        .trim();
    (!first.is_empty()).then(|| first.to_owned())
}

/// GET /api/auth/me: who is logged in and their access, or 401 (the frontend then shows only
/// the login page). The Painel polls this while the access is being read.
pub async fn me(State(s): State<AppState>, acc: CurrentAccount) -> Result<Response, ApiError> {
    let access = access::summary(&s, acc.id).await.map_err(db_error)?;
    let imports = members::summary(&s.db, acc.id).await.map_err(db_error)?;
    let body = json!({
        "accountId": acc.id,
        "label": acc.label,
        "role": acc.role,
        "access": access,
        "imports": imports,
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
    keyed_hash(secret, &siie::normalize_identifier(identifier))
}

/// HMAC-SHA256(secret, text). Callers prefix the text by purpose ("fail-ip:..."), so hashes for
/// different uses can't be matched against each other.
pub(crate) fn keyed_hash(secret: &[u8], text: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(text.as_bytes());
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
    fn login_limit_keys_differ_from_login_key() {
        let secret = [1; 32];
        let id = "123456789";
        assert_ne!(
            login_key(&secret, id),
            keyed_hash(&secret, &format!("fail-id:{id}"))
        );
    }

    #[test]
    fn client_ip_takes_first_forwarded_entry() {
        let mut h = HeaderMap::new();
        assert_eq!(client_ip(&h), None);
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7, 10.0.0.1"),
        );
        assert_eq!(client_ip(&h).as_deref(), Some("203.0.113.7"));
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
