//! SIIE client: logs in with the user's own credentials and reads the members grid.
//! A fresh client (and cookie jar) is built per user action and dropped at the end.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

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

// Every SIIE list screen is this prefix + a report path (e.g. elementos/progresso/lista-lobitos).
// The grid's `xml` report id is encrypted per session (a value copied from the browser fails with
// "Padding is invalid and cannot be removed."), so it is read from the page's HTML after login.
const GRID_PAGE_PREFIX: &str = "/elementos/List?xml=";
/// The "dados completos" members grid: access and member data are both read from it.
pub const MEMBERS_REPORT: &str = "elementos/elementos/dados-completos";
/// The agrupamentos list: general data per agrupamento. Not under GRID_PAGE_PREFIX, so it is
/// opened by its full page path.
pub const AGRUPAMENTOS_PAGE: &str = "/agrupamentos/list?xml=agrupamentos/agrupamento/agrupamento";
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
const AGRUPAMENTO_NAME_FIELD: &str = "agrupdesc";
// Identity columns, used only to find the logged-in person's own row (see `OwnId`).
const NIF_FIELD: &str = "nif";
const NIN_FIELD: &str = "nin";
// Kendo page size when reading a whole grid, and a hard stop so a runaway loop can't hang a login.
const GRID_PAGE_SIZE: u64 = 1000;
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
        "Não foi encontrado \"{GRID_API_MARKER}\" em {page}. Excertos da página com \"xml\": {mentions}"
    )]
    ReportIdNotFound { page: String, mentions: String },
    #[error("A grelha não devolveu JSON (sessão perdida, ou sem permissão para a ver).")]
    GridNotJson,
    #[error("A resposta da grelha tem um formato inesperado: {0}.")]
    UnexpectedShape(String),
    #[error("Faltam as colunas {SCOPE_FIELDS:?} na grelha. Colunas presentes: {0}")]
    ScopeFieldMissing(String),
    #[error("A grelha tem mais de {MAX_GRID_ROWS} linhas; a leitura foi recusada.")]
    TooManyRows,
    #[error("A coluna \"{field}\" da grelha tem um código que não é um número: \"{value}\".")]
    BadCode { field: &'static str, value: String },
    #[error("Faltam colunas de elementos na grelha: {0}")]
    MemberFieldMissing(String),
    #[error("A coluna \"{0}\" da grelha tem uma data num formato inesperado.")]
    BadDate(String),
    #[error("Uma linha da grelha não tem NIN.")]
    MissingNin,
    /// Any error while reading a report, with the report it came from.
    #[error("{report}: {source}")]
    InReport {
        report: String,
        source: Box<SiieError>,
    },
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

/// Logs in, reads one page of a report's grid and returns it as-is.
pub async fn fetch_grid(
    creds: Credentials,
    xml_path: &str,
    page_size: u32,
) -> Result<GridResponse, SiieError> {
    let client = new_client()?;
    let login = login(&client, creds).await?;
    let report = open_report(&client, xml_path).await?;
    let resp = grid_request(&client, &login.token, &report, 0, page_size.into(), None).await?;
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
    Seccao,
    Agrupamento,
    Nucleo,
    Regiao,
    Nacional,
}

impl AccessLevel {
    /// The value stored in `account_access.level`.
    pub fn as_str(self) -> &'static str {
        match self {
            AccessLevel::None => "none",
            AccessLevel::Seccao => "seccao",
            AccessLevel::Agrupamento => "agrupamento",
            AccessLevel::Nucleo => "nucleo",
            AccessLevel::Regiao => "regiao",
            AccessLevel::Nacional => "nacional",
        }
    }
}

/// A secção inside an agrupamento: `tipo` 1 Alcateia (Lobitos), 2 Expedição (Exploradores),
/// 3 Comunidade (Pioneiros), 4 Clã (Caminheiros); `numero` tells apart two of the same tipo
/// (ultra rare), 1 for the usual single one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seccao {
    pub tipo: i16,
    pub numero: i16,
}

/// One agrupamento as the grid shows it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgrupamentoSeen {
    pub regiao: Option<i16>,
    /// None: a região without núcleos.
    pub nucleo: Option<i16>,
    /// Grid `agrupdesc`.
    pub name: Option<String>,
    pub seccoes: BTreeSet<Seccao>,
    /// Some of its rows have no secção (e.g. agrupamento staff). Rules out secção-only access.
    pub rows_without_seccao: bool,
}

/// One access grant: a whole agrupamento, or a single secção in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub agrupamento: i16,
    pub seccao: Option<Seccao>,
}

/// What the members grid shows one person, as organisation codes.
/// No `Debug`: `other_identifier` is the person's own NIF or NIN.
#[derive(Default)]
pub struct Visibility {
    pub agrupamentos: BTreeMap<i16, AgrupamentoSeen>,
    /// Every núcleo in the grid (including rows outside any agrupamento) -> its região.
    pub nucleos: BTreeMap<i16, Option<i16>>,
    /// Every região in the grid (including rows outside any agrupamento).
    pub regioes: BTreeSet<i16>,
    /// If the person appears in their own grid: the identifier they did NOT log in with
    /// (the NIN after a NIF login, and vice versa), normalized. Lets both reach one account.
    pub other_identifier: Option<String>,
}

/// The identifier the person logged in with, so their own grid row can be recognised.
pub struct OwnId {
    /// true = they typed a NIF (Loja Escutista), false = a NIN (SIIE).
    pub loja_escutista: bool,
    /// Already passed through `normalize_identifier`.
    pub identifier: String,
}

/// One canonical form for NIF/NIN comparisons and hashing: no whitespace, uppercase.
pub fn normalize_identifier(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase()
}

impl Visibility {
    /// Adds one grid row. Pure (no I/O), so the rules are testable without SIIE.
    pub fn add_row(&mut self, row: &Map<String, Value>, own: &OwnId) -> Result<(), SiieError> {
        // A missing key means SIIE renamed the column. Field names (not values) help fix it.
        if SCOPE_FIELDS.iter().any(|f| !row.contains_key(*f)) {
            let fields = row.keys().map(String::as_str).collect::<Vec<_>>();
            return Err(SiieError::ScopeFieldMissing(fields.join(", ")));
        }
        let regiao = field_code(row, REGIAO_FIELD)?;
        let nucleo = field_code(row, NUCLEO_FIELD)?;
        let agrupamento = field_code(row, AGRUPAMENTO_FIELD)?;

        // None = placeholder/empty (e.g. regional staff outside any agrupamento): adds nothing.
        if let Some(r) = regiao {
            self.regioes.insert(r);
        }
        if let Some(n) = nucleo {
            // Keep the first known região for this núcleo.
            let parent = self.nucleos.entry(n).or_insert(None);
            *parent = parent.or(regiao);
        }
        if let Some(a) = agrupamento {
            let seen = self.agrupamentos.entry(a).or_default();
            // First row with a value wins; the rows of one agrupamento all agree in practice.
            seen.regiao = seen.regiao.or(regiao);
            seen.nucleo = seen.nucleo.or(nucleo);
            if seen.name.is_none() {
                seen.name = row.get(AGRUPAMENTO_NAME_FIELD).and_then(field_text);
            }
            match seccao_from_row(row)? {
                Some(s) => {
                    seen.seccoes.insert(s);
                }
                None => seen.rows_without_seccao = true,
            }
        }

        // The person's own row: same identifier as typed at login. First match wins.
        if self.other_identifier.is_none() {
            let (match_field, other_field) = if own.loja_escutista {
                (NIF_FIELD, NIN_FIELD)
            } else {
                (NIN_FIELD, NIF_FIELD)
            };
            self.other_identifier = own_other_identifier(row, match_field, other_field, own);
        }
        Ok(())
    }

    /// The level is read from how wide the visible set is, widest spread first:
    /// - nacional: more than 1 região
    /// - regiao: more than 1 núcleo
    /// - exactly 1 agrupamento: seccao if its rows all sit in one single secção, else
    ///   agrupamento. Checked before núcleo, because one agrupamento also means one núcleo.
    /// - nucleo: 1 núcleo, or several agrupamentos
    /// - none: nothing of the above (e.g. only a região with no núcleo or agrupamento under it)
    ///
    /// Placeholder codes (0000...) never get here, see `field_code`.
    /// Known blind spots (SIIE doesn't expose the role itself): núcleo access to a núcleo with a
    /// single agrupamento reads as "agrupamento"; an agrupamento whose only rows are one secção
    /// reads as "seccao". The grants are exact either way: they are what access is cut on.
    pub fn level(&self) -> AccessLevel {
        if self.regioes.len() > 1 {
            AccessLevel::Nacional
        } else if self.nucleos.len() > 1 {
            AccessLevel::Regiao
        } else if self.agrupamentos.len() == 1 {
            match self.single_seccao() {
                Some(_) => AccessLevel::Seccao,
                None => AccessLevel::Agrupamento,
            }
        } else if self.nucleos.len() == 1 || self.agrupamentos.len() > 1 {
            AccessLevel::Nucleo
        } else {
            AccessLevel::None
        }
    }

    /// What the person may see: their one secção at level seccao, otherwise whole agrupamentos.
    pub fn grants(&self) -> Vec<Grant> {
        if let Some((agrupamento, seccao)) = self.single_seccao() {
            return vec![Grant {
                agrupamento,
                seccao: Some(seccao),
            }];
        }
        self.agrupamentos
            .keys()
            .map(|&agrupamento| Grant {
                agrupamento,
                seccao: None,
            })
            .collect()
    }

    /// Exactly one agrupamento whose rows all belong to exactly one secção.
    fn single_seccao(&self) -> Option<(i16, Seccao)> {
        let mut agrupamentos = self.agrupamentos.iter();
        let (&code, seen) = agrupamentos.next()?;
        if agrupamentos.next().is_some() || seen.rows_without_seccao || seen.seccoes.len() != 1 {
            return None;
        }
        Some((code, *seen.seccoes.first()?))
    }
}

/// Reads the whole grid with this session and collects where its members belong.
/// Rows are dropped as soon as those fields are read: nothing about other members (names, NIFs,
/// contacts...) leaves this function. The only identifier that does is the person's own.
pub async fn visibility(
    client: &Client,
    token: &SecretString,
    own: &OwnId,
) -> Result<Visibility, SiieError> {
    let report = open_report(client, MEMBERS_REPORT).await?;
    let mut vis = Visibility::default();
    for_each_row(client, token, &report, None, |row| vis.add_row(row, own)).await?;
    Ok(vis)
}

/// One list screen opened in this session: its page (sent as Referer) and its report id.
pub struct Report {
    page: String,
    id: String,
}

/// Opens a list screen like a browser navigation (cookies only) and reads this session's
/// encrypted report id from it. `xml_path` is the screen's report under GRID_PAGE_PREFIX, e.g.
/// `MEMBERS_REPORT`, or a full page path starting with `/`, e.g. `AGRUPAMENTOS_PAGE`.
pub async fn open_report(client: &Client, xml_path: &str) -> Result<Report, SiieError> {
    let page = if xml_path.starts_with('/') {
        xml_path.to_owned()
    } else {
        format!("{GRID_PAGE_PREFIX}{xml_path}")
    };
    let html = page_html(client, &page).await?;
    match find_report_xml(&html) {
        Some(id) => Ok(Report { page, id }),
        None => Err(SiieError::ReportIdNotFound {
            mentions: xml_mentions(&html),
            page,
        }),
    }
}

/// Reads every row of a report, page by page, and hands each one to `f`. Returns the row count.
/// `filter` is a Kendo filter object, passed through as-is. Errors name the report.
pub async fn for_each_row(
    client: &Client,
    token: &SecretString,
    report: &Report,
    filter: Option<&Value>,
    f: impl FnMut(&Map<String, Value>) -> Result<(), SiieError>,
) -> Result<u64, SiieError> {
    read_rows(client, token, report, filter, f)
        .await
        .map_err(|e| SiieError::InReport {
            report: report.page.trim_start_matches(GRID_PAGE_PREFIX).to_owned(),
            source: Box::new(e),
        })
}

async fn read_rows(
    client: &Client,
    token: &SecretString,
    report: &Report,
    filter: Option<&Value>,
    mut f: impl FnMut(&Map<String, Value>) -> Result<(), SiieError>,
) -> Result<u64, SiieError> {
    let mut skip = 0u64;
    loop {
        let (rows, total) = grid_page(client, token, report, skip, GRID_PAGE_SIZE, filter).await?;
        for row in &rows {
            let row = row
                .as_object()
                .ok_or_else(|| SiieError::UnexpectedShape(format!("linha {}", shape(row))))?;
            f(row)?;
        }

        // Paging: stop on a short page or once `total` rows are read. Never silently truncate.
        let n = rows.len() as u64;
        skip += n;
        if n < GRID_PAGE_SIZE || total.is_some_and(|t| skip >= t) {
            return Ok(skip);
        }
        if skip >= MAX_GRID_ROWS {
            return Err(SiieError::TooManyRows);
        }
    }
}

/// One page of a report as rows, plus the grid's total row count when it says so.
pub async fn grid_page(
    client: &Client,
    token: &SecretString,
    report: &Report,
    skip: u64,
    take: u64,
    filter: Option<&Value>,
) -> Result<(Vec<Value>, Option<u64>), SiieError> {
    let resp = grid_request(client, token, report, skip, take, filter).await?;
    if is_login_page(&resp) || !content_type(&resp).contains("json") {
        return Err(SiieError::GridNotJson);
    }
    split_kendo(resp.json().await?)
}

/// A SIIE page's HTML, fetched like a browser navigation. `path` starts with `/`.
pub async fn page_html(client: &Client, path: &str) -> Result<String, SiieError> {
    let resp = client
        .get(format!("{BASE}{path}"))
        .send()
        .await?
        .error_for_status()?;
    if is_login_page(&resp) {
        return Err(SiieError::GridPageIsLogin);
    }
    Ok(resp.text().await?)
}

/// The secção a row belongs to, or None (no secção, e.g. agrupamento staff).
/// TODO(discovery): how `seccao` / `siglaseccao` encode tipo 1-4 and the numero is not known yet
/// (POC page, "Secções nesta página"). Until then no secções are collected, so the level is never
/// "seccao" and every grant is a whole agrupamento.
fn seccao_from_row(_row: &Map<String, Value>) -> Result<Option<Seccao>, SiieError> {
    Ok(None)
}

/// If `row` is the logged-in person's (its `match_field` equals what they typed), their
/// identifier in `other_field`, normalized.
fn own_other_identifier(
    row: &Map<String, Value>,
    match_field: &str,
    other_field: &str,
    own: &OwnId,
) -> Option<String> {
    let text = |f: &str| {
        row.get(f)
            .and_then(field_text)
            .map(|v| normalize_identifier(&v))
    };
    if text(match_field)? == own.identifier {
        text(other_field)
    } else {
        None
    }
}

/// An organisation code column as a number: "0053", 53 and " 53 " are all 53. All-zero
/// placeholders and empty values are None. Anything else non-numeric is an error rather than
/// silently dropped (the message holds the code, never member data).
pub(crate) fn field_code(
    row: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<i16>, SiieError> {
    let Some(text) = row.get(field).and_then(field_text) else {
        return Ok(None);
    };
    text.parse()
        .map(Some)
        .map_err(|_| SiieError::BadCode { field, value: text })
}

/// Grid values come as strings or numbers depending on the column; normalise to trimmed text.
/// SIIE fills "no região/núcleo/agrupamento" with zeros (região "00", núcleo or agrupamento
/// "0000"), so an all-zero value is treated like null: it is not a place anyone has access to.
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
    report: &Report,
    skip: u64,
    take: u64,
    filter: Option<&Value>,
) -> Result<reqwest::Response, SiieError> {
    // Kendo DataSource read params: the grid's parameterMap does JSON.stringify(data) and jQuery
    // appends it as a bare query component (no `key=`). Built by hand since `.query()` adds a key.
    let page = skip / take + 1;
    let filter = filter.map_or(String::new(), |f| format!(r#","filter":{f}"#));
    let params = format!(
        r#"{{"take":{take},"skip":{skip},"page":{page},"pageSize":{take},"sort":[]{filter}}}"#
    )
    .replace('"', "%22")
    .replace(' ', "%20");
    let url = format!("{BASE}/web/api/generic?xml={}&{params}", report.id);
    Ok(client
        .get(&url)
        .header("X-Requested-With", "XMLHttpRequest")
        .header(header::REFERER, format!("{BASE}{}", report.page))
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
                // An empty grid (e.g. an agrupamento with no rows in a small report): SIIE sends
                // a null Data, or no Data with a zero count. Anything else without Data, such as
                // an error object, stays an error so it can't pass for "no rows".
                Some(Value::Null) => Ok((Vec::new(), total)),
                None if total == Some(0) => Ok((Vec::new(), total)),
                data => {
                    if let Some(d) = data {
                        o.insert("Data".into(), d);
                    }
                    Err(SiieError::UnexpectedShape(shape(&Value::Object(o))))
                }
            }
        }
        other => Err(SiieError::UnexpectedShape(shape(&other))),
    }
}

/// A response's form for error messages: top-level keys and JSON types, never values, plus the
/// start of SIIE's own error text when there is one (a server message, not member data).
/// {"Data": null, "Message": "x"} -> "{Data: null, Message: string} (SIIE: x)".
fn shape(v: &Value) -> String {
    let kind = |v: &Value| match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    match v {
        Value::Object(o) => {
            let keys: Vec<String> = o
                .iter()
                .take(10)
                .map(|(k, v)| format!("{k}: {}", kind(v)))
                .collect();
            let message = ["Message", "message", "ExceptionMessage"]
                .iter()
                .find_map(|k| o.get(*k).and_then(Value::as_str))
                .map(|m| format!(" (SIIE: {})", m.chars().take(200).collect::<String>()))
                .unwrap_or_default();
            format!("{{{}}}{message}", keys.join(", "))
        }
        other => kind(other).to_owned(),
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
        // Empty grids.
        assert_eq!(
            split_kendo(json!({"Data": null, "Total": 0}))
                .unwrap()
                .0
                .len(),
            0
        );
        assert_eq!(split_kendo(json!({"Total": 0})).unwrap().0.len(), 0);
        // Not a grid: an error naming keys and types only, never values.
        let e = split_kendo(json!({"Message": "Erro X", "Data": 3, "x": "segredo"})).unwrap_err();
        assert_eq!(
            e.to_string(),
            "A resposta da grelha tem um formato inesperado: {Data: number, Message: string, x: string} (SIIE: Erro X)."
        );
        assert!(split_kendo(json!({"nope": 1})).is_err());
    }

    fn own() -> OwnId {
        OwnId {
            loja_escutista: true,
            identifier: "123456789".to_owned(),
        }
    }

    /// Builds a Visibility from grid-like rows: (agrupamento, nucleo, regiao).
    fn from_rows(rows: &[(&str, &str, &str)]) -> Visibility {
        let mut v = Visibility::default();
        for (a, n, r) in rows {
            let row = json!({"agrupamento": a, "nucleo": n, "regiao": r, "agrupdesc": "Agr"});
            v.add_row(row.as_object().unwrap(), &own()).unwrap();
        }
        v
    }

    #[test]
    fn finds_own_row_and_reads_the_other_identifier() {
        let own = own();
        let row = |v: Value| v.as_object().unwrap().clone();
        let mine = row(json!({"nif": " 123 456 789", "nin": 555}));
        assert_eq!(
            own_other_identifier(&mine, "nif", "nin", &own),
            Some("555".to_owned())
        );
        let other = row(json!({"nif": "987654321", "nin": 556}));
        assert_eq!(own_other_identifier(&other, "nif", "nin", &own), None);
        // Own row but no usable other identifier (null or a zero placeholder).
        let no_nin = row(json!({"nif": "123456789", "nin": "0000"}));
        assert_eq!(own_other_identifier(&no_nin, "nif", "nin", &own), None);
        let missing = row(json!({"nin": 555}));
        assert_eq!(own_other_identifier(&missing, "nif", "nin", &own), None);
    }

    #[test]
    fn normalize_identifier_strips_spaces_and_case() {
        assert_eq!(normalize_identifier(" 123 456 789 "), "123456789");
        assert_eq!(normalize_identifier("ab12"), "AB12");
    }

    #[test]
    fn add_row_collects_numeric_structure() {
        let v = from_rows(&[
            ("0053", "0505", "05"),
            ("0054", "0505", "05"),
            ("0000", "0505", "05"), // núcleo staff, no agrupamento
            ("0000", "0000", "00"), // nobody's
        ]);
        assert_eq!(v.regioes, BTreeSet::from([5]));
        assert_eq!(v.nucleos, BTreeMap::from([(505, Some(5))]));
        assert_eq!(
            v.agrupamentos.keys().copied().collect::<Vec<_>>(),
            vec![53, 54]
        );
        let a = &v.agrupamentos[&53];
        assert_eq!(
            (a.regiao, a.nucleo, a.name.as_deref()),
            (Some(5), Some(505), Some("Agr"))
        );
        // No secção parser yet: every row counts as "without secção".
        assert!(a.rows_without_seccao && a.seccoes.is_empty());
    }

    #[test]
    fn add_row_rejects_non_numeric_codes_and_missing_columns() {
        let mut v = Visibility::default();
        let bad = json!({"agrupamento": "A53", "nucleo": "0505", "regiao": "05"});
        assert!(matches!(
            v.add_row(bad.as_object().unwrap(), &own()),
            Err(SiieError::BadCode {
                field: "agrupamento",
                ..
            })
        ));
        let missing = json!({"agrupamento": "0053"});
        assert!(matches!(
            v.add_row(missing.as_object().unwrap(), &own()),
            Err(SiieError::ScopeFieldMissing(_))
        ));
    }

    #[test]
    fn level_rules() {
        use AccessLevel::*;
        assert_eq!(from_rows(&[]).level(), None);
        // Only a região, nothing under it.
        assert_eq!(from_rows(&[("0", "0", "01")]).level(), None);
        // One agrupamento also means one núcleo and one região: still agrupamento.
        assert_eq!(from_rows(&[("53", "505", "5")]).level(), Agrupamento);
        // One núcleo, with several agrupamentos or none at all.
        assert_eq!(
            from_rows(&[("53", "505", "5"), ("54", "505", "5")]).level(),
            Nucleo
        );
        assert_eq!(from_rows(&[("0", "505", "5")]).level(), Nucleo);
        // Several agrupamentos even without núcleo codes.
        assert_eq!(
            from_rows(&[("53", "0", "0"), ("54", "0", "0")]).level(),
            Nucleo
        );
        assert_eq!(
            from_rows(&[("53", "505", "5"), ("60", "506", "5")]).level(),
            Regiao
        );
        assert_eq!(
            from_rows(&[("53", "505", "5"), ("90", "901", "9")]).level(),
            Nacional
        );
    }

    #[test]
    fn seccao_level_and_grants() {
        let alcateia = Seccao { tipo: 1, numero: 1 };
        let mut v = from_rows(&[("53", "505", "5")]);
        let a = v.agrupamentos.get_mut(&53).unwrap();
        a.rows_without_seccao = false;
        a.seccoes.insert(alcateia);
        // One agrupamento, all rows in one secção: secção-only access.
        assert_eq!(v.level(), AccessLevel::Seccao);
        assert_eq!(
            v.grants(),
            vec![Grant {
                agrupamento: 53,
                seccao: Some(alcateia)
            }]
        );
        // Any row outside a secção (e.g. staff) makes it the whole agrupamento.
        v.agrupamentos.get_mut(&53).unwrap().rows_without_seccao = true;
        assert_eq!(v.level(), AccessLevel::Agrupamento);
        assert_eq!(
            v.grants(),
            vec![Grant {
                agrupamento: 53,
                seccao: None
            }]
        );
        // Several agrupamentos: whole agrupamentos.
        let v = from_rows(&[("53", "505", "5"), ("54", "505", "5")]);
        assert_eq!(v.grants().len(), 2);
        assert!(v.grants().iter().all(|g| g.seccao.is_none()));
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
