//! Applies migrations/ to the database: `cargo run --bin migrate`.
//! Run it by hand or from CI, never from the Vercel function (no DDL on cold starts).

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    // Neon: migrations need a direct session, so prefer the unpooled URL (PgBouncer in
    // transaction mode doesn't hold the advisory lock sqlx takes while migrating).
    let url = std::env::var("DATABASE_URL_UNPOOLED")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("set DATABASE_URL_UNPOOLED or DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url)
        .await
        .expect("could not connect to the database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migration failed");
    println!("migrations applied");
}
