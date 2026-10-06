//! SIIE fetcher: logs in as the user, pulls the member grid as JSON, and keeps only
//! an allowlist of non-identifying fields. Raw records never leave this module.
//!
//! Cargo.toml:
//!   reqwest    = { version = "0.12", default-features = false, features = ["cookies", "json", "rustls-tls"] }
//!   scraper    = "0.20"
//!   secrecy    = "0.10"
//!   serde_json = "1"
//!   thiserror  = "1"
//!   url        = "2"
//!   hmac       = "0.12"
//!   sha2       = "0.10"
//!   hex        = "0.4"

use hmac::{Hmac, Mac};
use reqwest::{header::CONTENT_TYPE, redirect::Policy, Client, Response};
use scraper::{Html, Selector};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value};
use sha2::Sha256;
use std::time::Duration;

const BASE: &str = "https://siie.escutismo.pt";
const LOGIN_PATH: &str = "/login";

// Fill these in from DevTools: Network tab -> the login POST -> Payload.
const USER_FIELD: &str = "Username"; // TODO: confirm the real field name
const PASS_FIELD: &str = "Password"; // TODO: confirm the real field name

// Opaque report id from the grid request. TODO: verify it is the same across sessions
// and users; if it changes, scrape it from the page that hosts the grid instead.
const REPORT_XML: &str = "zfzdyI0DtU0lqGtZu87pa6a%2b6S7oL73jCmoWPvI6g97EDM37Zc61l9WSpCLGt3iK";

const PAGE_SIZE: u64 = 1000;
const MAX_ROWS: u64 = 200_000;

/// What to do with each kept field.
enum Keep {
    AsIs,
    /// Reduce a date to its year.
    Year,
    /// Replace with a keyed hash: stable across syncs (for dedup/joins), not reversible.
    Pseudonym,
}

/// Only these fields survive ingestion; everything else is discarded in memory.
/// (source JSON property, stored name, treatment)
/// TODO: the JSON property names probably differ from the Excel headers used here.
///       Copy the real ones from a grid response in DevTools.
const ALLOWLIST: &[(&str, &str, Keep)] = &[
    ("NIN", "member_id", Keep::Pseudonym),
    ("Agrupamento", "agrupamento", Keep::AsIs),
    ("Núcleo", "nucleo", Keep::AsIs),
    ("Região", "regiao", Keep::AsIs),
    ("Situação", "situacao", Keep::AsIs),
    ("Categoria", "categoria", Keep::AsIs),
    ("Secção", "seccao", Keep::AsIs),
    ("Sexo", "sexo", Keep::AsIs),
    ("Idade", "idade", Keep::AsIs),
    ("Dt. Admissão", "ano_admissao", Keep::Year),
    ("Dt. Promessa", "ano_promessa", Keep::Year),
    ("Formador", "formador", Keep::AsIs),
    ("Diretor formação", "diretor_formacao", Keep::AsIs),
];

#[derive(thiserror::Error, Debug)]
pub enum SiieError {
    #[error("network error talking to SIIE")]
    Network(#[from] reqwest::Error),
    #[error("login form not found (page layout may have changed)")]
    FormNotFound,
    #[error("SIIE rejected the login")]
    LoginRejected,
    #[error("grid endpoint did not return JSON (session lost or no permission)")]
    NotJson,
    #[error("grid response had an unexpected shape")]
    UnexpectedShape,
}

/// Deliberately no `Debug`/`Clone`/`Serialize`, so it can't end up in logs by accident.
pub struct SiieCredentials {
    pub nif: String,
    pub password: SecretString,
}

pub type MemberRow = Map<String, Value>;

/// Logs in with the user's own credentials and returns the allowlisted rows.
/// `pseudonym_key` is a server-side secret (e.g. from KMS/secrets manager), never stored with the data.
pub async fn fetch_members(
    creds: SiieCredentials,
    pseudonym_key: &[u8],
) -> Result<Vec<MemberRow>, SiieError> {
    // Fresh client = fresh cookie jar for every call.
    // NEVER share a cookie-storing client between users: sessions would leak across accounts.
    let client = Client::builder()
        .cookie_store(true)
        .redirect(Policy::limited(5))
        .timeout(Duration::from_secs(60))
        .build()?;

    login(&client, creds).await?;

    let mut rows = Vec::new();
    let mut skip = 0u64;
    loop {
        let page = skip / PAGE_SIZE + 1;
        // Kendo DataSource params, appended as a bare query component (no `key=`),
        // exactly as the browser sends them. Build the string by hand; `.query()` would add a key.
        let params = format!(
            r#"{{"take":{PAGE_SIZE},"skip":{skip},"page":{page},"pageSize":{PAGE_SIZE},"sort":[]}}"#
        )
        .replace('"', "%22");
        let url = format!("{BASE}/web/api/generic?xml={REPORT_XML}&{params}");

        let resp = client
            .get(&url)
            // TODO: copy any other request headers the browser sends (e.g. an antiforgery header).
            .header("X-Requested-With", "XMLHttpRequest")
            .send()
            .await?
            .error_for_status()?;
        if is_login_page(&resp) || !is_json(&resp) {
            return Err(SiieError::NotJson);
        }

        let (items, total) = split_kendo(resp.json().await?)?;
        let n = items.len() as u64;
        for item in items {
            rows.push(project(item, pseudonym_key)?); // raw record dropped here
        }

        skip += n;
        let done = n < PAGE_SIZE || total.map_or(false, |t| skip >= t);
        if done || skip >= MAX_ROWS {
            break;
        }
    }
    Ok(rows)
    // `client` (and its session cookie) is dropped here: nothing persists.
}

async fn login(client: &Client, creds: SiieCredentials) -> Result<(), SiieError> {
    // 1. GET the login page: sets the antiforgery cookie and gives us the hidden token field(s).
    let login_url = format!("{BASE}{LOGIN_PATH}?ReturnUrl=%2F");
    let html = client
        .get(&login_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let (action, mut fields) = parse_login_form(&html)?;

    // 2. POST credentials together with every hidden field (antiforgery token, viewstate, ...).
    fields.push((USER_FIELD.to_owned(), creds.nif));
    fields.push((PASS_FIELD.to_owned(), creds.password.expose_secret().to_owned()));
    let resp = client.post(&action).form(&fields).send().await?;
    drop(fields); // plaintext copy gone as soon as it's sent
    drop(creds.password);

    // ASP.NET redirects away from /login on success and re-renders the login page on failure.
    if is_login_page(&resp) {
        return Err(SiieError::LoginRejected);
    }
    let _ = resp.bytes().await; // finish the response so cookies are fully processed
    Ok(())
}

/// Accepts `{"Data": [...], "Total": n}` (either casing) or a bare array.
fn split_kendo(body: Value) -> Result<(Vec<Value>, Option<u64>), SiieError> {
    match body {
        Value::Array(items) => Ok((items, None)),
        Value::Object(mut o) => {
            let total = o
                .get("Total")
                .or_else(|| o.get("total"))
                .and_then(Value::as_u64);
            let items = match o.remove("Data").or_else(|| o.remove("data")) {
                Some(Value::Array(a)) => a,
                _ => return Err(SiieError::UnexpectedShape),
            };
            Ok((items, total))
        }
        _ => Err(SiieError::UnexpectedShape),
    }
}

fn project(item: Value, key: &[u8]) -> Result<MemberRow, SiieError> {
    let Value::Object(src) = item else {
        return Err(SiieError::UnexpectedShape);
    };
    let mut out = Map::new();
    for (src_key, dst_key, how) in ALLOWLIST {
        let Some(v) = src.get(*src_key) else { continue };
        let v = match how {
            Keep::AsIs => v.clone(),
            Keep::Year => year_of(v).map(Value::from).unwrap_or(Value::Null),
            Keep::Pseudonym => Value::from(pseudonym(v, key)),
        };
        out.insert((*dst_key).to_owned(), v);
    }
    Ok(out)
}

fn pseudonym(v: &Value, key: &[u8]) -> String {
    let id = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(id.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// First 4-digit run in a date string: works for ISO ("2014-09-20T00:00:00") and "20/09/2014".
/// TODO: if the API returns old-style "/Date(1411171200000)/" values, parse the epoch instead.
fn year_of(v: &Value) -> Option<u16> {
    let s = v.as_str()?;
    let w = s
        .as_bytes()
        .windows(4)
        .find(|w| w.iter().all(u8::is_ascii_digit))?;
    std::str::from_utf8(w).ok()?.parse().ok()
}

fn is_login_page(resp: &Response) -> bool {
    resp.url().path().to_ascii_lowercase().starts_with(LOGIN_PATH)
}

fn is_json(resp: &Response) -> bool {
    resp.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map_or(false, |ct| ct.contains("json"))
}

/// Finds the form containing a password input and collects its hidden fields.
/// Sync on purpose: `scraper::Html` is not `Send`, so it must not live across an `.await`.
fn parse_login_form(html: &str) -> Result<(String, Vec<(String, String)>), SiieError> {
    let doc = Html::parse_document(html);
    let form_sel = Selector::parse("form").unwrap();
    let pw_sel = Selector::parse("input[type=password]").unwrap();
    let hidden_sel = Selector::parse("input[type=hidden]").unwrap();

    // TODO: the page offers "SIIE" and "Loja Escutista" logins. If both are separate forms,
    // filter on the SIIE form's action or id here instead of taking the first match.
    let form = doc
        .select(&form_sel)
        .find(|f| f.select(&pw_sel).next().is_some())
        .ok_or(SiieError::FormNotFound)?;

    let action = form.value().attr("action").unwrap_or(LOGIN_PATH);
    let action = url::Url::parse(BASE)
        .and_then(|b| b.join(action))
        .map_err(|_| SiieError::FormNotFound)?
        .to_string();

    let fields = form
        .select(&hidden_sel)
        .filter_map(|i| {
            let name = i.value().attr("name")?.to_owned();
            let value = i.value().attr("value").unwrap_or_default().to_owned();
            Some((name, value))
        })
        .collect();

    Ok((action, fields))
}
