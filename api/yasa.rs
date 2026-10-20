//! Vercel Function entry. vercel.json rewrites every /api/* request here, keeping the original
//! path, so the router in src/lib.rs matches it as usual.

use std::task::{Context, Poll};

use tower::{Layer, Service};
use vercel_runtime::{AppState as VercelState, Error, Request, axum::VercelLayer};

/// vercel_runtime's axum adapter drops the per-request runtime state, which carries
/// `wait_until`. This wrapper copies it into the request extensions first, so handlers can take
/// `Extension<vercel_runtime::AppState>` and keep work running after the response is sent.
#[derive(Clone)]
struct KeepVercelState<S>(S);

impl<S: Service<(VercelState, Request)>> Service<(VercelState, Request)> for KeepVercelState<S> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, (state, mut req): (VercelState, Request)) -> Self::Future {
        req.extensions_mut().insert(state.clone());
        self.0.call((state, req))
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Runs once per function instance. Fluid compute reuses the instance (and its DB pool)
    // across requests.
    let state = yasa::AppState::from_env(true)?;
    let app = KeepVercelState(VercelLayer::new().layer(yasa::router(state)));
    vercel_runtime::run(app).await
}
