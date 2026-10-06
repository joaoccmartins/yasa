//! SIIE client: logs in with the user's own credentials and reads the members grid.
//! A fresh client (and cookie jar) is built per user action and dropped at the end.

use std::{collections::BTreeSet, time::Duration};

use reqwest::{Client, header, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value};

const BASE: &str = "https://siie.escutismo.pt";

// Login flow, found by reading /login and /Scripts/xSIIE.2026.1.js (checked 2026-09-27):
// - /login serves an ASP.NET WebForms shell (<form id="form1">). Its #username and #password
//   inputs have NO `name` attribute, so a plain form POST never carries credentials. Its only
//   hidden fields are __VIEWSTATE, __VIEWSTATEGENERATOR, __EVENTTARGET, __EVENTARGUMENT and
//   RadScriptManager1_TSM; there is no antiforgery token.
// - The "Entrar" button calls TokenLogin(), which POSTs x-www-form-urlencoded to /api/logintoken
//   and receives {"access_token": ...}. It also sets the .SIIE / .AspNet.ApplicationCookie
//   cookies. Grid reads send `Authorization: Bearer <token>` plus those cookies from the jar.
// - Bad credentials come back as HTTP 404 with {"Message": "O Utilizador ou password ..."}.
// - Both login options (SIIE and Loja Escutista) use that same endpoint; the radio button only
//   flips `adauthentication`.
const LOGIN_PAGE: &str = "/login?ReturnUrl=%2F";
const LOGIN_PATH: &str = "/login";
const TOKEN_PATH: &str = "/api/logintoken";
const USER_FIELD: &str = "username";
const PASS_FIELD: &str = "password";
// "false" = SIIE login (NIN + password), "true" = Loja Escutista login (NIF + password).
const ADAUTH_FIELD: &str = "adauthentication";

// Page that hosts the "dados completos" members grid. The grid's `xml` report id is encrypted
// per session (a value copied from the browser fails with "Padding is invalid and cannot be
// removed."), so it is read from this page's HTML right after logging in.
const GRID_PAGE: &str = "/elementos/List?xml=elementos/elementos/dados-completos";
const GRID_API_MARKER: &str = "api/generic?xml=";
const MAX_XML_MENTIONS: usize = 15;

// The site has bot detection, so every request goes out with a normal desktop browser UA.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

const SNIPPET_CHARS: usize = 500;

// Access: the grid columns that say where each member belongs (keys checked 2026-10-06).
// Agrupamentos are what access is cut on; núcleo and região only serve to tell the access level.
// If SIIE renames them, login fails with ScopeFieldMissing, which lists the real field names.
const AGRUPAMENTO_FIELD: &str = "agrupamento";
const NUCLEO_FIELD: &str = "nucleo";
const REGIAO_FIELD: &str = "regiao";
const SCOPE_FIELDS: [&str; 3] = [AGRUPAMENTO_FIELD, NUCLEO_FIELD, REGIAO_FIELD];
// Kendo page size when reading the whole grid, and a hard stop so a runaway loop can't hang a login.
const SCOPE_PAGE_SIZE: u64 = 1000;
const MAX_GRID_ROWS: u64 = 200_000;
// Fields of the token response that must never leave this module.
const TOKEN_RESPONSE_SECRETS: &[&str] = &["access_token", "refresh_token"];

#[derive(thiserror::Error, Debug)]
pub enum SiieError {
    #[error("Erro de rede ao contactar o SIIE: {0}")]
    Network(#[from] reqwest::Error),
    #[error("O SIIE recusou o início de sessão (HTTP {0}). Verifique o NIF/NIN e a palavra-passe.")]
    LoginRejected(u16),
    #[error(
        "O SIIE não devolveu um token de acesso (o processo de login mudou, ou a deteção de bots bloqueou o pedido)."
    )]
    NoToken,
    #[error("A página da grelha redirecionou para o login: o SIIE não aceitou a sessão.")]
    GridPageIsLogin,
    #[error(
        "Não foi encontrado \"{GRID_API_MARKER}\" em {GRID_PAGE}. Excertos da página com \"xml\": {0}"
    )]
    ReportIdNotFound(String),
    #[error("A grelha não devolveu JSON (sessão perdida, ou sem permissão para a ver).")]
    GridNotJson,
    #[error("A resposta da grelha tem um formato inesperado.")]
    UnexpectedShape,
    #[error("Faltam as colunas {SCOPE_FIELDS:?} na grelha. Colunas presentes: {0}")]
    ScopeFieldMissing(String),
    #[error("A grelha tem mais de {MAX_GRID_ROWS} linhas; a leitura foi recusada.")]
    TooManyRows,
}

/// Deliberately no `Debug`/`Clone`/`Serialize`, so it can't end up in logs by accident.
pub struct Credentials {
    pub nif: String,
    pub password: SecretString,
    /// true = log in through Loja Escutista (NIF), false = through SIIE (NIN).
    pub loja_escutista: bool,
}

pub struct GridResponse {
    pub status: u16,
    pub final_url: String,
    pub content_type: String,
    pub body: GridBody,
    /// Discovery aid: the token response minus the tokens (see `LoginOk::profile`).
    pub login_profile: Map<String, Value>,
}

/// A successful SIIE login. No `Debug`: it holds the bearer token.
pub struct LoginOk {
    pub token: SecretString,
    /// The rest of the token response (e.g. user name, expiry), with the tokens removed.
    /// Used to discover whether SIIE tells us the person's NIN.
    pub profile: Map<String, Value>,
}

pub enum GridBody {
    Json(Value),
    /// Not JSON (e.g. redirected to the login page). Holds the first 500 chars for debugging.
    NotJson {
        snippet: String,
        on_login_page: bool,
    },
}

/// Logs in, reads one page of the grid and returns it as-is.
pub async fn fetch_grid(creds: Credentials, page_size: u32) -> Result<GridResponse, SiieError> {
    let client = new_client()?;
    let login = login(&client, creds).await?;
    let report_xml = report_xml(&client).await?;
    let resp = grid_request(&client, &login.token, &report_xml, 0, page_size.into()).await?;
    let login_profile = login.profile;

    let status = resp.status().as_u16();
    let final_url = resp.url().to_string();
    let on_login_page = is_login_page(&resp);
    let content_type = content_type(&resp);
    let text = resp.text().await?;

    // JSON only if the server says so AND it parses; anything else is shown as a short snippet.
    let json = if content_type.contains("json") && !on_login_page {
        serde_json::from_str(&text).ok()
    } else {
        None
    };
    let body = match json {
        Some(v) => GridBody::Json(v),
        None => GridBody::NotJson {
            snippet: text.chars().take(SNIPPET_CHARS).collect(),
            on_login_page,
        },
    };

    Ok(GridResponse {
        status,
        final_url,
        content_type,
        body,
        login_profile,
    })
    // `client` (and its session cookies) is dropped here: nothing persists.
}

/// How far a person's SIIE access reaches. Ordered from narrowest to widest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccessLevel {
    None,
    Agrupamento,
    Nucleo,
    Regiao,
    Nacional,
}

impl AccessLevel {
    /// The value stored in `accounts.access_level`.
    pub fn as_str(self) -> &'static str {
        match self {
            AccessLevel::None => "none",
            AccessLevel::Agrupamento => "agrupamento",
            AccessLevel::Nucleo => "nucleo",
            AccessLevel::Regiao => "regiao",
            AccessLevel::Nacional => "nacional",
        }
    }
}

/// What the members grid shows one person: the distinct places its rows belong to.
#[derive(Default)]
pub struct Visibility {
    pub agrupamentos: BTreeSet<String>,
    pub nucleos: BTreeSet<String>,
    pub regioes: BTreeSet<String>,
}

impl Visibility {
    /// The level is read from how wide the visible set is, widest spread first:
    /// - nacional: more than 1 região
    /// - regiao: more than 1 núcleo
    /// - agrupamento: exactly 1 agrupamento. Checked before núcleo, because someone with one
    ///   agrupamento also sees exactly 1 núcleo (the one it belongs to).
    /// - nucleo: 1 núcleo, or several agrupamentos
    /// - none: nothing of the above (e.g. only a região with no núcleo or agrupamento under it)
    ///
    /// Placeholder codes (0000...) never reach these sets, see `field_text`.
    /// Known blind spot (SIIE doesn't expose the role itself): núcleo access to a núcleo with a
    /// single agrupamento reads as "agrupamento". The agrupamentos list is exact either way, and
    /// it is what access is cut on.
    pub fn level(&self) -> AccessLevel {
        if self.regioes.len() > 1 {
            AccessLevel::Nacional
        } else if self.nucleos.len() > 1 {
            AccessLevel::Regiao
        } else if self.agrupamentos.len() == 1 {
            AccessLevel::Agrupamento
        } else if self.nucleos.len() == 1 || self.agrupamentos.len() > 1 {
            AccessLevel::Nucleo
        } else {
            AccessLevel::None
        }
    }
}

/// Reads the whole grid with this session and collects where its members belong.
/// Rows are dropped as soon as those three fields are read: nothing about individual members
/// (names, NIFs, contacts...) leaves this function.
pub async fn visibility(client: &Client, token: &SecretString) -> Result<Visibility, SiieError> {
    let report_xml = report_xml(client).await?;
    let mut vis = Visibility::default();
    let mut skip = 0u64;
    loop {
        let resp = grid_request(client, token, &report_xml, skip, SCOPE_PAGE_SIZE).await?;
        if is_login_page(&resp) || !content_type(&resp).contains("json") {
            return Err(SiieError::GridNotJson);
        }
        let (rows, total) = split_kendo(resp.json().await?)?;
        for row in &rows {
            let row = row.as_object().ok_or(SiieError::UnexpectedShape)?;
            // A missing key means SIIE renamed the column. Field names (not values) help fix it.
            if SCOPE_FIELDS.iter().any(|f| !row.contains_key(*f)) {
                let fields = row.keys().map(String::as_str).collect::<Vec<_>>();
                return Err(SiieError::ScopeFieldMissing(fields.join(", ")));
            }
            // Null/empty values (e.g. regional staff outside any agrupamento) add nothing.
            for (field, set) in [
                (AGRUPAMENTO_FIELD, &mut vis.agrupamentos),
                (NUCLEO_FIELD, &mut vis.nucleos),
                (REGIAO_FIELD, &mut vis.regioes),
            ] {
                if let Some(v) = field_text(&row[field]) {
                    set.insert(v);
                }
            }
        }

        // Paging: stop on a short page or once `total` rows are read. Never silently truncate.
        let n = rows.len() as u64;
        skip += n;
        if n < SCOPE_PAGE_SIZE || total.is_some_and(|t| skip >= t) {
            return Ok(vis);
        }
        if skip >= MAX_GRID_ROWS {
            return Err(SiieError::TooManyRows);
        }
    }
}

/// Grid values come as strings or numbers depending on the column; normalise to trimmed text.
/// SIIE fills "no região/núcleo/agrupamento" with zeros (região "00", núcleo or agrupamento
/// "0000"), so an all-zero code is treated like null: it is not a place anyone has access to.
fn field_text(v: &Value) -> Option<String> {
    let text = match v {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let placeholder = text.chars().all(|c| c == '0'); // also true for "", so empty is skipped too
    (!placeholder).then_some(text)
}

/// One page of the grid. `skip`/`take` are Kendo's paging params.
async fn grid_request(
    client: &Client,
    token: &SecretString,
    report_xml: &str,
    skip: u64,
    take: u64,
) -> Result<reqwest::Response, SiieError> {
    // Kendo DataSource read params: the grid's parameterMap does JSON.stringify(data) and jQuery
    // appends it as a bare query component (no `key=`). Built by hand since `.query()` adds a key.
    let page = skip / take + 1;
    let params =
        format!(r#"{{"take":{take},"skip":{skip},"page":{page},"pageSize":{take},"sort":[]}}"#)
            .replace('"', "%22");
    let url = format!("{BASE}/web/api/generic?xml={report_xml}&{params}");
    Ok(client
        .get(&url)
        .header("X-Requested-With", "XMLHttpRequest")
        .header(header::REFERER, format!("{BASE}{GRID_PAGE}"))
        .bearer_auth(token.expose_secret())
        .send()
        .await?)
}

/// Kendo answers `{"Data": [...], "Total": n}` (casing and the count key vary), or a bare array.
fn split_kendo(body: Value) -> Result<(Vec<Value>, Option<u64>), SiieError> {
    match body {
        Value::Array(rows) => Ok((rows, None)),
        Value::Object(mut o) => {
            let total = ["Total", "total", "Count", "count"]
                .iter()
                .find_map(|k| o.get(*k).and_then(Value::as_u64));
            match o.remove("Data").or_else(|| o.remove("data")) {
                Some(Value::Array(rows)) => Ok((rows, total)),
                _ => Err(SiieError::UnexpectedShape),
            }
        }
        _ => Err(SiieError::UnexpectedShape),
    }
}

fn is_login_page(resp: &reqwest::Response) -> bool {
    resp.url()
        .path()
        .to_ascii_lowercase()
        .starts_with(LOGIN_PATH)
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// Opens the grid page like a browser navigation (cookies only) and returns this session's
/// encrypted report id, ready to drop into the grid URL.
async fn report_xml(client: &Client) -> Result<String, SiieError> {
    let resp = client
        .get(format!("{BASE}{GRID_PAGE}"))
        .send()
        .await?
        .error_for_status()?;
    if is_login_page(&resp) {
        return Err(SiieError::GridPageIsLogin);
    }
    let html = resp.text().await?;
    find_report_xml(&html).ok_or_else(|| SiieError::ReportIdNotFound(xml_mentions(&html)))
}

/// Takes the value after the first "api/generic?xml=" in the page.
fn find_report_xml(html: &str) -> Option<String> {
    let start = html.find(GRID_API_MARKER)? + GRID_API_MARKER.len();
    // The value is base64 (A-Z a-z 0-9 + / =), possibly already percent-encoded; it ends at the
    // first quote, `&` or anything else outside that set.
    let raw: String = html[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || "%+/=".contains(*c))
        .collect();
    // Base64's + / = must be percent-encoded in a query (a bare + would decode to a space).
    // Existing %xx sequences pass through untouched.
    let encoded = raw
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D");
    (!encoded.is_empty()).then_some(encoded)
}

/// Debug aid when the marker is missing: short context around each "xml" in the page.
fn xml_mentions(html: &str) -> String {
    let found: Vec<String> = html
        .match_indices("xml")
        .take(MAX_XML_MENTIONS)
        .map(|(i, _)| {
            // 40 chars before and 80 after, counted in chars so we never split a UTF-8 char.
            let before: Vec<char> = html[..i].chars().rev().take(40).collect();
            let after: String = html[i..].chars().take(80).collect();
            format!("…{}{after}…", before.iter().rev().collect::<String>())
        })
        .collect();
    if found.is_empty() {
        "(none)".to_owned()
    } else {
        found.join(" | ")
    }
}

/// Fresh client = fresh cookie jar. Build one per user action and drop it at the end.
/// NEVER share a cookie-storing client between requests: sessions would leak across users.
pub fn new_client() -> Result<Client, SiieError> {
    Ok(Client::builder()
        .cookie_store(true)
        .user_agent(USER_AGENT)
        .redirect(Policy::limited(5))
        .timeout(Duration::from_secs(60))
        .build()?)
}

/// Same steps the browser takes when the user clicks "Entrar". Returns the bearer token plus
/// whatever else the token response says about the user.
pub async fn login(client: &Client, creds: Credentials) -> Result<LoginOk, SiieError> {
    // 1. GET the login page like a browser would, to pick up session / bot-detection cookies.
    //    Nothing in its HTML is needed: the token endpoint takes no hidden fields.
    let login_url = format!("{BASE}{LOGIN_PAGE}");
    client
        .get(&login_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    // 2. The POST TokenLogin() makes (see the comment at the top of this file).
    let form = [
        ("grant_type", "password"),
        (USER_FIELD, creds.nif.as_str()),
        (PASS_FIELD, creds.password.expose_secret()),
        (
            ADAUTH_FIELD,
            if creds.loja_escutista {
                "true"
            } else {
                "false"
            },
        ),
    ];
    let resp = client
        .post(format!("{BASE}{TOKEN_PATH}"))
        .header("X-Requested-With", "XMLHttpRequest")
        .header(header::REFERER, &login_url)
        .form(&form)
        .send()
        .await?;
    drop(creds); // plaintext copies gone as soon as the request is sent

    // 3. Success = 2xx JSON with an access_token. Bad credentials come back as a 4xx (404).
    //    The server's error message is not shown, in case it echoes the username.
    let status = resp.status();
    if !status.is_success() {
        return Err(SiieError::LoginRejected(status.as_u16()));
    }
    let Value::Object(mut profile) = resp.json().await.map_err(|_| SiieError::NoToken)? else {
        return Err(SiieError::NoToken);
    };
    let token = match profile.remove("access_token") {
        Some(Value::String(t)) => SecretString::from(t),
        _ => return Err(SiieError::NoToken),
    };
    for k in TOKEN_RESPONSE_SECRETS {
        profile.remove(*k);
    }
    Ok(LoginOk { token, profile })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn split_kendo_reads_both_shapes() {
        let (rows, total) = split_kendo(json!({"Data": [{"a": 1}], "Total": 7})).unwrap();
        assert_eq!((rows.len(), total), (1, Some(7)));
        let (rows, total) = split_kendo(json!({"data": [], "Count": 0})).unwrap();
        assert_eq!((rows.len(), total), (0, Some(0)));
        let (rows, total) = split_kendo(json!([{"a": 1}, {"a": 2}])).unwrap();
        assert_eq!((rows.len(), total), (2, None));
        assert!(split_kendo(json!({"nope": 1})).is_err());
    }

    fn vis(agrupamentos: &[&str], nucleos: &[&str], regioes: &[&str]) -> Visibility {
        let set = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect();
        Visibility {
            agrupamentos: set(agrupamentos),
            nucleos: set(nucleos),
            regioes: set(regioes),
        }
    }

    #[test]
    fn level_rules() {
        use AccessLevel::*;
        assert_eq!(vis(&[], &[], &[]).level(), None);
        // Only a região, nothing under it.
        assert_eq!(vis(&[], &[], &["R1"]).level(), None);
        // One agrupamento also means one núcleo and one região: still agrupamento.
        assert_eq!(vis(&["53"], &["N1"], &["R1"]).level(), Agrupamento);
        // One núcleo, with several agrupamentos or none at all.
        assert_eq!(vis(&["53", "54"], &["N1"], &["R1"]).level(), Nucleo);
        assert_eq!(vis(&[], &["N1"], &["R1"]).level(), Nucleo);
        // Several agrupamentos even without núcleo codes.
        assert_eq!(vis(&["53", "54"], &[], &[]).level(), Nucleo);
        assert_eq!(vis(&["53", "60"], &["N1", "N2"], &["R1"]).level(), Regiao);
        assert_eq!(
            vis(&["53", "90"], &["N1", "N9"], &["R1", "R2"]).level(),
            Nacional
        );
    }

    #[test]
    fn field_text_accepts_strings_and_numbers() {
        assert_eq!(field_text(&json!(" 53 ")), Some("53".to_owned()));
        assert_eq!(field_text(&json!(53)), Some("53".to_owned()));
        assert_eq!(field_text(&json!("")), None);
        assert_eq!(field_text(&Value::Null), None);
    }

    #[test]
    fn field_text_skips_zero_placeholders() {
        assert_eq!(field_text(&json!("00")), None);
        assert_eq!(field_text(&json!("0000")), None);
        assert_eq!(field_text(&json!(0)), None);
        // Leading zeros on a real code are kept as-is.
        assert_eq!(field_text(&json!("0053")), Some("0053".to_owned()));
        assert_eq!(field_text(&json!("1000")), Some("1000".to_owned()));
    }
}
