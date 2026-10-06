//! Local POC: log into SIIE with your own credentials and look at the raw JSON of one grid.
//! No logging, no storage: everything lives in memory for the duration of one request.

use yasa::siie::{self, GridBody, GridResponse};

use axum::{
    Form, Router,
    response::Html,
    routing::{get, post},
};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Map, Value};

// Loopback only, never 0.0.0.0.
const ADDR: &str = "127.0.0.1:3000";
const DEFAULT_PAGE_SIZE: u32 = 5;
const MAX_PAGE_SIZE: u32 = 1000;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/fetch", post(fetch));
    let listener = tokio::net::TcpListener::bind(ADDR)
        .await
        .expect("could not bind 127.0.0.1:3000");
    println!("SIIE POC listening on http://{ADDR}");
    axum::serve(listener, app).await.expect("server error");
}

/// Form body. No `Debug` on purpose: it holds the password.
#[derive(Deserialize)]
struct FetchForm {
    nif: String,
    password: SecretString,
    page_size: String,
    /// "loja" (NIF) or "siie" (NIN), same choice as the radio buttons on the SIIE login page.
    auth: String,
}

async fn index() -> Html<String> {
    page(&format!(
        r#"<h1>SIIE grid POC</h1>
<form method="post" action="/fetch" autocomplete="off">
  <label>NIF / NIN <input name="nif" required></label>
  <label>Password <input name="password" type="password" required></label>
  <label><input type="radio" name="auth" value="loja" checked> Loja Escutista (NIF + password)</label>
  <label><input type="radio" name="auth" value="siie"> SIIE (NIN + password)</label>
  <label>Page size <input name="page_size" type="number" min="1" max="{MAX_PAGE_SIZE}" value="{DEFAULT_PAGE_SIZE}"></label>
  <button>Fetch</button>
</form>"#
    ))
}

async fn fetch(Form(f): Form<FetchForm>) -> Html<String> {
    // Blank or invalid -> default; clamp so one click can't pull the whole database.
    let n = f
        .page_size
        .trim()
        .parse()
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let creds = siie::Credentials {
        nif: f.nif.trim().to_owned(),
        password: f.password,
        loja_escutista: f.auth != "siie",
    };
    match siie::fetch_grid(creds, n).await {
        Ok(r) => render_result(&r),
        // SiieError messages never contain the NIF or password.
        Err(e) => page(&format!(
            r#"<h1>Error</h1><p class="err">{}</p>{BACK}"#,
            esc(&e.to_string())
        )),
    }
}

fn render_result(r: &GridResponse) -> Html<String> {
    let meta = format!(
        "<dl><dt>HTTP status</dt><dd>{}</dd><dt>Final URL</dt><dd>{}</dd>\
         <dt>Content-Type</dt><dd>{}</dd></dl>",
        r.status,
        esc(&r.final_url),
        esc(&r.content_type)
    );

    let body = match &r.body {
        GridBody::Json(v) => {
            let top = match v {
                Value::Object(o) => join_keys(o),
                Value::Array(a) => format!("(bare array with {} items)", a.len()),
                _ => "(not an object)".to_owned(),
            };
            let fields = first_record(v).map_or("(no records)".to_owned(), join_keys);
            let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
            format!(
                "<h2>Top-level keys</h2><p><code>{}</code></p>\
                 <h2>Fields in the first record</h2><p><code>{}</code></p>\
                 <h2>JSON</h2><pre>{}</pre>",
                esc(&top),
                esc(&fields),
                esc(&pretty)
            )
        }
        GridBody::NotJson {
            snippet,
            on_login_page,
        } => {
            let why = if *on_login_page {
                "SIIE redirected to the login page: the session was not accepted."
            } else {
                "The grid endpoint did not return JSON."
            };
            format!(
                r#"<p class="err">{why}</p><h2>First 500 chars of the body</h2><pre>{}</pre>"#,
                esc(snippet)
            )
        }
    };

    // Discovery: does SIIE tell us who logged in (e.g. the NIN) beyond the token itself?
    let profile = serde_json::to_string_pretty(&r.login_profile).unwrap_or_default();
    let login = format!(
        "<h2>Login response (tokens removed)</h2><p>Keys: <code>{}</code></p><pre>{}</pre>",
        esc(&join_keys(&r.login_profile)),
        esc(&profile)
    );

    page(&format!("<h1>Result</h1>{meta}{login}{body}{BACK}"))
}

/// Kendo grids answer `{"Data": [...], "Count": n}`; also accept lowercase or a bare array.
fn first_record(v: &Value) -> Option<&Map<String, Value>> {
    let rows = match v {
        Value::Array(a) => a,
        _ => v.get("Data").or_else(|| v.get("data"))?.as_array()?,
    };
    rows.first()?.as_object()
}

fn join_keys(o: &Map<String, Value>) -> String {
    o.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
}

const BACK: &str = r#"<p><a href="/">Back</a></p>"#;

fn page(content: &str) -> Html<String> {
    Html(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>SIIE POC</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 960px; margin: 2rem auto; padding: 0 1rem; }}
label {{ display: block; margin: .5rem 0; }}
input {{ margin-left: .5rem; }}
pre {{ background: #f4f4f4; padding: 1rem; overflow: auto; }}
dt {{ font-weight: bold; }}
.err {{ color: #b00020; font-weight: bold; }}
</style></head><body>{content}</body></html>"#
    ))
}

/// Minimal HTML escaping for anything that came from SIIE.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
