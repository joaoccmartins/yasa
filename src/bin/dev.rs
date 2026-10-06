//! Local server: the same router as on Vercel, on loopback. The Vite dev server proxies /api here.

// Loopback only, never 0.0.0.0.
const ADDR: &str = "127.0.0.1:3000";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    // Plain http on localhost, so the session cookie goes out without the Secure flag.
    let state = yasa::AppState::from_env(false).unwrap_or_else(|e| panic!("{e}"));
    let listener = tokio::net::TcpListener::bind(ADDR)
        .await
        .expect("could not bind 127.0.0.1:3000");
    println!("YASA API listening on http://{ADDR}");
    axum::serve(listener, yasa::router(state))
        .await
        .expect("server error");
}
