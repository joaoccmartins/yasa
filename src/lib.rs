//! YASA backend: one axum router, served on Vercel (api/yasa.rs) or locally (src/bin/dev.rs).

pub mod access;
pub mod admin;
pub mod auth;
pub mod siie;

use std::{future::Future, sync::Arc};

use axum::{
    Router,
    routing::{get, patch, post},
};
use secrecy::SecretSlice;
use sqlx::{PgPool, postgres::PgPoolOptions};

// Serverless: every warm instance holds its own pool, so keep it small. Neon's pooled URL
// (PgBouncer) multiplexes these onto the real Postgres connections.
const MAX_DB_CONNECTIONS: u32 = 3;
// How long an access read from SIIE stays valid, in days, unless ACCESS_VALIDITY_DAYS says
// otherwise. A login after that re-reads it (in the background).
const DEFAULT_ACCESS_VALIDITY_DAYS: i32 = 7;
const MAX_ACCESS_VALIDITY_DAYS: i32 = 365;
// HMAC-SHA256 key: 32 bytes is the full hash width, shorter would weaken it.
const MIN_LOGIN_KEY_BYTES: usize = 32;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Key that turns a NIF/NIN into a user key (see auth.rs). Lives in env vars only, never in the DB.
    pub login_key_secret: Arc<SecretSlice<u8>>,
    /// false only for the local http server, where browsers would drop a Secure cookie on some setups.
    pub secure_cookies: bool,
    /// Days an access read from SIIE stays valid (ACCESS_VALIDITY_DAYS).
    pub access_validity_days: i32,
}

impl AppState {
    /// Reads DATABASE_URL and LOGIN_KEY_SECRET (hex). The pool connects lazily, so a cold start
    /// that only serves /api/health never opens a DB connection.
    pub fn from_env(secure_cookies: bool) -> Result<Self, String> {
        let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set")?;
        let db = PgPoolOptions::new()
            .max_connections(MAX_DB_CONNECTIONS)
            .connect_lazy(&url)
            .map_err(|e| format!("invalid DATABASE_URL: {e}"))?;

        let key_hex =
            std::env::var("LOGIN_KEY_SECRET").map_err(|_| "LOGIN_KEY_SECRET is not set")?;
        let key = hex::decode(key_hex.trim()).map_err(|_| "LOGIN_KEY_SECRET must be hex")?;
        if key.len() < MIN_LOGIN_KEY_BYTES {
            return Err(format!(
                "LOGIN_KEY_SECRET must be at least {MIN_LOGIN_KEY_BYTES} bytes (64 hex chars)"
            ));
        }

        // Optional: unset means the default; anything else must be a whole number of days.
        let access_validity_days = match std::env::var("ACCESS_VALIDITY_DAYS") {
            Err(_) => DEFAULT_ACCESS_VALIDITY_DAYS,
            Ok(v) => v
                .trim()
                .parse()
                .ok()
                .filter(|d| (1..=MAX_ACCESS_VALIDITY_DAYS).contains(d))
                .ok_or(format!(
                    "ACCESS_VALIDITY_DAYS must be a number of days from 1 to {MAX_ACCESS_VALIDITY_DAYS}"
                ))?,
        };

        Ok(Self {
            db,
            login_key_secret: Arc::new(key.into()),
            secure_cookies,
            access_validity_days,
        })
    }
}

/// Runs `fut` after the response is sent. On Vercel, `wait_until` keeps the invocation alive
/// until it finishes (bounded by the function's maxDuration); locally, a plain tokio task.
pub fn in_background<F>(vercel: Option<&vercel_runtime::AppState>, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    match vercel {
        Some(v) => v.wait_until(fut),
        None => {
            tokio::spawn(fut);
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/admin/accounts", get(admin::list).post(admin::invite))
        .route("/api/admin/accounts/{id}", patch(admin::update))
        .route("/api/admin/events", get(admin::events))
        .with_state(state)
}
