//! Local POC: log into SIIE with your own credentials and look at its grids and pages.
//! No logging, no storage: everything lives in memory for the duration of one request.
//!
//! Modes:
//! - raw: one page of a report's grid as JSON (shows personal data, never paste it anywhere).
//! - profile: the whole grid, summarised per column (types, fill, shapes, common values only).
//! - links: the links, report paths, Kendo templates and column fields found in a page's HTML.
//!
//! profile and links never print raw rows, so their output can be shared for the field review.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use yasa::siie::{self, GridBody, GridResponse};

use axum::{
    Form, Router,
    response::Html,
    routing::{get, post},
};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Map, Value, json};

// Loopback only, never 0.0.0.0.
const ADDR: &str = "127.0.0.1:3000";
const DEFAULT_PAGE_SIZE: u32 = 5;
const MAX_PAGE_SIZE: u32 = 1000;
// Profile: a column's values are listed only if it has at most this many distinct ones, and then
// only the values at least MIN_VALUE_COUNT rows share, so a small grid can't reveal a person.
const MAX_LISTED_DISTINCT: usize = 40;
const MIN_VALUE_COUNT: u64 = 5;
// Stop counting distinct values past this (memory bound); the column then shows "> 5000".
const MAX_TRACKED_DISTINCT: usize = 5000;
const TOP_SHAPES: usize = 5;
// Links: digit runs this long or longer (NIN, NIF, phone) are masked in everything shown.
const MASK_DIGITS_FROM: usize = 6;
const MAX_SNIPPETS: usize = 40;
const MAX_LINK_HTML: usize = 300;
// The column the server-side filter test uses.
const FILTER_FIELD: &str = "agrupamento";

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
    /// "raw", "profile" or "links".
    mode: String,
    /// A report path (raw, profile) or a page path starting with `/` (links). Blank = default.
    target: String,
}

async fn index() -> Html<String> {
    let report = siie::MEMBERS_REPORT;
    page(&format!(
        r#"<h1>SIIE grid POC</h1>
<form method="post" action="/fetch" autocomplete="off">
  <label>NIF / NIN <input name="nif" required></label>
  <label>Password <input name="password" type="password" required></label>
  <label><input type="radio" name="auth" value="loja" checked> Loja Escutista (NIF + password)</label>
  <label><input type="radio" name="auth" value="siie"> SIIE (NIN + password)</label>
  <fieldset><legend>Mode</legend>
  <label><input type="radio" name="mode" value="profile" checked> Profile a report (shareable)</label>
  <label><input type="radio" name="mode" value="links"> Links on a page (shareable, check first)</label>
  <label><input type="radio" name="mode" value="raw"> Raw JSON, one page (personal data, don't share)</label>
  </fieldset>
  <label>Report(s) or page<br><textarea name="target" rows="8" cols="70" placeholder="{report}"></textarea></label>
  <p><small>Profile: one report per line, e.g. <code>elementos/progresso/lista-lobitos</code>
  (a pasted <code>/elementos/List?xml=...</code> page path works too); all are read with one
  login. Links: one page, e.g. <code>/</code> for the menu. Raw: one report.</small></p>
  <label>Page size (raw) <input name="page_size" type="number" min="1" max="{MAX_PAGE_SIZE}" value="{DEFAULT_PAGE_SIZE}"></label>
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
    let target = f.target.trim().to_owned();
    let creds = siie::Credentials {
        nif: f.nif.trim().to_owned(),
        password: f.password,
        loja_escutista: f.auth != "siie",
    };
    // One report per line; blank means dados completos.
    let mut reports: Vec<String> = target
        .lines()
        .map(report_path)
        .filter(|r| !r.is_empty())
        .collect();
    if reports.is_empty() {
        reports.push(siie::MEMBERS_REPORT.to_owned());
    }
    let result = match f.mode.as_str() {
        "raw" => siie::fetch_grid(creds, &reports[0], n)
            .await
            .map(|r| render_result(&r)),
        "links" => links(creds, if target.is_empty() { "/" } else { &target }).await,
        _ => profile(creds, &reports).await,
    };
    // SiieError messages never contain the NIF or password.
    result.unwrap_or_else(|e| {
        page(&format!(
            r#"<h1>Error</h1><p class="err">{}</p>{BACK}"#,
            esc(&e.to_string())
        ))
    })
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
                 {}\
                 <h2>JSON</h2><pre>{}</pre>",
                esc(&top),
                esc(&fields),
                seccoes_table(v),
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
/// Discovery for secções: how `seccao` / `siglaseccao` encode the type (1-4) and tell apart two
/// secções of the same type in one agrupamento. Organisation codes only, no personal data.
fn seccoes_table(v: &Value) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let rows = match v {
        Value::Array(a) => a.as_slice(),
        _ => match v
            .get("Data")
            .or_else(|| v.get("data"))
            .and_then(Value::as_array)
        {
            Some(a) => a.as_slice(),
            None => &[],
        },
    };
    // (seccao, siglaseccao) -> (row count, distinct agrupamentos)
    let mut pairs: BTreeMap<(String, String), (usize, BTreeSet<String>)> = BTreeMap::new();
    for row in rows {
        let raw = |f: &str| row.get(f).map_or("(missing)".to_owned(), Value::to_string);
        let entry = pairs
            .entry((raw("seccao"), raw("siglaseccao")))
            .or_default();
        entry.0 += 1;
        entry.1.insert(raw("agrupamento"));
    }
    let lines: String = pairs
        .iter()
        .map(|((s, sigla), (n, agrs))| {
            format!(
                "<tr><td><code>{}</code></td><td><code>{}</code></td><td>{n}</td><td>{}</td></tr>",
                esc(s),
                esc(sigla),
                agrs.len()
            )
        })
        .collect();
    format!(
        "<h2>Secções nesta página</h2>\
         <table border=1 cellpadding=4><tr><th>seccao</th><th>siglaseccao</th><th>rows</th>\
         <th>agrupamentos</th></tr>{lines}</table>"
    )
}

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

/// What profile mode keeps about one column. Values are counted, never shown unless common.
#[derive(Default)]
struct ColStats {
    types: BTreeSet<&'static str>,
    filled: u64,
    values: HashMap<String, u64>,
    values_overflow: bool,
    shapes: HashMap<String, u64>,
}

/// A report path from one input line. A pasted page path ("/elementos/List?xml=<report>&t=...")
/// also works: the report is what follows "xml=", up to any "&". Without this, SIIE gets the
/// prefix twice and answers 500.
fn report_path(line: &str) -> String {
    let line = line.trim();
    let report = match line.split_once("xml=") {
        Some((page, report)) if page.starts_with('/') => report,
        _ => line,
    };
    report.split('&').next().unwrap_or_default().to_owned()
}

/// Profiles each report with one login. A report that fails shows its error and the rest go on.
async fn profile(
    creds: siie::Credentials,
    reports: &[String],
) -> Result<Html<String>, siie::SiieError> {
    let client = siie::new_client()?;
    let login = siie::login(&client, creds).await?;
    let mut sections = String::new();
    for r in reports {
        sections.push_str(&match profile_one(&client, &login.token, r).await {
            Ok(html) => html,
            Err(e) => format!(
                r#"<h1>Profile: <code>{}</code></h1><p class="err">{}</p>"#,
                esc(r),
                esc(&e.to_string())
            ),
        });
    }
    Ok(page(&format!("{sections}{BACK}")))
}

/// Reads the whole report and summarises it per column, then checks whether SIIE honours a
/// server-side filter on `agrupamento` (decides how per-agrupamento imports fetch their rows).
async fn profile_one(
    client: &reqwest::Client,
    token: &SecretString,
    report_path: &str,
) -> Result<String, siie::SiieError> {
    let report = siie::open_report(client, report_path).await?;

    let mut cols: BTreeMap<String, ColStats> = BTreeMap::new();
    let mut filter_value: Option<Value> = None;
    let rows = siie::for_each_row(client, token, &report, None, |row| {
        for (k, v) in row {
            let c = cols.entry(k.clone()).or_default();
            let Some(text) = value_text(v) else {
                c.types.insert("null");
                continue;
            };
            c.types.insert(json_type(v));
            c.filled += 1;
            *c.shapes.entry(shape(&text)).or_default() += 1;
            if c.values.len() < MAX_TRACKED_DISTINCT || c.values.contains_key(&text) {
                *c.values.entry(text).or_default() += 1;
            } else {
                c.values_overflow = true;
            }
        }
        // First real agrupamento code (not a 0000 placeholder) is the filter test value.
        if filter_value.is_none() {
            filter_value = row
                .get(FILTER_FIELD)
                .filter(|v| value_text(v).is_some_and(|t| !t.chars().all(|c| c == '0')))
                .cloned();
        }
        Ok(())
    })
    .await?;

    let filter = match &filter_value {
        Some(v) => filter_test(client, token, &report, v, rows).await,
        None => {
            format!("<p>No <code>{FILTER_FIELD}</code> column with a value: filter not tested.</p>")
        }
    };

    let lines: String = cols
        .iter()
        .map(|(k, c)| {
            let fill = if rows == 0 { 0 } else { c.filled * 100 / rows };
            let distinct = if c.values_overflow {
                format!("> {MAX_TRACKED_DISTINCT}")
            } else {
                c.values.len().to_string()
            };
            let mut shapes: Vec<_> = c.shapes.iter().collect();
            shapes.sort_by(|a, b| b.1.cmp(a.1));
            let shapes: Vec<String> = shapes
                .iter()
                .take(TOP_SHAPES)
                .map(|(s, n)| format!("{s} ({n})"))
                .collect();
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{fill}%</td><td>{distinct}</td>\
                 <td><code>{}</code></td><td>{}</td></tr>",
                esc(k),
                c.types.iter().copied().collect::<Vec<_>>().join(", "),
                esc(&shapes.join(" | ")),
                esc(&common_values(c)),
            )
        })
        .collect();

    Ok(format!(
        "<h1>Profile: <code>{}</code></h1><p>{rows} rows, {} columns. Shapes: letters are \
         <code>a</code>, digits <code>9</code> (<code>9{{n}}</code> for long runs). Values are \
         listed only for columns with at most {MAX_LISTED_DISTINCT} distinct values, and only the \
         ones at least {MIN_VALUE_COUNT} rows share.</p>{filter}\
         <table border=1 cellpadding=4><tr><th>key</th><th>types</th><th>fill</th><th>distinct</th>\
         <th>shapes</th><th>common values</th></tr>{lines}</table>",
        esc(report_path),
        cols.len()
    ))
}

/// Reads one small page with a Kendo `eq` filter on FILTER_FIELD and compares it with the full read.
async fn filter_test(
    client: &reqwest::Client,
    token: &SecretString,
    report: &siie::Report,
    value: &Value,
    all_rows: u64,
) -> String {
    let filter = json!({"logic": "and", "filters": [{"field": FILTER_FIELD, "operator": "eq", "value": value}]});
    let shown = esc(&value_text(value).unwrap_or_default());
    match siie::grid_page(client, token, report, 0, 50, Some(&filter)).await {
        Ok((rows, total)) => {
            let matching = rows
                .iter()
                .filter(|r| r.get(FILTER_FIELD) == Some(value))
                .count();
            // Honoured = fewer rows than the whole grid, and every returned row has the value.
            let honoured = total.is_some_and(|t| t < all_rows) && matching == rows.len();
            format!(
                "<h2>Server-side filter</h2><p><code>{FILTER_FIELD} eq {shown}</code>: total {}, \
                 {matching} of {} returned rows match. <b>{}</b></p>",
                total.map_or("(none)".to_owned(), |t| t.to_string()),
                rows.len(),
                if honoured {
                    "SIIE honours the filter."
                } else {
                    "SIIE ignores the filter."
                }
            )
        }
        Err(e) => format!(
            r#"<h2>Server-side filter</h2><p class="err">Filtered read failed: {}</p>"#,
            esc(&e.to_string())
        ),
    }
}

/// Values shared by at least MIN_VALUE_COUNT rows, most common first, for low-cardinality columns.
fn common_values(c: &ColStats) -> String {
    if c.values_overflow || c.values.len() > MAX_LISTED_DISTINCT {
        return String::new();
    }
    let mut vals: Vec<_> = c.values.iter().collect();
    vals.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let (common, rare): (Vec<_>, Vec<_>) =
        vals.into_iter().partition(|(_, n)| **n >= MIN_VALUE_COUNT);
    let mut out: Vec<String> = common.iter().map(|(v, n)| format!("{v} ({n})")).collect();
    if !rare.is_empty() {
        out.push(format!("+{} rare hidden", rare.len()));
    }
    out.join(" | ")
}

/// A grid value as text; null and empty strings are "not filled".
fn value_text(v: &Value) -> Option<String> {
    let t = match v {
        Value::Null => return None,
        Value::String(s) => s.trim().to_owned(),
        other => other.to_string(),
    };
    (!t.is_empty()).then_some(t)
}

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The value's form without its content: a letter run becomes `a`, a digit run of n becomes n 9s
/// (or `9{n}` past 4), everything else is kept. "12/03/2011" -> "99/99/9999", "Ana Sá" -> "a a".
fn shape(text: &str) -> String {
    mask(text, 5, true).chars().take(40).collect()
}

/// Masks digit runs of at least `from` digits as `9{n}`, shorter ones as n 9s if `all_digits`
/// (else kept), and letter runs as `a` if `all_digits` (the profile shape) or kept (links mode).
fn mask(text: &str, from: usize, all_digits: bool) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut run = String::from(c);
            while let Some(d) = chars.next_if(char::is_ascii_digit) {
                run.push(d);
            }
            let n = run.len();
            if n >= from {
                out.push_str(&format!("9{{{n}}}"));
            } else if all_digits {
                out.push_str(&"9".repeat(n));
            } else {
                out.push_str(&run);
            }
        } else if all_digits && c.is_alphabetic() {
            while chars.next_if(|d| d.is_alphabetic()).is_some() {}
            out.push('a');
        } else {
            out.push(c);
        }
    }
    out
}

/// Lists what a page links to: hrefs with their text, `xml=` report paths, Kendo row templates
/// (`#= ... #`) and column `field` names. Long digit runs are masked in all of it.
async fn links(creds: siie::Credentials, path: &str) -> Result<Html<String>, siie::SiieError> {
    let client = siie::new_client()?;
    // The page needs the session cookies, not the token.
    drop(siie::login(&client, creds).await?);
    let html = siie::page_html(&client, path).await?;
    let m = |s: &str| esc(&mask(s, MASK_DIGITS_FROM, false));

    let hrefs: BTreeSet<(String, String)> = html
        .match_indices("href=")
        .filter_map(|(i, _)| {
            let rest = &html[i + 5..];
            let quote = rest.chars().next().filter(|q| *q == '"' || *q == '\'')?;
            let url: String = rest[1..].chars().take_while(|c| *c != quote).collect();
            // Contacts are personal and "#" leads nowhere; javascript: links stay (they may open a ficha).
            if url.is_empty()
                || url == "#"
                || ["mailto:", "tel:"].iter().any(|p| url.starts_with(p))
            {
                return None;
            }
            Some((url, link_text(rest)))
        })
        .collect();
    let xml_paths: BTreeSet<String> = html
        .match_indices("xml=")
        .map(|(i, _)| {
            html[i + 4..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "_-/.%+=".contains(*c))
                .collect()
        })
        .collect();
    let templates: BTreeSet<String> = html
        .match_indices("#=")
        .chain(html.match_indices("#:"))
        .take(MAX_SNIPPETS)
        .map(|(i, _)| context(&html, i, 60, 80))
        .collect();
    let fields: BTreeSet<String> = ["field", "title"]
        .iter()
        .flat_map(|key| {
            quoted_after(&html, key)
                .into_iter()
                .map(move |v| format!("{key}: {v}"))
        })
        .collect();

    let list = |items: Vec<String>| -> String {
        if items.is_empty() {
            "<p>(none)</p>".to_owned()
        } else {
            format!(
                "<ul>{}</ul>",
                items
                    .iter()
                    .map(|i| format!("<li>{i}</li>"))
                    .collect::<String>()
            )
        }
    };
    Ok(page(&format!(
        "<h1>Links: <code>{}</code></h1><p class=\"err\">Check this before sharing it: link texts \
         come from the page. Digit runs of {MASK_DIGITS_FROM}+ are masked.</p>\
         <h2>Report paths (xml=)</h2>{}<h2>Links</h2>{}<h2>Column fields and titles</h2>{}\
         <h2>Kendo templates</h2>{}{BACK}",
        esc(path),
        list(
            xml_paths
                .iter()
                .map(|x| format!("<code>{}</code>", m(x)))
                .collect()
        ),
        list(
            hrefs
                .iter()
                .map(|(u, t)| format!("<code>{}</code> {}", m(u), m(t)))
                .collect()
        ),
        list(
            fields
                .iter()
                .map(|f| format!("<code>{}</code>", m(f)))
                .collect()
        ),
        list(
            templates
                .iter()
                .map(|t| format!("<code>{}</code>", m(t)))
                .collect()
        ),
    )))
}

/// The visible text of the `<a>` that starts right before `rest`, tags stripped, 60 chars max.
fn link_text(rest: &str) -> String {
    let Some(open) = rest.find('>') else {
        return String::new();
    };
    // Only a `</a` close by counts, so a non-<a> href can't pull in the page's text.
    let inner = &rest[open + 1..];
    let inner = match inner.find("</a") {
        Some(end) if end <= MAX_LINK_HTML => &inner[..end],
        _ => "",
    };
    let mut text = String::new();
    let mut in_tag = false;
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(60)
        .collect()
}

/// Every quoted value after `key` followed by `:` (JS `field: "x"` or JSON `"field":"x"`).
fn quoted_after(html: &str, key: &str) -> Vec<String> {
    html.match_indices(key)
        .filter_map(|(i, _)| {
            let rest = html[i + key.len()..]
                .trim_start_matches(['"', '\''])
                .trim_start();
            let rest = rest.strip_prefix(':')?.trim_start();
            let quote = rest.chars().next().filter(|q| *q == '"' || *q == '\'')?;
            let v: String = rest[1..]
                .chars()
                .take_while(|c| *c != quote)
                .take(80)
                .collect();
            (!v.is_empty()).then_some(v)
        })
        .collect()
}

/// `before` chars before byte index `i` and `after` chars from it, counted in chars (UTF-8 safe).
fn context(html: &str, i: usize, before: usize, after: usize) -> String {
    let b: Vec<char> = html[..i].chars().rev().take(before).collect();
    let a: String = html[i..].chars().take(after).collect();
    format!("…{}{a}…", b.iter().rev().collect::<String>())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_path_accepts_page_paths() {
        assert_eq!(
            report_path(" elementos/cursos/lista-cursos "),
            "elementos/cursos/lista-cursos"
        );
        assert_eq!(
            report_path("/elementos/List?xml=elementos/habilitacoes/lista-habilitacoes&amp;t=X"),
            "elementos/habilitacoes/lista-habilitacoes"
        );
    }

    #[test]
    fn shape_hides_content() {
        assert_eq!(shape("12/03/2011"), "99/99/9999");
        assert_eq!(shape("Ana Sá, 12"), "a a, 99");
        assert_eq!(shape("123456789"), "9{9}");
    }

    #[test]
    fn links_mask_only_long_digit_runs() {
        assert_eq!(
            mask("/Ficha?nin=123456789&t=2", MASK_DIGITS_FROM, false),
            "/Ficha?nin=9{9}&t=2"
        );
    }

    #[test]
    fn common_values_hide_rare_ones() {
        let mut c = ColStats::default();
        c.values.insert("Ativo".into(), 9);
        c.values.insert("Raro".into(), 1);
        assert_eq!(common_values(&c), "Ativo (9) | +1 rare hidden");
    }
}
