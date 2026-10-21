//! Members: a pseudonymised copy of SIIE's "dados completos" grid, imported per agrupamento.
//!
//! - At login, `queue` adds one `member_imports` row per agrupamento the account holds whole and
//!   that wasn't imported within MEMBERS_SYNC_DAYS. The Painel shows those rows to everyone whose
//!   grants cover the agrupamento.
//! - `sync` (after the login response) re-reads the access if due, then `run` imports the queued
//!   agrupamentos one at a time with the same SIIE session, each in its own transaction.
//! - `parse` keeps only the allowlisted columns (see the field table in the README). Names, NIFs,
//!   contacts and street addresses are dropped right there, in memory.
//! - Each import also reads, for the same agrupamento, the especialidades reports
//!   (`member_especialidades`) and every report in member_reports.rs (progress, past
//!   agrupamentos, exits, health, cargos, cursos, atividades, distinções, habilitações).
//! - Every change of a member field is a `member_changes` row; entries and exits are changes of
//!   the `present` field; the other tables have their own field prefixes.

use std::collections::{BTreeMap, HashMap};

use reqwest::Client;
use secrecy::SecretString;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::{
    AppState,
    access::{self, STALE_RUN_MINUTES},
    auth::Method,
    member_reports,
    siie::{self, OwnId, SiieError, field_code},
};

// "dados completos" columns that are kept (keys checked 2026-10-21 with the POC profile).
// Every one of them must be in the grid, so a SIIE rename fails the import instead of silently
// storing nulls. Everything else in the row (nome, nif, cc, morada, contacts, parents, notes,
// registo criminal, internal ids) is never read.
pub(crate) const NIN: &str = "nin";
pub(crate) const AGRUPAMENTO: &str = "agrupamento";
const MEMBER_KEYS: &[&str] = &[
    NIN,
    "totem",
    "siglasexo",
    "datanascimento",
    AGRUPAMENTO,
    "nucleo",
    "regiao",
    "elemnucleo",
    "elemregiao",
    "elemNacional",
    "situacao",
    "categoria",
    "ordem",
    "seccao",
    "ccartao",
    "dataadmissao",
    "datapromessa",
    "profissao",
    "formador",
    "diretorformacao",
    "imagensnao",
    // member_addresses
    "cp1",
    "cp2",
    "localidade",
    "codigopostal",
    "naturalidade",
    // member_health
    "deficiencia",
    "sigladeficiencia",
];
// Especialidades: one report per secção (seccao_tipo 1 Lobitos ... 4 Caminheiros), with one row
// per especialidade earned there: nin, especialidade, datarealizacao (keys checked 2026-10-21).
const ESPECIALIDADES_REPORTS: [(&str, i16); 4] = [
    ("elementos/especialidades/lista-lobitos", 1),
    ("elementos/especialidades/lista-exploradores", 2),
    ("elementos/especialidades/lista-pioneiros", 3),
    ("elementos/especialidades/lista-caminheiros", 4),
];
const ESPECIALIDADE_KEYS: [&str; 3] = [NIN, "especialidade", "datarealizacao"];
// Prefix of member_especialidades changes: "especialidade.<seccao_tipo>.<name>", old/new = date.
const ESPECIALIDADE_FIELD_PREFIX: &str = "especialidade.";
// Message stored on imports whose run died with its function (see STALE_RUN_MINUTES).
const STALE_ERROR: &str =
    "A importação foi interrompida (tempo limite). Será repetida no próximo início de sessão.";
// How long a finished import stays on the Painel.
const RECENT_IMPORT_HOURS: i32 = 24;

/// One `members` row. Dates are `YYYY-MM-DD` text, cast to `date` by Postgres.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Member {
    pub nin: String,
    pub totem: String,
    pub totem_from_siie: bool,
    pub sexo: Option<String>,
    pub birth_month: Option<String>,
    pub agrupamento: Option<i16>,
    pub nucleo: Option<i16>,
    pub regiao: Option<i16>,
    pub el_nucleo: Option<i16>,
    pub el_regiao: Option<i16>,
    pub el_nacional: Option<bool>,
    pub situacao: Option<String>,
    pub categoria: Option<String>,
    pub categoria_ordem: Option<i16>,
    pub seccao: Option<String>,
    pub cartao: Option<bool>,
    pub admitted_on: Option<String>,
    pub promised_on: Option<String>,
    pub profissao: Option<String>,
    pub formador: Option<bool>,
    pub diretor_formacao: Option<bool>,
    pub refuses_image: Option<bool>,
}

/// One `member_addresses` row.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Address {
    pub nin: String,
    pub cp4: Option<String>,
    pub cp3: Option<String>,
    pub localidade: Option<String>,
    pub cp_localidade: Option<String>,
    pub naturalidade: Option<String>,
}

/// One `member_health` row.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Health {
    pub nin: String,
    pub deficiencia: Option<String>,
    pub sigla_deficiencia: Option<String>,
}

/// What one grid row becomes. The rest of the row is dropped by `parse`.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub member: Member,
    pub address: Address,
    pub health: Health,
}

/// Keeps the allowlisted columns of one "dados completos" row. Pure, so testable without SIIE.
pub fn parse(row: &Map<String, Value>) -> Result<Parsed, SiieError> {
    // Key names only in the error, never values.
    let missing: Vec<&str> = MEMBER_KEYS
        .iter()
        .copied()
        .filter(|k| !row.contains_key(*k))
        .collect();
    if !missing.is_empty() {
        return Err(SiieError::MemberFieldMissing(missing.join(", ")));
    }
    let nin = text(row, NIN)
        .map(|n| siie::normalize_identifier(&n))
        .ok_or(SiieError::MissingNin)?;
    let (totem, totem_from_siie) = totem(&nin, text(row, "totem"));
    // Birth date to the month only: "2011-03-12" -> "2011-03-01".
    let birth_month = date(row, "datanascimento")?.map(|d| format!("{}-01", &d[..7]));

    let member = Member {
        totem,
        totem_from_siie,
        sexo: text(row, "siglasexo"),
        birth_month,
        // Codes go through field_code: "0455" -> 455, and the all-zero "none" codes -> null.
        agrupamento: field_code(row, AGRUPAMENTO)?,
        nucleo: field_code(row, "nucleo")?,
        regiao: field_code(row, "regiao")?,
        el_nucleo: field_code(row, "elemnucleo")?,
        el_regiao: field_code(row, "elemregiao")?,
        el_nacional: flag(row, "elemNacional"),
        situacao: text(row, "situacao"),
        categoria: text(row, "categoria"),
        categoria_ordem: field_code(row, "ordem")?,
        seccao: text(row, "seccao"),
        cartao: flag(row, "ccartao"),
        admitted_on: date(row, "dataadmissao")?,
        promised_on: date(row, "datapromessa")?,
        profissao: text(row, "profissao"),
        formador: flag(row, "formador"),
        diretor_formacao: flag(row, "diretorformacao"),
        refuses_image: flag(row, "imagensnao"),
        nin: nin.clone(),
    };
    let address = Address {
        nin: nin.clone(),
        cp4: text(row, "cp1"),
        cp3: text(row, "cp2"),
        localidade: text(row, "localidade"),
        cp_localidade: text(row, "codigopostal"),
        naturalidade: text(row, "naturalidade"),
    };
    let health = Health {
        nin,
        deficiencia: text(row, "deficiencia"),
        sigla_deficiencia: text(row, "sigladeficiencia"),
    };
    Ok(Parsed {
        member,
        address,
        health,
    })
}

/// Trimmed text; empty is null. Numbers are kept as their digits.
pub(crate) fn text(row: &Map<String, Value>, key: &str) -> Option<String> {
    let t = match row.get(key)? {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!t.is_empty()).then_some(t)
}

/// SIIE booleans come as JSON booleans, or as "1"/"0", "Sim"/"Não", "true"/"false".
pub(crate) fn flag(row: &Map<String, Value>, key: &str) -> Option<bool> {
    match row.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "1" | "sim" | "true" => Some(true),
            "0" | "não" | "nao" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// ISO dates ("2011-03-12T00:00:00") as "2011-03-12". Anything else is an error, not dropped.
pub(crate) fn date(row: &Map<String, Value>, key: &str) -> Result<Option<String>, SiieError> {
    let Some(t) = text(row, key) else {
        return Ok(None);
    };
    let d: String = t.chars().take(10).collect();
    let b = d.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if ok {
        Ok(Some(d))
    } else {
        Err(SiieError::BadDate(key.to_owned()))
    }
}

/// The row's NIN, normalized.
pub(crate) fn row_nin(row: &Map<String, Value>) -> Result<String, SiieError> {
    text(row, NIN)
        .map(|n| siie::normalize_identifier(&n))
        .ok_or(SiieError::MissingNin)
}

/// One member_especialidades key: (nin, seccao_tipo, especialidade).
type EspecialidadeKey = (String, i16, String);
/// Especialidades of one import: key -> date (null if SIIE has none).
type Especialidades = BTreeMap<EspecialidadeKey, Option<String>>;

/// Adds one especialidades row to `into`. If SIIE lists the same one twice, the earliest date
/// wins (one row per member, secção and especialidade).
fn parse_especialidade(
    row: &Map<String, Value>,
    seccao_tipo: i16,
    into: &mut Especialidades,
) -> Result<(), SiieError> {
    let missing: Vec<&str> = ESPECIALIDADE_KEYS
        .iter()
        .copied()
        .filter(|k| !row.contains_key(*k))
        .collect();
    if !missing.is_empty() {
        return Err(SiieError::MemberFieldMissing(missing.join(", ")));
    }
    let Some(name) = text(row, "especialidade") else {
        return Ok(()); // a row without a name says nothing
    };
    let on = date(row, "datarealizacao")?;
    let slot = into
        .entry((row_nin(row)?, seccao_tipo, name))
        .or_insert(None);
    if slot.is_none() || on.is_some() && on < *slot {
        *slot = on;
    }
    Ok(())
}

#[rustfmt::skip]
const ANIMAIS: [&str; 64] = [
    "Lobo", "Mocho", "Águia", "Falcão", "Raposa", "Lince", "Urso", "Veado",
    "Javali", "Texugo", "Lontra", "Castor", "Esquilo", "Lebre", "Coelho", "Ouriço",
    "Corvo", "Gaio", "Melro", "Pisco", "Andorinha", "Gaivota", "Garça", "Cegonha",
    "Coruja", "Abutre", "Milhafre", "Peneireiro", "Pica-Pau", "Guarda-Rios", "Cuco", "Poupa",
    "Grou", "Flamingo", "Golfinho", "Baleia", "Tubarão", "Polvo", "Tartaruga", "Salamandra",
    "Lagarto", "Sapo", "Truta", "Salmão", "Leão", "Tigre", "Pantera", "Leopardo",
    "Jaguar", "Elefante", "Rinoceronte", "Búfalo", "Bisonte", "Alce", "Camurça", "Cabra-Montês",
    "Foca", "Morsa", "Pinguim", "Albatroz", "Condor", "Panda", "Gamo", "Bufo",
];

#[rustfmt::skip]
const ADJETIVOS: [&str; 64] = [
    "Veloz", "Audaz", "Feroz", "Tenaz", "Sagaz", "Perspicaz", "Vivaz", "Leal",
    "Fiel", "Ágil", "Gentil", "Subtil", "Hábil", "Feliz", "Forte", "Alegre",
    "Valente", "Paciente", "Prudente", "Constante", "Livre", "Nobre", "Humilde", "Selvagem",
    "Indomável", "Incansável", "Imparável", "Afável", "Vigilante", "Errante", "Brilhante", "Sublime",
    "Solene", "Silente", "Insaciável", "Implacável", "Mente-Aberta", "Pé-Ligeiro", "Olhar-Atento", "Asa-Veloz",
    "Garra-Certeira", "Coração-Valente", "Passo-Firme", "Língua-Afiada", "Ouvido-Fino", "Faro-Apurado", "Mão-Amiga", "Riso-Fácil",
    "Alma-Livre", "Sangue-Frio", "Voz-Mansa", "Olho-Vivo", "Peito-Aberto", "Sono-Leve", "Cabeça-nas-Nuvens", "Braço-Forte",
    "Rasto-Invisível", "Pata-Leve", "Que Nunca Dorme", "Que Sonha Acordado", "Que Canta ao Luar", "Que Ri da Chuva", "Que Segue o Vento", "Que Nunca Desiste",
];

/// The member's totem, and whether it is SIIE's own.
/// SIIE's totem wins when there is one. Otherwise, with h = SHA-256(NIN):
/// ANIMAIS[h[0] % 64] + " " + ADJETIVOS[h[1] % 64], so the same NIN always gets the same totem.
/// 4096 combinations for ~11k members: duplicates happen and are fine, the NIN is the key.
pub fn totem(nin: &str, siie_totem: Option<String>) -> (String, bool) {
    if let Some(t) = siie_totem {
        return (t, true);
    }
    let h = Sha256::digest(nin.as_bytes());
    let animal = ANIMAIS[usize::from(h[0]) % ANIMAIS.len()];
    let adjetivo = ADJETIVOS[usize::from(h[1]) % ADJETIVOS.len()];
    (format!("{animal} {adjetivo}"), false)
}

/// One `member_changes` row before it gets its import id.
#[derive(Serialize, Debug, PartialEq)]
pub(crate) struct Change {
    pub nin: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

/// What changed between the stored row (`old`, as `to_jsonb(members)`) and the new one.
/// No stored row: only `present` null -> true (the first values are the old_value of each
/// field's first change). A comeback adds `present` false -> true.
fn diff(old: Option<&Map<String, Value>>, new: &Member) -> Vec<Change> {
    let change = |field: &str, old_value: Option<String>, new_value: Option<String>| Change {
        nin: new.nin.clone(),
        field: field.to_owned(),
        old_value,
        new_value,
    };
    let Some(old) = old else {
        return vec![change("present", None, Some("true".into()))];
    };
    let Ok(Value::Object(new_map)) = serde_json::to_value(new) else {
        return Vec::new();
    };
    let mut changes = changed(old, &new_map, "");
    if old.get("present") == Some(&Value::Bool(false)) {
        changes.push(change("present", Some("false".into()), Some("true".into())));
    }
    changes
}

/// The fields of `new` whose value differs from the stored row's, named `{prefix}{field}`.
pub(crate) fn changed(
    old: &Map<String, Value>,
    new: &Map<String, Value>,
    prefix: &str,
) -> Vec<Change> {
    let nin = new.get(NIN).and_then(Value::as_str).unwrap_or_default();
    new.iter()
        .filter(|(k, _)| k.as_str() != NIN)
        .filter_map(|(k, v)| {
            let (a, b) = (value_text(old.get(k)), value_text(Some(v)));
            (a != b).then(|| Change {
                nin: nin.to_owned(),
                field: format!("{prefix}{k}"),
                old_value: a,
                new_value: b,
            })
        })
        .collect()
}

/// Especialidades added (null -> date), re-dated, or gone (date -> null), as changes named
/// "especialidade.<seccao_tipo>.<name>". `known(nin)`: the member was stored before this import;
/// for one seen for the first time, nothing is logged (its dates already are the history).
fn especialidade_changes(
    old: &Especialidades,
    new: &Especialidades,
    known: impl Fn(&str) -> bool,
) -> Vec<Change> {
    let change =
        |(nin, tipo, name): &EspecialidadeKey, a: Option<&String>, b: Option<&String>| Change {
            nin: nin.clone(),
            field: format!("{ESPECIALIDADE_FIELD_PREFIX}{tipo}.{name}"),
            old_value: a.cloned(),
            new_value: b.cloned(),
        };
    let added_or_redated = new.iter().filter_map(|(k, on)| match old.get(k) {
        None if known(&k.0) => Some(change(k, None, on.as_ref())),
        Some(was) if was != on => Some(change(k, was.as_ref(), on.as_ref())),
        _ => None,
    });
    let gone = old
        .iter()
        .filter(|(k, _)| !new.contains_key(*k))
        .map(|(k, was)| change(k, was.as_ref(), None));
    added_or_redated.chain(gone).collect()
}

/// A JSON value as comparable text: dates and codes compare equal however they were encoded.
pub(crate) fn value_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// A queued import, as `run` needs it.
pub struct Queued {
    pub id: i64,
    pub agrupamento: i16,
}

/// Queues an import for each agrupamento the account holds whole whose last import is older than
/// `sync_days` (or that was never imported), unless one is already queued or running.
/// Secção-only grants never queue: that reader sees part of the agrupamento, so everyone else
/// would be recorded as an exit.
pub async fn queue(
    db: &PgPool,
    account_id: Uuid,
    sync_days: i32,
) -> Result<Vec<Queued>, sqlx::Error> {
    // Imports whose function died (started or queued longer ago than any run can last) fail
    // first, so their agrupamentos can be queued again.
    sqlx::query(
        "update member_imports set status = 'failed', finished_at = now(), error = $1
         where status in ('queued', 'running')
           and coalesce(started_at, queued_at) < now() - make_interval(mins => $2)",
    )
    .bind(STALE_ERROR)
    .bind(STALE_RUN_MINUTES)
    .execute(db)
    .await?;

    let mut queued: Vec<(i64, i16)> = sqlx::query_as(
        "insert into member_imports (agrupamento, account_id)
         select g.agrupamento, $1 from account_grants g
         where g.account_id = $1 and g.seccao_tipo is null
           and not exists (
               select 1 from member_imports i
               where i.agrupamento = g.agrupamento and i.status = 'done'
                 and i.finished_at > now() - make_interval(days => $2))
         on conflict (agrupamento) where status in ('queued', 'running') do nothing
         returning id, agrupamento",
    )
    .bind(account_id)
    .bind(sync_days)
    .fetch_all(db)
    .await?;
    // Code order, so the Painel fills in a predictable order.
    queued.sort_by_key(|&(_, a)| a);
    Ok(queued
        .into_iter()
        .map(|(id, agrupamento)| Queued { id, agrupamento })
        .collect())
}

/// The login's background work, with its SIIE session: the access read first if it was due
/// (`own` is set; a first read is what creates the grants imports are queued from), then the
/// member imports. The SIIE session ends when this returns.
#[allow(clippy::too_many_arguments)]
pub async fn sync(
    state: AppState,
    account_id: Uuid,
    method: Method,
    client: Client,
    login: siie::LoginOk,
    own: Option<OwnId>,
    mut queued: Vec<Queued>,
) {
    if let Some(own) = own {
        access::read_access(&state, account_id, method, &client, &login.token, own).await;
        queued = queue(&state.db, account_id, state.members_sync_days)
            .await
            .unwrap_or_else(|e| {
                eprintln!("members: queueing for account {account_id} failed: {e}");
                Vec::new()
            });
    }
    run(&state, &client, &login.token, queued).await;
}

/// Imports the queued agrupamentos one at a time. One failing doesn't stop the others.
/// If the function runs out of time, the rest stay queued until `queue` fails them as stale.
pub async fn run(state: &AppState, client: &Client, token: &SecretString, queued: Vec<Queued>) {
    if queued.is_empty() {
        return;
    }
    // Reports without an agrupamento column are read whole once, for every import of the run.
    let opened = async {
        let reports = open_reports(client).await?;
        let run_wide = member_reports::fetch(client, token, &reports.others, None).await;
        Ok::<_, SiieError>((reports, run_wide))
    };
    let (reports, run_wide) = match opened.await {
        Ok(r) => r,
        Err(e) => {
            for q in &queued {
                fail(&state.db, q.id, &e.to_string()).await;
            }
            return;
        }
    };
    for q in &queued {
        if let Err(e) = import_one(&state.db, client, token, &reports, &run_wide, q).await {
            fail(&state.db, q.id, &e).await;
        }
    }
}

/// The reports one import reads, opened once per run (each page holds this session's report id).
struct Reports {
    members: siie::Report,
    /// (report, seccao_tipo), see ESPECIALIDADES_REPORTS.
    especialidades: Vec<(siie::Report, i16)>,
    /// Everything in member_reports.rs.
    others: member_reports::Opened,
}

async fn open_reports(client: &Client) -> Result<Reports, SiieError> {
    let members = siie::open_report(client, siie::MEMBERS_REPORT).await?;
    let mut especialidades = Vec::new();
    for (path, tipo) in ESPECIALIDADES_REPORTS {
        especialidades.push((siie::open_report(client, path).await?, tipo));
    }
    let others = member_reports::open(client).await?;
    Ok(Reports {
        members,
        especialidades,
        others,
    })
}

/// What one import fetched for its agrupamento. Especialidades may hold NINs that aren't in
/// `rows`; `store` keeps only those that are.
struct Fetched {
    rows: Vec<Parsed>,
    /// None if an especialidades report failed: they are then left untouched (they are
    /// replaced per member, so a partial set would delete the rest).
    especialidades: Option<Especialidades>,
    /// This agrupamento's rows of the member_reports.rs reports.
    others: member_reports::Fetched,
    /// The reports skipped in this import and why, shown on the Painel.
    warning: Option<String>,
}

async fn fail(db: &PgPool, id: i64, error: &str) {
    let r = sqlx::query(
        "update member_imports set status = 'failed', finished_at = now(), error = $2 where id = $1",
    )
    .bind(id)
    .bind(error)
    .execute(db)
    .await;
    if let Err(e) = r {
        eprintln!("members: recording the failure of import {id} failed: {e}");
    }
}

/// One agrupamento: fetch its rows (SIIE honours a filter on `agrupamento`), then store them.
/// The error is the message shown on the Painel: SIIE errors hold codes and column names, and
/// database errors are replaced by a generic message.
async fn import_one(
    db: &PgPool,
    client: &Client,
    token: &SecretString,
    reports: &Reports,
    run_wide: &member_reports::Fetched,
    q: &Queued,
) -> Result<(), String> {
    sqlx::query("update member_imports set status = 'running', started_at = now() where id = $1")
        .bind(q.id)
        .execute(db)
        .await
        .map_err(db_message)?;

    // The grid's codes are zero-padded strings ("0455").
    let code = format!("{:04}", q.agrupamento);
    let filter = json!({"logic": "and", "filters": [
        {"field": AGRUPAMENTO, "operator": "eq", "value": code}
    ]});
    let mut rows = Vec::new();
    siie::for_each_row(client, token, &reports.members, Some(&filter), |row| {
        rows.push(parse(row)?);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;
    // The same agrupamento in each secção's especialidades report and in the other reports.
    // Only dados completos is required: any other report that fails is skipped, its tables
    // left untouched, and named in the import's warning.
    let mut warnings = Vec::new();
    let mut especialidades = Some(Especialidades::new());
    for (report, tipo) in &reports.especialidades {
        let Some(found) = especialidades.as_mut() else {
            break;
        };
        let read = siie::for_each_row(client, token, report, Some(&filter), |row| {
            parse_especialidade(row, *tipo, found)
        });
        if let Err(e) = read.await {
            warnings.push(e.to_string());
            especialidades = None;
        }
    }

    // dados completos lists inactive members too, so an agrupamento is never empty. No rows means
    // SIIE answered wrongly, and storing that would record everyone as an exit.
    if rows.is_empty() {
        return Err(format!(
            "O SIIE não devolveu nenhum elemento do agrupamento {code}; a importação foi recusada."
        ));
    }
    // If SIIE ever ignored the filter, other agrupamentos' members would be marked as exits and
    // stored under this import. Refuse instead.
    if rows
        .iter()
        .any(|p| p.member.agrupamento != Some(q.agrupamento))
    {
        return Err(format!(
            "O SIIE devolveu elementos de outros agrupamentos ao pedir o {code}; a importação foi recusada."
        ));
    }
    let others = member_reports::fetch(client, token, &reports.others, Some(&filter)).await;
    warnings.extend(others.skipped.iter().map(|(_, e)| e.clone()));
    warnings.extend(run_wide.skipped.iter().map(|(_, e)| e.clone()));
    let fetched = Fetched {
        rows,
        especialidades,
        others,
        warning: (!warnings.is_empty()).then(|| warnings.join(" | ")),
    };
    store(db, q, fetched, run_wide).await.map_err(db_message)
}

/// The stored rows of `table` for these NINs, as JSON objects keyed by NIN.
async fn stored(
    tx: &mut sqlx::PgConnection,
    table: &'static str,
    nins: &[String],
) -> Result<HashMap<String, Map<String, Value>>, sqlx::Error> {
    let rows: Vec<(Value,)> = sqlx::query_as(&format!(
        "select to_jsonb(t) from {table} t where nin = any($1)"
    ))
    .bind(nins)
    .fetch_all(tx)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(v,)| match v {
            Value::Object(m) => Some((m.get(NIN)?.as_str()?.to_owned(), m)),
            _ => None,
        })
        .collect())
}

fn db_message(e: sqlx::Error) -> String {
    eprintln!("members: database error: {e}");
    "Erro na base de dados ao guardar os elementos.".to_owned()
}

/// Writes one agrupamento's rows in a single transaction: changes, upserts, exits, counts.
/// `run_wide` holds the reports without an agrupamento column, read once per run.
async fn store(
    db: &PgPool,
    q: &Queued,
    fetched: Fetched,
    run_wide: &member_reports::Fetched,
) -> Result<(), sqlx::Error> {
    let Fetched {
        mut rows,
        especialidades,
        others,
        warning,
    } = fetched;
    // Sorted by NIN, so two imports touching the same members lock rows in the same order.
    rows.sort_by(|a, b| a.member.nin.cmp(&b.member.nin));
    let nins: Vec<String> = rows.iter().map(|p| p.member.nin.clone()).collect();
    let mut tx = db.begin().await?;

    // 1. Diff against the stored rows. Anything seen for the first time isn't a change: its
    //    values (dates mostly) are already the history. What appears, moves or goes later is.
    let old = stored(&mut tx, "members", &nins).await?;
    let mut changes: Vec<Change> = rows
        .iter()
        .flat_map(|p| diff(old.get(&p.member.nin), &p.member))
        .collect();
    // Members of other agrupamentos (records may name them) were stored before, so known too.
    let first_seen: std::collections::HashSet<&str> = nins
        .iter()
        .filter(|n| !old.contains_key(*n))
        .map(String::as_str)
        .collect();
    let known = |nin: &str| !first_seen.contains(nin);
    // 2. Upserts. jsonb_to_recordset turns the rows into a table with these column types.
    let members: Vec<&Member> = rows.iter().map(|p| &p.member).collect();
    sqlx::query(
        "insert into members (nin, totem, totem_from_siie, sexo, birth_month, agrupamento, nucleo,
             regiao, el_nucleo, el_regiao, el_nacional, situacao, categoria, categoria_ordem,
             seccao, cartao, admitted_on, promised_on, profissao, formador, diretor_formacao,
             refuses_image)
         select * from jsonb_to_recordset($1) as r(nin text, totem text, totem_from_siie boolean,
             sexo text, birth_month date, agrupamento smallint, nucleo smallint, regiao smallint,
             el_nucleo smallint, el_regiao smallint, el_nacional boolean, situacao text,
             categoria text, categoria_ordem smallint, seccao text, cartao boolean,
             admitted_on date, promised_on date, profissao text, formador boolean,
             diretor_formacao boolean, refuses_image boolean)
         on conflict (nin) do update set
             totem = excluded.totem, totem_from_siie = excluded.totem_from_siie,
             sexo = excluded.sexo, birth_month = excluded.birth_month,
             agrupamento = excluded.agrupamento, nucleo = excluded.nucleo,
             regiao = excluded.regiao, el_nucleo = excluded.el_nucleo,
             el_regiao = excluded.el_regiao, el_nacional = excluded.el_nacional,
             situacao = excluded.situacao, categoria = excluded.categoria,
             categoria_ordem = excluded.categoria_ordem, seccao = excluded.seccao,
             cartao = excluded.cartao, admitted_on = excluded.admitted_on,
             promised_on = excluded.promised_on, profissao = excluded.profissao,
             formador = excluded.formador, diretor_formacao = excluded.diretor_formacao,
             refuses_image = excluded.refuses_image, present = true, last_seen_at = now()",
    )
    .bind(Json(&members))
    .execute(&mut *tx)
    .await?;

    let addresses: Vec<&Address> = rows.iter().map(|p| &p.address).collect();
    sqlx::query(
        "insert into member_addresses (nin, cp4, cp3, localidade, cp_localidade, naturalidade)
         select * from jsonb_to_recordset($1) as r(nin text, cp4 text, cp3 text,
             localidade text, cp_localidade text, naturalidade text)
         on conflict (nin) do update set
             cp4 = excluded.cp4, cp3 = excluded.cp3, localidade = excluded.localidade,
             cp_localidade = excluded.cp_localidade, naturalidade = excluded.naturalidade,
             updated_at = now()",
    )
    .bind(Json(&addresses))
    .execute(&mut *tx)
    .await?;

    let health: Vec<&Health> = rows.iter().map(|p| &p.health).collect();
    sqlx::query(
        "insert into member_health (nin, deficiencia, sigla_deficiencia)
         select * from jsonb_to_recordset($1) as r(nin text, deficiencia text,
             sigla_deficiencia text)
         on conflict (nin) do update set
             deficiencia = excluded.deficiencia, sigla_deficiencia = excluded.sigla_deficiencia,
             updated_at = now()",
    )
    .bind(Json(&health))
    .execute(&mut *tx)
    .await?;

    if let Some(found) = especialidades {
        changes.extend(write_especialidades(&mut tx, &nins, known, found).await?);
    }

    // The member_reports.rs tables, now that every member has its row.
    let sources = [&others, run_wide];
    let more = member_reports::write(&mut tx, q.agrupamento, &nins, &known, &sources);
    changes.extend(more.await?);
    // 3. Exits: still marked present in this agrupamento, but no longer in its grid. Someone who
    //    moved shows up again in the other agrupamento's import (agrupamento change + comeback).
    let gone: Vec<(String,)> = sqlx::query_as(
        "update members set present = false
         where agrupamento = $1 and present and nin <> all($2)
         returning nin",
    )
    .bind(q.agrupamento)
    .bind(&nins)
    .fetch_all(&mut *tx)
    .await?;
    let exits = gone.len();
    changes.extend(gone.into_iter().map(|(nin,)| Change {
        nin,
        field: "present".into(),
        old_value: Some("true".into()),
        new_value: Some("false".into()),
    }));

    // 4. History.
    sqlx::query(
        "insert into member_changes (nin, import_id, field, old_value, new_value)
         select r.nin, $2, r.field, r.old_value, r.new_value
         from jsonb_to_recordset($1) as r(nin text, field text, old_value text, new_value text)",
    )
    .bind(Json(&changes))
    .bind(q.id)
    .execute(&mut *tx)
    .await?;

    // 5. Done, with counts only.
    sqlx::query(
        "update member_imports set status = 'done', finished_at = now(), error = null,
             members_seen = $2, changes = $3, exits = $4, warning = $5
         where id = $1",
    )
    .bind(q.id)
    .bind(rows.len() as i32)
    .bind(changes.len() as i32)
    .bind(exits as i32)
    .bind(warning)
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

/// Especialidades of this import's members: diffed against the stored ones, then replaced
/// wholesale (what SIIE no longer lists goes, and its removal is in the changes).
async fn write_especialidades(
    tx: &mut sqlx::PgConnection,
    nins: &[String],
    known: impl Fn(&str) -> bool,
    found: Especialidades,
) -> Result<Vec<Change>, sqlx::Error> {
    let new: Especialidades = found
        .into_iter()
        .filter(|((nin, _, _), _)| nins.binary_search(nin).is_ok())
        .collect();
    let old: Vec<(String, i16, String, Option<String>)> = sqlx::query_as(
        "select nin, seccao_tipo, especialidade, achieved_on::text
         from member_especialidades where nin = any($1)",
    )
    .bind(nins)
    .fetch_all(&mut *tx)
    .await?;
    let old: Especialidades = old
        .into_iter()
        .map(|(nin, tipo, name, on)| ((nin, tipo, name), on))
        .collect();
    let changes = especialidade_changes(&old, &new, known);

    sqlx::query("delete from member_especialidades where nin = any($1)")
        .bind(nins)
        .execute(&mut *tx)
        .await?;
    let rows: Vec<Value> = new
        .iter()
        .map(|((nin, tipo, name), on)| {
            json!({"nin": nin, "seccao_tipo": tipo, "especialidade": name, "achieved_on": on})
        })
        .collect();
    sqlx::query(
        "insert into member_especialidades (nin, seccao_tipo, especialidade, achieved_on)
         select * from jsonb_to_recordset($1) as r(nin text, seccao_tipo smallint,
             especialidade text, achieved_on date)",
    )
    .bind(Json(&rows))
    .execute(&mut *tx)
    .await?;
    Ok(changes)
}

/// One import as /api/auth/me shows it (times in Unix seconds).
#[derive(sqlx::FromRow)]
struct ImportRow {
    agrupamento: i16,
    name: Option<String>,
    status: String,
    queued_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    members_seen: Option<i32>,
    changes: Option<i32>,
    exits: Option<i32>,
    error: Option<String>,
    warning: Option<String>,
}

/// The `imports` part of /api/auth/me: per granted agrupamento, its latest import if it is
/// queued, running or finished in the last RECENT_IMPORT_HOURS. Shown to everyone whose grants
/// cover the agrupamento, not only to whoever started it.
pub async fn summary(db: &PgPool, account_id: Uuid) -> Result<Value, sqlx::Error> {
    let rows: Vec<ImportRow> = sqlx::query_as(
        "select * from (
             select distinct on (i.agrupamento) i.agrupamento, a.name, i.status,
                 extract(epoch from i.queued_at)::bigint as queued_at,
                 extract(epoch from i.started_at)::bigint as started_at,
                 extract(epoch from i.finished_at)::bigint as finished_at,
                 i.members_seen, i.changes, i.exits, i.error, i.warning
             from member_imports i
             left join agrupamentos a on a.code = i.agrupamento
             where i.agrupamento in (select agrupamento from account_grants where account_id = $1)
             order by i.agrupamento, i.id desc
         ) latest
         where status in ('queued', 'running')
            or finished_at > extract(epoch from now() - make_interval(hours => $2))::bigint
         order by agrupamento",
    )
    .bind(account_id)
    .bind(RECENT_IMPORT_HOURS)
    .fetch_all(db)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "agrupamento": r.agrupamento,
                "name": r.name,
                "status": r.status,
                "queuedAt": r.queued_at,
                "startedAt": r.started_at,
                "finishedAt": r.finished_at,
                "members": r.members_seen,
                "changes": r.changes,
                "exits": r.exits,
                "error": r.error,
                "warning": r.warning,
            })
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A grid row with every kept column, as SIIE sends them, plus some that must be dropped.
    pub(crate) fn row() -> Map<String, Value> {
        let v = json!({
            "nin": "1234567890123", "nome": "Nome Apagado", "nif": "999999990",
            "email": "x@y.pt", "morada": "Rua X", "totem": null, "siglasexo": "F",
            "datanascimento": "2011-03-12T00:00:00", "agrupamento": "0455", "nucleo": "0505",
            "regiao": "05", "elemnucleo": "0000", "elemregiao": "00", "elemNacional": false,
            "situacao": "Ativo", "categoria": "Lobito", "ordem": 20, "seccao": "Lobitos",
            "ccartao": true, "dataadmissao": "2019-10-01T00:00:00", "datapromessa": null,
            "profissao": "", "formador": false, "diretorformacao": false, "imagensnao": true,
            "cp1": "4400", "cp2": "123", "localidade": "Gaia", "codigopostal": "Vila Nova de Gaia",
            "naturalidade": "Porto", "deficiencia": null, "sigladeficiencia": null,
        });
        v.as_object().unwrap().clone()
    }

    #[test]
    fn parse_keeps_only_the_allowlist() {
        let p = parse(&row()).unwrap();
        let m = &p.member;
        assert_eq!(m.nin, "1234567890123");
        assert_eq!(m.birth_month.as_deref(), Some("2011-03-01"));
        assert_eq!(
            (m.agrupamento, m.nucleo, m.regiao),
            (Some(455), Some(505), Some(5))
        );
        // All-zero "none" codes are null.
        assert_eq!((m.el_nucleo, m.el_regiao), (None, None));
        assert_eq!(m.categoria_ordem, Some(20));
        assert_eq!(m.admitted_on.as_deref(), Some("2019-10-01"));
        assert_eq!(
            (m.promised_on.as_deref(), m.profissao.as_deref()),
            (None, None)
        );
        assert_eq!((m.cartao, m.refuses_image), (Some(true), Some(true)));
        assert_eq!(p.address.cp4.as_deref(), Some("4400"));
        // Nothing of nome/nif/email/morada survives anywhere.
        let all = format!("{p:?}");
        for dropped in ["Nome Apagado", "999999990", "x@y.pt", "Rua X"] {
            assert!(!all.contains(dropped), "{dropped} leaked");
        }
    }

    #[test]
    fn parse_fails_loudly() {
        let mut r = row();
        r.remove("situacao");
        assert!(matches!(parse(&r), Err(SiieError::MemberFieldMissing(k)) if k == "situacao"));
        let mut r = row();
        r.insert("dataadmissao".into(), json!("01/10/2019"));
        assert!(matches!(parse(&r), Err(SiieError::BadDate(k)) if k == "dataadmissao"));
        let mut r = row();
        r.insert("nin".into(), json!(" "));
        assert!(matches!(parse(&r), Err(SiieError::MissingNin)));
    }

    #[test]
    fn especialidades_keep_the_earliest_date() {
        let mut into = Especialidades::new();
        for on in ["2019-01-01T00:00:00", "2018-06-16T00:00:00"] {
            let r =
                json!({"nin": "1", "nome": "X", "especialidade": "Canoísta", "datarealizacao": on});
            parse_especialidade(r.as_object().unwrap(), 2, &mut into).unwrap();
        }
        assert_eq!(
            into.into_iter().collect::<Vec<_>>(),
            vec![(
                ("1".into(), 2, "Canoísta".into()),
                Some("2018-06-16".into())
            )]
        );
    }

    #[test]
    fn flags_accept_siie_spellings() {
        let r = json!({"a": "Sim", "b": "0", "c": true, "d": "?"});
        let r = r.as_object().unwrap();
        assert_eq!(
            (flag(r, "a"), flag(r, "b"), flag(r, "c"), flag(r, "d")),
            (Some(true), Some(false), Some(true), None)
        );
    }

    #[test]
    fn totem_prefers_siie_and_is_stable() {
        assert_eq!(
            totem("1", Some("Lince Sábio".into())),
            ("Lince Sábio".into(), true)
        );
        let (a, from_siie) = totem("1234567890123", None);
        assert!(!from_siie);
        assert_eq!(a, totem("1234567890123", None).0);
        let (animal, adjetivo) = a.split_once(' ').unwrap();
        assert!(ANIMAIS.contains(&animal) && ADJETIVOS.contains(&adjetivo));
    }

    #[test]
    fn diff_records_entries_changes_and_comebacks() {
        let new = parse(&row()).unwrap().member;
        // First sighting: only "present".
        let first = diff(None, &new);
        assert_eq!(first.len(), 1);
        assert_eq!(
            (first[0].field.as_str(), first[0].new_value.as_deref()),
            ("present", Some("true"))
        );

        // Stored as Postgres' to_jsonb would give it: same values, so no changes.
        let Value::Object(mut stored) = serde_json::to_value(&new).unwrap() else {
            panic!()
        };
        stored.insert("present".into(), json!(true));
        assert!(diff(Some(&stored), &new).is_empty());

        // A categoria change, after an exit: two changes.
        stored.insert("categoria".into(), json!("Aspirante a Lobito"));
        stored.insert("present".into(), json!(false));
        let fields: Vec<_> = diff(Some(&stored), &new)
            .into_iter()
            .map(|c| (c.field, c.old_value, c.new_value))
            .collect();
        assert_eq!(
            fields,
            vec![
                (
                    "categoria".into(),
                    Some("Aspirante a Lobito".into()),
                    Some("Lobito".into())
                ),
                ("present".into(), Some("false".into()), Some("true".into())),
            ]
        );
    }
}

/// Database tests: run against DATABASE_URL (local yasa_dev) with `cargo test -- --ignored`.
/// They use a throwaway account, agrupamentos 9101 (whole grant) and 9102 (secção grant) and
/// NINs starting with TEST91, and clean up after.
#[cfg(test)]
mod db_tests {
    use super::*;

    const WHOLE: i16 = 9101;

    async fn setup() -> (AppState, Uuid) {
        dotenvy::dotenv().ok();
        let state = AppState::from_env(false).expect("DATABASE_URL and LOGIN_KEY_SECRET");
        cleanup(&state.db).await;
        let db = &state.db;
        let (id,): (Uuid,) = sqlx::query_as(
            "insert into accounts (nif_key, label) values ('\\x02', 'members db test') returning id",
        )
        .fetch_one(db)
        .await
        .unwrap();
        for sql in [
            "insert into agrupamentos (code, name) values (9101, 'Teste 9101'), (9102, 'Teste 9102')",
            "insert into seccoes (agrupamento, tipo) values (9102, 1)",
        ] {
            sqlx::query(sql).execute(db).await.unwrap();
        }
        sqlx::query(
            "insert into account_grants (account_id, agrupamento, seccao_tipo, seccao_numero)
             values ($1, 9101, null, null), ($1, 9102, 1, 1)",
        )
        .bind(id)
        .execute(db)
        .await
        .unwrap();
        (state, id)
    }

    async fn cleanup(db: &PgPool) {
        for sql in [
            "delete from members where nin like 'TEST91%'",
            "delete from member_imports where agrupamento in (9101, 9102)",
            "delete from accounts where label = 'members db test'",
            "delete from seccoes where agrupamento in (9101, 9102)",
            "delete from agrupamentos where code in (9101, 9102)",
        ] {
            sqlx::query(sql).execute(db).await.unwrap();
        }
    }

    /// A grid row for agrupamento 9101.
    fn row(nin: &str, categoria: &str) -> Parsed {
        let mut r = super::tests::row();
        r.insert("nin".into(), json!(nin));
        r.insert("agrupamento".into(), json!("9101"));
        r.insert("categoria".into(), json!(categoria));
        parse(&r).unwrap()
    }

    /// A lobitos progress row for one member: etapa1 done, etapa2 as given.
    fn progress(nin: &str, etapa2: Option<&str>) -> member_reports::Fetched {
        let r = json!({
            "nin": nin, "nome": "Nome Apagado", "ladesao": null,
            "letapa1": "2024-05-01T00:00:00", "letapa2": etapa2, "letapa3": null,
            "lanilhamerito": null,
        });
        let lobitos = member_reports::COLUMN_REPORTS
            .iter()
            .find(|r| r.report == "elementos/progresso/lista-lobitos")
            .unwrap();
        let mut f = member_reports::Fetched::default();
        f.add_column_row(lobitos, r.as_object().unwrap()).unwrap();
        f
    }

    /// What an import fetched, with especialidades as (nin, seccao_tipo, name, date).
    fn fetched(
        rows: Vec<Parsed>,
        others: member_reports::Fetched,
        especialidades: &[(&str, i16, &str, &str)],
    ) -> Fetched {
        let especialidades = especialidades
            .iter()
            .map(|(nin, tipo, name, on)| {
                (((*nin).into(), *tipo, (*name).into()), Some((*on).into()))
            })
            .collect();
        Fetched {
            rows,
            especialidades: Some(especialidades),
            others,
            warning: None,
        }
    }

    /// A fresh import row for 9101, as `queue` would add it.
    async fn new_import(db: &PgPool) -> Queued {
        let (id,): (i64,) = sqlx::query_as(
            "insert into member_imports (agrupamento, status, started_at)
             values (9101, 'running', now()) returning id",
        )
        .fetch_one(db)
        .await
        .unwrap();
        Queued {
            id,
            agrupamento: WHOLE,
        }
    }

    async fn changes(
        db: &PgPool,
        import: i64,
    ) -> Vec<(String, String, Option<String>, Option<String>)> {
        sqlx::query_as(
            "select nin, field, old_value, new_value from member_changes
             where import_id = $1 order by nin, field",
        )
        .bind(import)
        .fetch_all(db)
        .await
        .unwrap()
    }

    async fn import_counts(
        db: &PgPool,
        id: i64,
    ) -> (String, Option<i32>, Option<i32>, Option<i32>) {
        sqlx::query_as(
            "select status, members_seen, changes, exits from member_imports where id = $1",
        )
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
    }

    fn c(
        nin: &str,
        field: &str,
        old: Option<&str>,
        new: Option<&str>,
    ) -> (String, String, Option<String>, Option<String>) {
        (
            nin.into(),
            field.into(),
            old.map(Into::into),
            new.map(Into::into),
        )
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn queue_only_whole_stale_agrupamentos() {
        let (state, id) = setup().await;
        let db = &state.db;

        // Only the whole agrupamento is queued, and only once.
        let q = queue(db, id, 1).await.unwrap();
        assert_eq!(
            q.iter().map(|q| q.agrupamento).collect::<Vec<_>>(),
            vec![WHOLE]
        );
        assert!(queue(db, id, 1).await.unwrap().is_empty());
        assert_eq!(summary(db, id).await.unwrap()[0]["status"], "queued");

        // Done: fresh, nothing to queue. Older than sync_days: queued again.
        store(
            db,
            &q[0],
            fetched(
                vec![row("TEST91A", "Lobito")],
                member_reports::Fetched::default(),
                &[],
            ),
            &member_reports::Fetched::default(),
        )
        .await
        .unwrap();
        assert!(queue(db, id, 1).await.unwrap().is_empty());
        sqlx::query(
            "update member_imports set finished_at = now() - interval '2 days' where id = $1",
        )
        .bind(q[0].id)
        .execute(db)
        .await
        .unwrap();
        let again = queue(db, id, 1).await.unwrap();
        assert_eq!(again.len(), 1);

        // A run that died 11 minutes ago is failed, so the agrupamento can be queued again.
        sqlx::query(
            "update member_imports set queued_at = now() - interval '11 minutes' where id = $1",
        )
        .bind(again[0].id)
        .execute(db)
        .await
        .unwrap();
        assert_eq!(queue(db, id, 1).await.unwrap().len(), 1);
        assert_eq!(import_counts(db, again[0].id).await.0, "failed");

        cleanup(db).await;
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn imports_record_entries_changes_exits_and_comebacks() {
        let (state, id) = setup().await;
        let db = &state.db;

        // 1. Two new members: one "present" entry each.
        let i1 = new_import(db).await;
        store(
            db,
            &i1,
            fetched(
                vec![row("TEST91B", "Lobito"), row("TEST91A", "Lobito")],
                progress("TEST91A", None),
                &[("TEST91A", 2, "Canoísta", "2018-06-16")],
            ),
            &member_reports::Fetched::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            changes(db, i1.id).await,
            vec![
                c("TEST91A", "present", None, Some("true")),
                c("TEST91B", "present", None, Some("true")),
            ]
        );
        assert_eq!(
            import_counts(db, i1.id).await,
            ("done".into(), Some(2), Some(2), Some(0))
        );
        let (addresses, health): (i64, i64) = sqlx::query_as(
            "select (select count(*) from member_addresses where nin like 'TEST91%'),
                    (select count(*) from member_health where nin like 'TEST91%')",
        )
        .fetch_one(db)
        .await
        .unwrap();
        assert_eq!((addresses, health), (2, 2));

        // 2. A moves up, B is gone.
        let i2 = new_import(db).await;
        store(
            db,
            &i2,
            fetched(
                vec![row("TEST91A", "Explorador")],
                progress("TEST91A", Some("2025-06-01T00:00:00")),
                &[
                    ("TEST91A", 2, "Canoísta", "2018-06-16"),
                    ("TEST91A", 2, "Nadador", "2025-07-01"),
                ],
            ),
            &member_reports::Fetched::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            changes(db, i2.id).await,
            vec![
                c("TEST91A", "categoria", Some("Lobito"), Some("Explorador")),
                c(
                    "TEST91A",
                    "especialidade.2.Nadador",
                    None,
                    Some("2025-07-01")
                ),
                c("TEST91A", "progresso.letapa2", None, Some("2025-06-01")),
                c("TEST91B", "present", Some("true"), Some("false")),
            ]
        );
        assert_eq!(
            import_counts(db, i2.id).await,
            ("done".into(), Some(1), Some(4), Some(1))
        );
        let (etapa1,): (Option<String>,) =
            sqlx::query_as("select letapa1::text from member_progress where nin = 'TEST91A'")
                .fetch_one(db)
                .await
                .unwrap();
        assert_eq!(etapa1.as_deref(), Some("2024-05-01"));

        // 3. B is back.
        let i3 = new_import(db).await;
        store(
            db,
            &i3,
            fetched(
                vec![row("TEST91A", "Explorador"), row("TEST91B", "Lobito")],
                // Same progress as before: no change. (Absent from the report would mean
                // SIIE cleared it: nulls, logged.)
                progress("TEST91A", Some("2025-06-01T00:00:00")),
                &[],
            ),
            &member_reports::Fetched::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            changes(db, i3.id).await,
            vec![
                c(
                    "TEST91A",
                    "especialidade.2.Canoísta",
                    Some("2018-06-16"),
                    None
                ),
                c(
                    "TEST91A",
                    "especialidade.2.Nadador",
                    Some("2025-07-01"),
                    None
                ),
                c("TEST91B", "present", Some("false"), Some("true")),
            ]
        );

        // The Painel sees the latest import of 9101, done; nothing for the secção-only 9102.
        let s = summary(db, id).await.unwrap();
        assert_eq!(s.as_array().unwrap().len(), 1);
        assert_eq!(
            (s[0]["agrupamento"].as_i64(), s[0]["status"].as_str()),
            (Some(9101), Some("done"))
        );
        assert_eq!(s[0]["name"], "Teste 9101");

        cleanup(db).await;
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn imports_remove_only_records_listed_under_their_agrupamento() {
        let (state, _) = setup().await;
        let db = &state.db;
        let record = |table: &str, row: Value| {
            let r = member_reports::RECORD_REPORTS
                .iter()
                .find(|r| r.table == table)
                .unwrap();
            let mut f = member_reports::Fetched::default();
            f.add_record_row(r, row.as_object().unwrap()).unwrap();
            f.records.remove(r.table).unwrap()
        };
        // A cargo listed under 9101, and an activity listed under another agrupamento (9199).
        let mut others = member_reports::Fetched::default();
        others.records.insert(
            "member_cargos",
            record(
                "member_cargos",
                json!({"rid": 9101000001i64, "nin": "TEST91A", "agrupamento": "9101",
                    "cargo": "Chefe", "siglacargo": "CU", "agrup": "9101",
                    "datainicio": null, "datafim": null, "activo": true}),
            ),
        );
        others.records.insert(
            "member_atividades",
            record(
                "member_atividades",
                json!({"rid": 9101000002i64, "nin": "TEST91A", "agrupamento": "9199",
                    "idatividade": "a1", "siglatipo": "acamp", "siglagrupo": "agr",
                    "datainicio": null, "datafim": null, "noites": 2, "noitescampo": 2,
                    "contanoitescampo": true}),
            ),
        );
        let none = member_reports::Fetched::default();
        let i1 = new_import(db).await;
        store(
            db,
            &i1,
            fetched(vec![row("TEST91A", "Lobito")], others, &[]),
            &none,
        )
        .await
        .unwrap();
        // First sighting: stored, nothing logged.
        assert_eq!(
            changes(db, i1.id).await,
            vec![c("TEST91A", "present", None, Some("true"))]
        );

        // Next import of 9101 lists neither: only the cargo (listed under 9101) goes.
        let i2 = new_import(db).await;
        let empty = member_reports::Fetched::default();
        store(
            db,
            &i2,
            fetched(vec![row("TEST91A", "Lobito")], empty, &[]),
            &none,
        )
        .await
        .unwrap();
        assert_eq!(
            changes(db, i2.id).await,
            vec![c("TEST91A", "cargo.9101000001", Some("Chefe"), None)]
        );
        let (cargos, atividades): (i64, i64) = sqlx::query_as(
            "select (select count(*) from member_cargos where nin = 'TEST91A'),
                    (select count(*) from member_atividades where nin = 'TEST91A')",
        )
        .fetch_one(db)
        .await
        .unwrap();
        assert_eq!((cargos, atividades), (0, 1));

        cleanup(db).await;
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn a_skipped_report_leaves_its_columns_untouched() {
        let (state, _) = setup().await;
        let db = &state.db;
        let none = member_reports::Fetched::default();
        let i1 = new_import(db).await;
        let first = fetched(
            vec![row("TEST91A", "Lobito")],
            progress("TEST91A", None),
            &[],
        );
        store(db, &i1, first, &none).await.unwrap();

        // The lobitos progress report failed this time: nothing it holds may become null.
        let mut failed = member_reports::Fetched::default();
        failed
            .skipped
            .push(("elementos/progresso/lista-lobitos", "erro".into()));
        let i2 = new_import(db).await;
        store(
            db,
            &i2,
            fetched(vec![row("TEST91A", "Lobito")], failed, &[]),
            &none,
        )
        .await
        .unwrap();
        assert!(changes(db, i2.id).await.is_empty());
        let (etapa1,): (Option<String>,) =
            sqlx::query_as("select letapa1::text from member_progress where nin = 'TEST91A'")
                .fetch_one(db)
                .await
                .unwrap();
        assert_eq!(etapa1.as_deref(), Some("2024-05-01"));

        cleanup(db).await;
    }
}
