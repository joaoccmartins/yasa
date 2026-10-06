//! YASA backend: one axum router, served on Vercel (api/yasa.rs) or locally (src/bin/dev.rs).

pub mod admin;
pub mod auth;
pub mod siie;

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, patch, post},
};
use secrecy::SecretSlice;
use sqlx::{PgPool, postgres::PgPoolOptions};

// Serverless: every warm instance holds its own pool, so keep it small. Neon's pooled URL
// (PgBouncer) multiplexes these onto the real Postgres connections.
const MAX_DB_CONNECTIONS: u32 = 3;
// HMAC-SHA256 key: 32 bytes is the full hash width, shorter would weaken it.
const MIN_LOGIN_KEY_BYTES: usize = 32;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Key that turns a NIF/NIN into a user key (see auth.rs). Lives in env vars only, never in the DB.
    pub login_key_secret: Arc<SecretSlice<u8>>,
    /// false only for the local http server, where browsers would drop a Secure cookie on some setups.
    pub secure_cookies: bool,
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

        Ok(Self {
            db,
            login_key_secret: Arc::new(key.into()),
            secure_cookies,
        })
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
        .with_state(state)
}
