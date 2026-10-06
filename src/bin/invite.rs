//! Invites someone from the command line. Needed once, to create the first admin:
//!
//!   cargo run --bin invite -- --method loja --identifier <NIF> --label "João" --admin
//!
//! `--method loja` = they will log in with their NIF (Loja Escutista), `siie` = with their NIN.
//! Uses DATABASE_URL and LOGIN_KEY_SECRET from the environment or .env, like the API.

use secrecy::ExposeSecret;
use yasa::{
    AppState,
    admin::{Role, create_account},
    auth::Method,
};

const USAGE: &str =
    "usage: invite --method loja|siie --identifier <NIF/NIN> --label <text> [--admin]";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Tiny flag parser: four flags don't justify a dependency.
    let (mut method, mut identifier, mut label, mut role) = (None, None, None, Role::Member);
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--method" => {
                method = match args.next().as_deref() {
                    Some("loja") => Some(Method::Loja),
                    Some("siie") => Some(Method::Siie),
                    _ => exit(USAGE),
                }
            }
            "--identifier" => identifier = args.next(),
            "--label" => label = args.next(),
            "--admin" => role = Role::Admin,
            _ => exit(USAGE),
        }
    }
    let (Some(method), Some(identifier), Some(label)) = (method, identifier, label) else {
        exit(USAGE)
    };

    let state = AppState::from_env(false).unwrap_or_else(|e| exit(&e));
    let secret = state.login_key_secret.expose_secret();
    match create_account(&state.db, secret, method, &identifier, &label, role, None).await {
        Ok(id) => println!("invited {label:?} as account {id}"),
        Err(e) => exit(&e.1),
    }
}

fn exit(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1)
}
