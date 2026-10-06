//! Vercel Function entry. vercel.json rewrites every /api/* request here, keeping the original
//! path, so the router in src/lib.rs matches it as usual.

use tower::ServiceBuilder;
use vercel_runtime::{Error, axum::VercelLayer};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Runs once per function instance. Fluid compute reuses the instance (and its DB pool)
    // across requests.
    let state = yasa::AppState::from_env(true)?;
    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(yasa::router(state));
    vercel_runtime::run(app).await
}
