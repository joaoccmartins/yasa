// sqlx::migrate! embeds migrations/ at compile time. Without this, adding a migration doesn't
// rebuild the migrate binary, and `cargo run --bin migrate` silently applies the old set.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
