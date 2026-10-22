//! Agrupamento digest: how many members an agrupamento had per ano escutista, in total and per
//! secção, rebuilt from each member's admission, progression, age and exit data, plus how well
//! the progresso is filled in. The digest itself is counts only; the secção view lists the
//! secção's current scouts by totem (never a NIN), with their progression and especialidades.
//!
//! - An ano escutista runs from October 1 to September 30; 2024 means 2024/25.
//! - A member counts in every year from their start to their end, both included ("active at any
//!   time in it"). The totals don't depend on progresso: admission and last censo set them.
//! - Their secção in a year is the one they were in on its last day (September 30), so the secções
//!   always add up to the total. Dated records decide it first; where there are none, their age
//!   does, and that count is reported apart (`by_age`) so the page can mark it as estimated.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use secrecy::ExposeSecret;

use crate::{
    AppState,
    auth::{ApiError, CurrentAccount, db_error, keyed_hash},
    criteria::{self, InstrutorSpan},
};

/// Where a member is counted in one year.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Bucket {
    Lobitos,
    Exploradores,
    Pioneiros,
    Caminheiros,
    Dirigentes,
    /// Neither a dated record nor their age says where the member was that year (no birth
    /// month, or an adult with no promessa de dirigente or cargo).
    SemRegisto,
}

/// The four secções: tipo (as in `seccoes` and the especialidades reports), bucket, the prefix of
/// their member_progress columns and their promessa column. Arrays in `MemberDates` follow this
/// order.
pub(crate) const SECCOES: [(i16, Bucket, &str, &str); 4] = [
    (1, Bucket::Lobitos, "l", "promessalobito"),
    (2, Bucket::Exploradores, "e", "promessaexplorador"),
    (3, Bucket::Pioneiros, "p", "promessapioneiro"),
    (4, Bucket::Caminheiros, "c", "promessacaminheiro"),
];
/// The progression steps of a secção, in order: member_progress columns after the prefix.
const STEPS: [&str; 5] = ["adesao", "etapa1", "etapa2", "etapa3", "anilhamerito"];

/// Age bands per secção, with age = the ano escutista's first year - birth year. Checked against
/// SIIE's own secção for every active member on 2026-10-22: each change-over age goes to the
/// secção most members of that age were in (10: 124 Lobitos / 91 Exploradores; 14: 80
/// Exploradores / 60 Pioneiros; 18: 55 Pioneiros / 62 Caminheiros).
const AGE_BANDS: [(i32, i32, Bucket); 4] = [
    (6, 10, Bucket::Lobitos),
    (11, 14, Bucket::Exploradores),
    (15, 17, Bucket::Pioneiros),
    (18, 23, Bucket::Caminheiros),
];

/// The dates one member's years are built from, all `YYYY-MM-DD`. Loaded by `load`.
#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct MemberDates {
    /// Only to fetch the secção view's details; never sent.
    pub nin: String,
    pub agrupamento: Option<i16>,
    pub admitted_on: Option<String>,
    /// The earliest adesão or promessa in any secção, or first cargo in this agrupamento.
    pub first_record: Option<String>,
    /// The latest dated record: admission, adesão, etapa, anilha, promessa or cargo.
    pub last_record: Option<String>,
    /// When YASA first saw the member: the start of an active member with no dates at all.
    pub first_seen: String,
    /// Present in the latest import and situação "Ativo..." (includes "Ativo (Formação)").
    pub active: bool,
    /// Former members (saida-elementos-por-ano): the year they left, and the last censo they
    /// were in. In SIIE ano_saida is always ultimoano_censos + 1.
    pub ano_saida: Option<i32>,
    pub ultimoano_censos: Option<i32>,
    /// `YYYY-MM-01`: the age fallback.
    pub birth_month: Option<String>,
    /// The earliest stage date in each secção (adesão, else the first etapa SIIE holds), in
    /// SECCOES order: when the member was first registered in it. SIIE often holds a later etapa
    /// without the adesão (about 110 active members, 2026-10-22), so the adesão alone isn't used.
    pub first_steps: Vec<Option<String>>,
    /// Promessa in each secção, in SECCOES order.
    pub promessas: Vec<Option<String>>,
    /// Every dated record of each secção: adesão, etapas, anilha, promessa, especialidades.
    pub ldates: Vec<String>,
    pub edates: Vec<String>,
    pub pdates: Vec<String>,
    pub cdates: Vec<String>,
    /// The promessa de dirigente or the first cargo in this agrupamento, whichever came first.
    pub dirigente_since: Option<String>,
    /// Current secção and categoria, as SIIE writes them.
    pub seccao: Option<String>,
    pub categoria: Option<String>,
}

/// Of a year's count in each secção, how many were placed by age (no dated record).
#[derive(Serialize, Debug, Default, Clone, PartialEq)]
pub struct ByAge {
    pub lobitos: u32,
    pub exploradores: u32,
    pub pioneiros: u32,
    pub caminheiros: u32,
}

/// One year of the digest.
#[derive(Serialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct YearRow {
    /// The year the ano escutista starts in: 2024 = 2024/25.
    pub year: i32,
    pub label: String,
    pub total: u32,
    pub lobitos: u32,
    pub exploradores: u32,
    pub pioneiros: u32,
    pub caminheiros: u32,
    pub dirigentes: u32,
    pub sem_registo: u32,
    pub by_age: ByAge,
}

/// The ano escutista a `YYYY-MM-DD` date falls in: October onwards starts a new one.
pub fn scout_year(date: &str) -> i32 {
    let year: i32 = date[..4].parse().unwrap_or_default();
    let month: u32 = date[5..7].parse().unwrap_or_default();
    if month >= 10 { year } else { year - 1 }
}

/// When a member started: the admission date, else the first adesão, promessa or cargo. An
/// active member with none of these is placed when YASA first saw them (they are here now). A
/// former member with none is None: nothing says when they were here (old SIIE records).
pub fn start_date(m: &MemberDates) -> Option<&str> {
    m.admitted_on
        .as_deref()
        .or(m.first_record.as_deref())
        .or(m.active.then_some(m.first_seen.as_str()))
}

/// The years a member counts in, with their bucket in each and whether age decided it, up to the
/// current year. Empty for a former member with no dates (see `start_date`).
pub fn member_years(m: &MemberDates, today: &str) -> Vec<(i32, Bucket, bool)> {
    let current = scout_year(today);
    let Some(start_date) = start_date(m) else {
        return Vec::new();
    };
    let start = scout_year(start_date);

    // End: active members are still here. A former member's last year is their last censo
    // (censo 2025 = 2025/26: the ones who left in 2026 had their last censo in 2025), else
    // ano_saida - 1, else the year of their latest dated record.
    let end = if m.active {
        current
    } else if let Some(y) = m.ultimoano_censos.or(m.ano_saida.map(|y| y - 1)) {
        y
    } else {
        scout_year(m.last_record.as_deref().unwrap_or(start_date))
    };
    let end = end.clamp(start, current);

    (start..=end)
        .map(|y| {
            let (b, by_age) = bucket(m, y, y == current);
            (y, b, by_age)
        })
        .collect()
}

/// Where a member was on the last day (September 30) of ano escutista `year`, and whether only
/// their age says so.
fn bucket(m: &MemberDates, year: i32, is_current: bool) -> (Bucket, bool) {
    let year_start = format!("{year}-10-01");
    let year_end = format!("{}-09-30", year + 1);

    // 1. Dirigente from the promessa de dirigente or first cargo onwards.
    if on_or_before(&m.dirigente_since, &year_end).is_some() {
        return (Bucket::Dirigentes, false);
    }
    // 2. This year, an active member is where SIIE says they are now.
    if is_current
        && m.active
        && let Some(b) = current_bucket(m.seccao.as_deref(), m.categoria.as_deref())
    {
        return (b, false);
    }
    // 3. A record dated within this ano escutista (adesão, etapa, promessa, especialidade):
    //    the secção of the latest one. ISO dates compare as text.
    let dates = [&m.ldates, &m.edates, &m.pdates, &m.cdates];
    let in_year = SECCOES
        .iter()
        .zip(dates)
        .filter_map(|((_, b, ..), ds)| {
            ds.iter()
                .filter(|d| **d >= year_start && **d <= year_end)
                .max()
                .map(|d| (d, *b))
        })
        .max_by_key(|(d, _)| *d);
    if let Some((_, b)) = in_year {
        return (b, false);
    }
    // 4. The latest secção they were registered in by the year's end (its earliest stage date),
    //    unless their age is out of its band by more than a year (they moved on, and the next
    //    secção wasn't recorded).
    let age = m
        .birth_month
        .as_deref()
        .and_then(|b| b.get(..4)?.parse::<i32>().ok())
        .map(|born| year - born);
    let adesao = SECCOES
        .iter()
        .zip(&m.first_steps)
        .filter_map(|((_, b, ..), d)| on_or_before(d, &year_end).map(|d| (d, *b)))
        .max_by_key(|(d, _)| *d);
    if let Some((_, b)) = adesao
        && age.is_none_or(|a| fits(b, a))
    {
        return (b, false);
    }
    // 5. Their age band: an estimate, reported as such.
    if let Some(b) = age.and_then(age_bucket) {
        return (b, true);
    }
    (Bucket::SemRegisto, false)
}

/// The secção of an age, by AGE_BANDS. None below 6 or above 23.
fn age_bucket(age: i32) -> Option<Bucket> {
    AGE_BANDS
        .iter()
        .find(|(lo, hi, _)| (*lo..=*hi).contains(&age))
        .map(|(.., b)| *b)
}

/// Whether `age` is within a year of `b`'s age band.
fn fits(b: Bucket, age: i32) -> bool {
    AGE_BANDS
        .iter()
        .any(|(lo, hi, band)| *band == b && (lo - 1..=hi + 1).contains(&age))
}

/// The `i`th date of `v` if it is on or before `end`.
pub(crate) fn dated_at<'a>(v: &'a [Option<String>], i: usize, end: &str) -> Option<&'a str> {
    v.get(i).and_then(|d| on_or_before(d, end))
}

/// `date` if it is on or before `end`. ISO dates compare as text.
fn on_or_before<'a>(date: &'a Option<String>, end: &str) -> Option<&'a str> {
    date.as_deref().filter(|d| *d <= end)
}

/// The bucket of SIIE's current secção / categoria text, if it names one.
fn current_bucket(seccao: Option<&str>, categoria: Option<&str>) -> Option<Bucket> {
    let text = format!(
        "{} {}",
        categoria.unwrap_or_default(),
        seccao.unwrap_or_default()
    )
    .to_lowercase();
    // Categoria first: a dirigente's secção is the one they lead, not the one they're in.
    let words: [(&[&str], Bucket); 5] = [
        (&["dirigente", "auxiliar"], Bucket::Dirigentes),
        (&["caminh", "clã"], Bucket::Caminheiros),
        (&["pione", "comunidade"], Bucket::Pioneiros),
        (&["explor", "expedi"], Bucket::Exploradores),
        (&["lobit", "alcateia"], Bucket::Lobitos),
    ];
    words
        .into_iter()
        .find(|(ws, _)| ws.iter().any(|w| text.contains(w)))
        .map(|(_, b)| b)
}

/// One row per year from the earliest start to the current year; years with nobody are 0.
pub fn digest(members: &[MemberDates], today: &str) -> Vec<YearRow> {
    let current = scout_year(today);
    let all: Vec<(i32, Bucket, bool)> = members
        .iter()
        .flat_map(|m| member_years(m, today))
        .collect();
    let Some(first) = all.iter().map(|(y, ..)| *y).min() else {
        return Vec::new();
    };
    let mut rows: Vec<YearRow> = (first..=current)
        .map(|year| YearRow {
            year,
            label: year_label(year),
            ..Default::default()
        })
        .collect();
    for (y, b, by_age) in all {
        let r = &mut rows[(y - first) as usize];
        r.total += 1;
        let (count, aged) = match b {
            Bucket::Lobitos => (&mut r.lobitos, &mut r.by_age.lobitos),
            Bucket::Exploradores => (&mut r.exploradores, &mut r.by_age.exploradores),
            Bucket::Pioneiros => (&mut r.pioneiros, &mut r.by_age.pioneiros),
            Bucket::Caminheiros => (&mut r.caminheiros, &mut r.by_age.caminheiros),
            Bucket::Dirigentes => {
                r.dirigentes += 1;
                continue;
            }
            Bucket::SemRegisto => {
                r.sem_registo += 1;
                continue;
            }
        };
        *count += 1;
        if by_age {
            *aged += 1;
        }
    }
    rows
}

/// "2024/25"
pub(crate) fn year_label(year: i32) -> String {
    format!("{year}/{:02}", (year + 1) % 100)
}

/// Every member ever seen in these agrupamentos, present or not, with the dates `member_years`
/// needs.
pub async fn load(db: &sqlx::PgPool, codes: &[i16]) -> Result<Vec<MemberDates>, sqlx::Error> {
    // least / greatest skip nulls, so each one is the earliest / latest date the member has.
    // array_remove drops the nulls of the per-secção record lists.
    sqlx::query_as(
        "select m.nin, m.agrupamento, m.admitted_on::text as admitted_on,
                least(p.ladesao, p.eadesao, p.padesao, p.cadesao, p.promessalobito,
                      p.promessaexplorador, p.promessapioneiro, p.promessacaminheiro,
                      p.promessadirigente, c.first_cargo)::text as first_record,
                greatest(m.admitted_on, m.promised_on,
                         p.ladesao, p.letapa1, p.letapa2, p.letapa3, p.lanilhamerito,
                         p.eadesao, p.eetapa1, p.eetapa2, p.eetapa3, p.eanilhamerito,
                         p.padesao, p.petapa1, p.petapa2, p.petapa3, p.panilhamerito,
                         p.cadesao, p.cetapa1, p.cetapa2, p.cetapa3, p.canilhamerito,
                         p.promessalobito, p.promessaexplorador, p.promessapioneiro,
                         p.promessacaminheiro, p.promessadirigente,
                         c.last_cargo)::text as last_record,
                m.first_seen_at::date::text as first_seen,
                m.present and coalesce(m.situacao like 'Ativo%', false) as active,
                p.ano_saida, p.ultimoano_censos,
                m.birth_month::text as birth_month,
                array[least(p.ladesao, p.letapa1, p.letapa2, p.letapa3, p.lanilhamerito),
                      least(p.eadesao, p.eetapa1, p.eetapa2, p.eetapa3, p.eanilhamerito),
                      least(p.padesao, p.petapa1, p.petapa2, p.petapa3, p.panilhamerito),
                      least(p.cadesao, p.cetapa1, p.cetapa2, p.cetapa3, p.canilhamerito)
                ]::text[] as first_steps,
                array[p.promessalobito, p.promessaexplorador, p.promessapioneiro,
                      p.promessacaminheiro]::text[] as promessas,
                array_remove(array[p.ladesao, p.letapa1, p.letapa2, p.letapa3, p.lanilhamerito,
                                   p.promessalobito]::text[] || e.l, null) as ldates,
                array_remove(array[p.eadesao, p.eetapa1, p.eetapa2, p.eetapa3, p.eanilhamerito,
                                   p.promessaexplorador]::text[] || e.e, null) as edates,
                array_remove(array[p.padesao, p.petapa1, p.petapa2, p.petapa3, p.panilhamerito,
                                   p.promessapioneiro]::text[] || e.p, null) as pdates,
                array_remove(array[p.cadesao, p.cetapa1, p.cetapa2, p.cetapa3, p.canilhamerito,
                                   p.promessacaminheiro]::text[] || e.c, null) as cdates,
                least(p.promessadirigente, c.first_cargo)::text as dirigente_since,
                m.seccao, m.categoria
         from members m
         left join member_progress p on p.nin = m.nin
         left join lateral (
             select min(started_on) as first_cargo,
                    greatest(max(started_on), max(ended_on)) as last_cargo
             -- A cargo's agrupamento: SIIE's agrup, else the one it's listed under (agrup is empty
             -- on about a third of the cargos, checked 2026-10-22).
             from member_cargos
             where nin = m.nin and coalesce(agrupamento, import_agrupamento) = m.agrupamento
         ) c on true
         left join lateral (
             select coalesce(array_agg(achieved_on::text) filter (where seccao_tipo = 1), '{}') as l,
                    coalesce(array_agg(achieved_on::text) filter (where seccao_tipo = 2), '{}') as e,
                    coalesce(array_agg(achieved_on::text) filter (where seccao_tipo = 3), '{}') as p,
                    coalesce(array_agg(achieved_on::text) filter (where seccao_tipo = 4), '{}') as c
             from member_especialidades where nin = m.nin and achieved_on is not null
         ) e on true
         where m.agrupamento = any($1)",
    )
    .bind(codes)
    .fetch_all(db)
    .await
}

/// The instrutor cargos of these agrupamentos whose secção is known, for the criteria. An ended
/// cargo with no end date is taken to have held only where it started.
async fn instrutor_spans(
    db: &sqlx::PgPool,
    codes: &[i16],
) -> Result<Vec<InstrutorSpan>, sqlx::Error> {
    sqlx::query_as(
        "select coalesce(agrupamento, import_agrupamento) as agrupamento, seccao_tipo as tipo,
                started_on::text as \"from\",
                case when ended_on is not null then ended_on::text
                     when active then null
                     else started_on::text end as \"to\"
         from member_cargos
         where coalesce(agrupamento, import_agrupamento) = any($1) and sigla_cargo = 'I'
           and seccao_tipo is not null and started_on is not null",
    )
    .bind(codes)
    .fetch_all(db)
    .await
}

/// Today's date from the database, so every request uses the same clock.
async fn today(db: &sqlx::PgPool) -> Result<String, ApiError> {
    let (today,): (String,) = sqlx::query_as("select current_date::text")
        .fetch_one(db)
        .await
        .map_err(db_error)?;
    Ok(today)
}

fn not_whole() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "O resumo só aparece com acesso ao agrupamento inteiro.",
    )
}

/// GET /api/agrupamentos/{code}/digest: members per ano escutista, in total and per secção, the
/// quality criteria per secção and year, and the agrupamento's own data. Needs a whole-agrupamento grant:
/// secção-only readers have no import of their agrupamento, so there's nothing to count for them.
pub async fn get(
    State(s): State<AppState>,
    acc: CurrentAccount,
    Path(code): Path<i16>,
) -> Result<Json<Value>, ApiError> {
    let db = &s.db;
    let agrupamento = granted(db, acc.id, code)
        .await
        .map_err(db_error)?
        .ok_or_else(not_whole)?;
    let today = today(db).await?;
    let members = load(db, &[code]).await.map_err(db_error)?;
    let spans = instrutor_spans(db, &[code]).await.map_err(db_error)?;
    let (last_import_at,): (Option<i64>,) = sqlx::query_as(
        "select extract(epoch from max(finished_at))::bigint from member_imports
         where agrupamento = $1 and status = 'done'",
    )
    .bind(code)
    .fetch_one(db)
    .await
    .map_err(db_error)?;
    // Former members with no date at all: left out of the years, but counted so the page says so.
    let undated = members.iter().filter(|m| start_date(m).is_none()).count();
    Ok(Json(json!({
        "agrupamento": agrupamento,
        "years": digest(&members, &today),
        "criteriaDefs": criteria::definitions(),
        "criteria": SECCOES
            .iter()
            .zip(criteria::evaluate(&members, &spans, &today))
            .map(|(s, years)| json!({"tipo": s.0, "years": years}))
            .collect::<Vec<_>>(),
        "undated": undated,
        "lastImportAt": last_import_at,
    })))
}

/// GET /api/agrupamentos/quality: every agrupamento the account holds whole, with this year's
/// quality points (the sum of the points of every criterion each of its secções fails) and which
/// those are, to rank them from worst to best.
pub async fn quality_list(
    State(s): State<AppState>,
    acc: CurrentAccount,
) -> Result<Json<Value>, ApiError> {
    let db = &s.db;
    let agrupamentos: Vec<(i16, Option<String>)> = sqlx::query_as(
        "select a.code, a.name from account_grants g join agrupamentos a on a.code = g.agrupamento
         where g.account_id = $1 and g.seccao_tipo is null order by a.code",
    )
    .bind(acc.id)
    .fetch_all(db)
    .await
    .map_err(db_error)?;
    let codes: Vec<i16> = agrupamentos.iter().map(|(c, _)| *c).collect();
    let today = today(db).await?;
    let mut by_code: HashMap<i16, Vec<MemberDates>> = HashMap::new();
    for m in load(db, &codes).await.map_err(db_error)? {
        by_code
            .entry(m.agrupamento.unwrap_or_default())
            .or_default()
            .push(m);
    }
    let spans = instrutor_spans(db, &codes).await.map_err(db_error)?;
    let current = scout_year(&today);
    let rows: Vec<Value> = agrupamentos
        .into_iter()
        .map(|(code, name)| {
            let own: Vec<InstrutorSpan> = spans
                .iter()
                .filter(|s| s.agrupamento == code)
                .cloned()
                .collect();
            let all = criteria::evaluate(
                by_code.get(&code).map_or(&[][..], Vec::as_slice),
                &own,
                &today,
            );
            // This year's column of each secção: what it fails, and the points that costs.
            let now: Vec<(i16, &criteria::CriteriaYear)> = SECCOES
                .iter()
                .zip(&all)
                .filter_map(|(sec, years)| {
                    years
                        .last()
                        .filter(|c| c.year == current)
                        .map(|c| (sec.0, c))
                })
                .collect();
            let failed: Vec<Value> = now
                .iter()
                .flat_map(|(tipo, c)| {
                    c.failed
                        .iter()
                        .map(move |key| json!({"tipo": tipo, "key": key}))
                })
                .collect();
            json!({
                "code": code,
                "name": name,
                "members": now.iter().map(|(_, c)| c.members).sum::<u32>(),
                "points": now.iter().map(|(_, c)| c.points).sum::<u32>(),
                "failed": failed,
            })
        })
        .collect();
    Ok(Json(json!({"defs": criteria::definitions(), "rows": rows})))
}

/// One scout of a secção, as the secção view shows them.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Scout {
    #[serde(skip)]
    nin: String,
    /// Opaque id for the member page (see `member_ref`).
    #[sqlx(skip)]
    #[serde(rename = "ref")]
    member_ref: String,
    #[serde(skip)]
    birth_month: Option<String>,
    /// Whole years today.
    #[sqlx(skip)]
    age: Option<i32>,
    totem: String,
    categoria: Option<String>,
    /// The date of each of STEPS in this secção (`YYYY-MM-DD`), null if not reached.
    steps: Vec<Option<String>>,
    /// The stage they're at: an index of STEPS (see `stage`).
    #[sqlx(skip)]
    stage: usize,
    promessa_on: Option<String>,
    /// [{name, on}] earned in this secção, newest first.
    especialidades: sqlx::types::Json<Value>,
}

/// The stage a scout is at: the STEPS index of their latest dated step (0 adesão, e.g. Pata-tenra,
/// 1-3 the etapas, 4 anilha de mérito), as SIIE's own progresso distribution counts them (checked
/// against 0005 on 2026-10-22). Nothing dated is 0 too: they're in Adesão, where SIIE leaves them
/// out. By order, not by date: SIIE may hold a later stage without an earlier one.
fn stage(steps: &[Option<String>]) -> usize {
    steps.iter().rposition(Option::is_some).unwrap_or(0)
}

/// The NINs of the active members who are in `target` this ano escutista, by the same rule as
/// the digest's current year (so the secção view and the digest's count agree).
fn current_members(members: &[MemberDates], target: Bucket, today: &str) -> Vec<String> {
    let current = scout_year(today);
    members
        .iter()
        .filter(|m| m.active && bucket(m, current, true).0 == target)
        .map(|m| m.nin.clone())
        .collect()
}

/// GET /api/agrupamentos/{code}/seccoes/{tipo}: the scouts in a secção this ano escutista, with
/// their progression steps, promessa and especialidades there, and the secção's progresso
/// quality. Same access rule as the digest.
pub async fn seccao(
    State(s): State<AppState>,
    acc: CurrentAccount,
    Path((code, tipo)): Path<(i16, i16)>,
) -> Result<Json<Value>, ApiError> {
    let db = &s.db;
    let i = SECCOES
        .iter()
        .position(|(t, ..)| *t == tipo)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Secção desconhecida."))?;
    let (_, target, prefix, promessa) = SECCOES[i];
    granted(db, acc.id, code)
        .await
        .map_err(db_error)?
        .ok_or_else(not_whole)?;
    let today = today(db).await?;
    let members = load(db, &[code]).await.map_err(db_error)?;
    let spans = instrutor_spans(db, &[code]).await.map_err(db_error)?;
    let nins = current_members(&members, target, &today);

    // Column names come from SECCOES / STEPS, never from the request.
    let steps: Vec<String> = STEPS
        .iter()
        .map(|c| format!("p.{prefix}{c}::text"))
        .collect();
    let mut scouts: Vec<Scout> = sqlx::query_as(&format!(
        "select m.nin, m.birth_month::text as birth_month, m.totem, m.categoria,
                array[{}] as steps, p.{promessa}::text as promessa_on,
                coalesce((
                    select json_agg(json_build_object('name', e.especialidade, 'on', e.achieved_on)
                                    order by e.achieved_on desc nulls last, e.especialidade)
                    from member_especialidades e where e.nin = m.nin and e.seccao_tipo = $2
                ), '[]') as especialidades
         from members m left join member_progress p on p.nin = m.nin
         where m.nin = any($1)
         order by m.totem",
        steps.join(", ")
    ))
    .bind(&nins)
    .bind(tipo)
    .fetch_all(db)
    .await
    .map_err(db_error)?;
    let secret = s.login_key_secret.expose_secret();
    for sc in &mut scouts {
        sc.stage = stage(&sc.steps);
        sc.member_ref = member_ref(secret, &sc.nin);
        sc.age = age(sc.birth_month.as_deref(), &today);
    }
    let mut adults = seccao_adults(db, code, tipo).await.map_err(db_error)?;
    // SIIE's own sub-unidades when it has them, else estimated from the number of elementos.
    let subunidades = match siie_subunidades(db, code, tipo).await.map_err(db_error)? {
        Some(sizes) => json!({"sizes": sizes, "estimated": false}),
        None => {
            let sizes = estimate_subunidades(scouts.len() as u32, subunidade_min(tipo));
            json!({"sizes": sizes, "estimated": true})
        }
    };
    for a in &mut adults {
        a.member_ref = member_ref(secret, &a.nin);
        a.age = age(a.birth_month.as_deref(), &today);
    }

    // The ano escutista these scouts are counted in: from October 1 of its first year.
    let year = scout_year(&today);
    Ok(Json(json!({
        "tipo": tipo,
        "yearLabel": year_label(year),
        "yearStart": format!("{year}-10-01"),
        "subunidades": subunidades,
        "scouts": scouts,
        "adults": adults,
        "criteriaDefs": criteria::definitions(),
        "criteria": criteria::evaluate(&members, &spans, &today).swap_remove(i),
    })))
}

/// An adult tied to a secção by an active cargo there (chefe, adjunto, or one whose descrição
/// names the secção, e.g. an instrutor's: see member_reports::cargo_seccao).
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Adult {
    #[serde(skip)]
    nin: String,
    #[sqlx(skip)]
    #[serde(rename = "ref")]
    member_ref: String,
    #[serde(skip)]
    birth_month: Option<String>,
    #[sqlx(skip)]
    age: Option<i32>,
    totem: String,
    /// Dirigente, Aspirante a Dirigente, Noviço a Dirigente...
    categoria: Option<String>,
    /// Their cargos in the secção, e.g. "Chefe de Alcateia".
    cargos: String,
    /// Instrutor of the secção (sigla I): the ones counted per sub-unidade. Chefes and chefes
    /// adjuntos lead the whole unidade, so they aren't.
    instrutor: bool,
}

/// The active adults holding an active cargo of secção `tipo` in the agrupamento.
async fn seccao_adults(db: &sqlx::PgPool, code: i16, tipo: i16) -> Result<Vec<Adult>, sqlx::Error> {
    sqlx::query_as(
        "select m.nin, m.birth_month::text as birth_month, m.totem, m.categoria,
                string_agg(distinct c.cargo, ', ') as cargos,
                bool_or(c.sigla_cargo = 'I') as instrutor
         from member_cargos c join members m on m.nin = c.nin
         where coalesce(c.agrupamento, c.import_agrupamento) = $1 and c.active
           and c.seccao_tipo = $2
           and m.present and m.situacao like 'Ativo%'
         group by m.nin, m.birth_month, m.totem, m.categoria
         order by m.totem",
    )
    .bind(code)
    .bind(tipo)
    .fetch_all(db)
    .await
}

/// Sub-unidades (bandos, patrulhas, equipas, tribos) have at most 8 members and at least 4 (5 in
/// the Alcateia), and agrupamentos aim for about 6 each. A secção has at most 5 of them.
const SUBUNIDADE_MAX: u32 = 8;
const SUBUNIDADES_MAX: u32 = 5;
const SUBUNIDADE_TARGET: f64 = 6.0;
pub(crate) fn subunidade_min(tipo: i16) -> u32 {
    if tipo == 1 { 5 } else { 4 }
}

/// The sub-unidades a secção of `members` most likely has, as their sizes, largest first.
/// - Possible counts: those whose even split fits min..=max (n from ceil(m/max) to floor(m/min)),
///   and no more than 5.
/// - Of those, the one whose average size is closest to 6; a tie goes to fewer, larger ones.
///   12 = 2 x 6, not 3 x 4; 15 = 3 x 5; 17 = 6 + 6 + 5.
/// - None fits (under the minimum, or e.g. 9 lobitos: 1 x 9 is too big, 5 + 4 too small): as
///   few as stay under the maximum, so one incomplete sub-unidade for a small secção. Past 40
///   members, 5 sub-unidades over the maximum size.
pub fn estimate_subunidades(members: u32, min: u32) -> Vec<u32> {
    if members == 0 {
        return Vec::new();
    }
    let distance = |n: u32| (f64::from(members) / f64::from(n) - SUBUNIDADE_TARGET).abs();
    let n = (members.div_ceil(SUBUNIDADE_MAX)..=(members / min).min(SUBUNIDADES_MAX))
        .filter(|n| *n > 0)
        // min_by keeps the first of equals: the smaller count.
        .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
        .unwrap_or_else(|| members.div_ceil(SUBUNIDADE_MAX).clamp(1, SUBUNIDADES_MAX));
    // Even split: the first `extra` sub-unidades get one more.
    let (base, extra) = (members / n, members % n);
    (0..n).map(|k| base + u32::from(k < extra)).collect()
}

/// The secção's sub-unidades as SIIE records them, as their sizes.
/// TODO(siie): no SIIE report of sub-unidades has been found yet, and some agrupamentos may not
/// fill it either. When one is, import it (a column per member) and read it here; until then this
/// is None and `estimate_subunidades` gives the count.
async fn siie_subunidades(
    _db: &sqlx::PgPool,
    _code: i16,
    _tipo: i16,
) -> Result<Option<Vec<u32>>, sqlx::Error> {
    Ok(None)
}

/// An opaque, stable id for a member in URLs: HMAC of the NIN with the server's key, cut to 12
/// bytes. The NIN never leaves the server, and the id can't be turned back into one without the
/// key.
fn member_ref(secret: &[u8], nin: &str) -> String {
    hex::encode(&keyed_hash(secret, &format!("member-ref:{nin}"))[..12])
}

/// Whole years from a `YYYY-MM-01` birth month to `today`, counting the birthday month as reached.
fn age(birth_month: Option<&str>, today: &str) -> Option<i32> {
    let b = birth_month?;
    let (by, bm): (i32, u32) = (b.get(..4)?.parse().ok()?, b.get(5..7)?.parse().ok()?);
    let (ty, tm): (i32, u32) = (
        today.get(..4)?.parse().ok()?,
        today.get(5..7)?.parse().ok()?,
    );
    Some(ty - by - i32::from(tm < bm))
}

/// GET /api/agrupamentos/{code}/elementos/{ref}: one member's history, year by year: their secção
/// (and whether age placed them), progresso, promessa, especialidades, cargos, cursos and
/// atividades. Same access rule as the digest; the member must be in this agrupamento.
pub async fn member(
    State(s): State<AppState>,
    acc: CurrentAccount,
    Path((code, member_id)): Path<(i16, String)>,
) -> Result<Json<Value>, ApiError> {
    let db = &s.db;
    granted(db, acc.id, code)
        .await
        .map_err(db_error)?
        .ok_or_else(not_whole)?;
    let today = today(db).await?;
    let secret = s.login_key_secret.expose_secret();
    let members = load(db, &[code]).await.map_err(db_error)?;
    let m = members
        .iter()
        .find(|m| member_ref(secret, &m.nin) == member_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Elemento não encontrado."))?;

    // Column names come from SECCOES / STEPS, never from the request.
    let steps: Vec<String> = SECCOES
        .iter()
        .map(|(_, _, prefix, promessa)| {
            let cols: Vec<String> = STEPS.iter().map(|c| format!("p.{prefix}{c}")).collect();
            format!("array[{}, p.{promessa}]::text[]", cols.join(", "))
        })
        .collect();
    // ano escutista of a date: October onwards starts a new one (as `scout_year`).
    let year_of = |col: &str| {
        format!(
            "(extract(year from {col}) - case when extract(month from {col}) >= 10 then 0 else 1 end)::int"
        )
    };
    let (info,): (Value,) = sqlx::query_as(&format!(
        "select json_build_object(
            'totem', m.totem, 'categoria', m.categoria, 'seccao', m.seccao,
            'situacao', m.situacao, 'present', m.present,
            'admittedOn', m.admitted_on, 'promisedOn', m.promised_on,
            -- per secção (SECCOES order): the STEPS dates, then the promessa
            'progresso', json_build_array({steps}),
            'especialidades', coalesce((
                select json_agg(json_build_object('tipo', e.seccao_tipo, 'name', e.especialidade,
                                                  'on', e.achieved_on)
                                order by e.achieved_on nulls last, e.especialidade)
                from member_especialidades e where e.nin = m.nin), '[]'),
            'cargos', coalesce((
                select json_agg(json_build_object('cargo', c.cargo, 'seccaoTipo', c.seccao_tipo,
                                                  'agrupamento', coalesce(c.agrupamento, c.import_agrupamento),
                                                  'startedOn', c.started_on, 'endedOn', c.ended_on,
                                                  'active', c.active)
                                order by c.started_on nulls last)
                from member_cargos c where c.nin = m.nin), '[]'),
            'cursos', coalesce((
                select json_agg(json_build_object('curso', c.curso, 'on', c.completed_on)
                                order by c.completed_on nulls last)
                from member_cursos c where c.nin = m.nin), '[]'),
            'atividades', coalesce((
                select json_agg(a order by a.year)
                from (select {year} as year, count(*) as count,
                             coalesce(sum(noites), 0) as noites,
                             coalesce(sum(noites_campo), 0) as \"noitesCampo\"
                      from member_atividades where nin = m.nin and started_on is not null
                      group by 1) a), '[]'))
         from members m left join member_progress p on p.nin = m.nin
         where m.nin = $1",
        steps = steps.join(", "),
        year = year_of("started_on"),
    ))
    .bind(&m.nin)
    .fetch_one(db)
    .await
    .map_err(db_error)?;

    // The secção they were counted in each year, as in the digest.
    let years: Vec<Value> = member_years(m, &today)
        .into_iter()
        .map(|(y, b, by_age)| json!({"year": y, "label": year_label(y), "bucket": b, "byAge": by_age}))
        .collect();
    Ok(Json(json!({
        "ref": member_id,
        "age": age(m.birth_month.as_deref(), &today),
        "info": info,
        "years": years,
    })))
}

/// The agrupamento's own data, as shown on top of the digest (from the agrupamentos list,
/// see member_reports::AGRUPAMENTO_COLUMNS). Dates are `YYYY-MM-DD`.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Agrupamento {
    code: i16,
    name: Option<String>,
    nucleo: Option<i16>,
    nucleo_name: Option<String>,
    regiao: Option<i16>,
    regiao_name: Option<String>,
    estado: Option<String>,
    tipo: Option<String>,
    paroquia: Option<String>,
    patrono: Option<String>,
    email: Option<String>,
    emailnotif: Option<String>,
    telefone: Option<String>,
    site: Option<String>,
    cp4: Option<String>,
    localidade: Option<String>,
    iniciado_on: Option<String>,
    filiado_on: Option<String>,
}

/// The agrupamento's own data, if the account holds it whole.
async fn granted(
    db: &sqlx::PgPool,
    account: Uuid,
    code: i16,
) -> Result<Option<Agrupamento>, sqlx::Error> {
    sqlx::query_as(
        "select a.code, a.name, a.nucleo, n.name as nucleo_name, a.regiao,
                r.name as regiao_name, a.estado, a.tipo, a.paroquia, a.patrono, a.email,
                a.emailnotif, a.telefone, a.site, a.cp4, a.localidade,
                a.iniciado_on::text as iniciado_on, a.filiado_on::text as filiado_on
         from account_grants g join agrupamentos a on a.code = g.agrupamento
         left join nucleos n on n.code = a.nucleo
         left join regioes r on r.code = a.regiao
         where g.account_id = $1 and g.agrupamento = $2 and g.seccao_tipo is null",
    )
    .bind(account)
    .bind(code)
    .fetch_optional(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-10-21";

    fn member() -> MemberDates {
        MemberDates {
            first_seen: "2026-10-20".into(),
            active: true,
            first_steps: vec![None; 4],
            promessas: vec![None; 4],
            ..Default::default()
        }
    }

    fn d(s: &str) -> Option<String> {
        Some(s.into())
    }

    /// One value per secção, in SECCOES order (Lobitos, Exploradores, Pioneiros, Caminheiros).
    fn per_seccao(
        l: Option<&str>,
        e: Option<&str>,
        p: Option<&str>,
        c: Option<&str>,
    ) -> Vec<Option<String>> {
        [l, e, p, c]
            .into_iter()
            .map(|x| x.map(String::from))
            .collect()
    }

    /// member_years without the by-age flag.
    fn years(m: &MemberDates) -> Vec<(i32, Bucket)> {
        member_years(m, TODAY)
            .into_iter()
            .map(|(y, b, _)| (y, b))
            .collect()
    }

    #[test]
    fn stage_is_the_latest_dated_step() {
        // As SIIE counts them: the step of the latest date. Nothing dated: in Adesão too.
        assert_eq!(stage(&[None, None, None, None, None]), 0);
        assert_eq!(stage(&[d("2024-10-01"), None, None, None, None]), 0);
        assert_eq!(
            stage(&[d("2024-10-01"), d("2025-03-01"), None, None, None]),
            1
        );
        // A later step without an earlier one still counts as reached.
        assert_eq!(stage(&[None, None, d("2025-06-01"), None, None]), 2);
        assert_eq!(stage(&[None, None, None, None, d("2025-06-01")]), 4);
    }

    #[test]
    fn current_members_are_active_and_in_the_seccao_now() {
        let m = |nin: &str, seccao: &str, active: bool| MemberDates {
            nin: nin.into(),
            admitted_on: d("2024-10-01"),
            seccao: d(seccao),
            active,
            ..member()
        };
        let all = [
            m("A", "Lobitos", true),
            m("B", "Exploradores", true),
            m("C", "Lobitos", false),
        ];
        assert_eq!(current_members(&all, Bucket::Lobitos, TODAY), vec!["A"]);
    }

    #[test]
    fn subunidades_aim_at_six_within_the_limits() {
        assert_eq!(estimate_subunidades(12, 4), vec![6, 6]);
        assert_eq!(estimate_subunidades(15, 4), vec![5, 5, 5]);
        assert_eq!(estimate_subunidades(17, 4), vec![6, 6, 5]);
        assert_eq!(estimate_subunidades(8, 4), vec![8]);
        // At most 5: 40 lobitos are 5 x 8, not 7 bandos; past 40 they grow over 8.
        assert_eq!(estimate_subunidades(40, 5), vec![8, 8, 8, 8, 8]);
        assert_eq!(estimate_subunidades(30, 4), vec![6, 6, 6, 6, 6]);
        assert_eq!(estimate_subunidades(52, 4), vec![11, 11, 10, 10, 10]);
        assert_eq!(estimate_subunidades(10, 5), vec![5, 5]);
        // Too few for one, or no split that fits: as few as stay under 8.
        assert_eq!(estimate_subunidades(3, 5), vec![3]);
        assert_eq!(estimate_subunidades(9, 5), vec![5, 4]);
        assert!(estimate_subunidades(0, 4).is_empty());
        // Every fitting answer stays within 4..=8.
        for m in 1..=80 {
            let sizes = estimate_subunidades(m, 4);
            assert_eq!(sizes.iter().sum::<u32>(), m);
            assert!(sizes.len() <= 5, "{m}: {sizes:?}");
            // From 8 to 40 members, every sub-unidade fits 4..=8.
            if (8..=40).contains(&m) {
                assert!(sizes.iter().all(|s| (4..=8).contains(s)), "{m}: {sizes:?}");
            }
        }
    }

    #[test]
    fn age_counts_the_birthday_month() {
        assert_eq!(age(d("2016-10-01").as_deref(), "2026-10-21"), Some(10));
        assert_eq!(age(d("2016-11-01").as_deref(), "2026-10-21"), Some(9));
        assert_eq!(age(None, "2026-10-21"), None);
    }

    #[test]
    fn member_ref_is_stable_and_hides_the_nin() {
        let r = member_ref(&[1; 32], "1234567890123");
        assert_eq!(r, member_ref(&[1; 32], "1234567890123"));
        assert_ne!(r, member_ref(&[2; 32], "1234567890123"));
        assert_eq!(r.len(), 24);
        assert!(!r.contains("1234567890123"));
    }

    #[test]
    fn scout_year_starts_in_october() {
        assert_eq!(scout_year("2024-09-30"), 2023);
        assert_eq!(scout_year("2024-10-01"), 2024);
        assert_eq!(scout_year("2025-01-15"), 2024);
    }

    #[test]
    fn moving_up_mid_year_counts_in_the_new_seccao() {
        // Lobito from 2022/23, exploradora from March 2025 (2024/25), now an exploradora.
        let m = MemberDates {
            admitted_on: d("2022-11-05"),
            first_steps: per_seccao(Some("2022-11-05"), Some("2025-03-10"), None, None),
            seccao: d("Exploradores"),
            ..member()
        };
        assert_eq!(
            years(&m),
            vec![
                (2022, Bucket::Lobitos),
                (2023, Bucket::Lobitos),
                (2024, Bucket::Exploradores),
                (2025, Bucket::Exploradores),
                (2026, Bucket::Exploradores),
            ]
        );
    }

    #[test]
    fn former_members_end_at_their_last_censo_or_last_record() {
        let left = MemberDates {
            admitted_on: d("2015-10-10"),
            first_steps: per_seccao(Some("2015-10-10"), None, None, None),
            ano_saida: Some(2018),
            active: false,
            ..member()
        };
        assert_eq!(
            years(&left).iter().map(|y| y.0).collect::<Vec<_>>(),
            vec![2015, 2016, 2017]
        );
        // The last censo wins: censo 2016 = 2016/17 is the last year.
        let censo = MemberDates {
            ultimoano_censos: Some(2016),
            ..left.clone()
        };
        assert_eq!(years(&censo).last().map(|y| y.0), Some(2016));
        let inactive = MemberDates {
            admitted_on: d("2019-10-10"),
            last_record: d("2021-05-01"),
            active: false,
            ..member()
        };
        assert_eq!(
            years(&inactive),
            vec![(2019, Bucket::SemRegisto), (2020, Bucket::SemRegisto)]
        );
    }

    #[test]
    fn former_members_with_no_dates_are_left_out() {
        // Old SIIE records: inactive, no admission, adesão, promessa or cargo. Before, these
        // landed in the current year as Sem registo.
        let undated = MemberDates {
            active: false,
            ..member()
        };
        assert!(start_date(&undated).is_none());
        assert!(member_years(&undated, TODAY).is_empty());
        // A cargo date is enough to place them.
        let cargo = MemberDates {
            first_record: d("2010-11-01"),
            last_record: d("2012-06-01"),
            dirigente_since: d("2010-11-01"),
            ..undated.clone()
        };
        assert_eq!(
            years(&cargo),
            vec![(2010, Bucket::Dirigentes), (2011, Bucket::Dirigentes)]
        );
        // An active member with no dates is here now: this year only.
        let active = MemberDates {
            seccao: d("Lobitos"),
            ..member()
        };
        assert_eq!(years(&active), vec![(2026, Bucket::Lobitos)]);
    }

    #[test]
    fn dirigentes_from_promessa_or_cargo() {
        let m = MemberDates {
            admitted_on: d("2020-10-01"),
            first_steps: per_seccao(None, None, None, Some("2020-10-01")),
            dirigente_since: d("2023-06-01"),
            seccao: d("Caminheiros"),
            ..member()
        };
        let years = years(&m);
        assert_eq!(years[1], (2021, Bucket::Caminheiros));
        assert_eq!(years[2], (2022, Bucket::Dirigentes));
        assert_eq!(years.last(), Some(&(2026, Bucket::Dirigentes)));
    }

    #[test]
    fn no_dates_and_no_age_is_sem_registo_except_this_year() {
        let m = MemberDates {
            admitted_on: d("2024-10-01"),
            seccao: d("Pioneiros"),
            ..member()
        };
        assert_eq!(
            years(&m),
            vec![
                (2024, Bucket::SemRegisto),
                (2025, Bucket::SemRegisto),
                (2026, Bucket::Pioneiros)
            ]
        );
        assert_eq!(
            current_bucket(d("Lobitos").as_deref(), d("Dirigente").as_deref()),
            Some(Bucket::Dirigentes)
        );
    }

    #[test]
    fn without_records_age_decides_and_is_flagged() {
        // Born 2016, admitted 2022 with no progresso at all: lobito by age (6-10), flagged.
        let m = MemberDates {
            admitted_on: d("2022-10-15"),
            birth_month: d("2016-03-01"),
            seccao: d("Lobitos"),
            ..member()
        };
        assert_eq!(
            member_years(&m, TODAY),
            vec![
                (2022, Bucket::Lobitos, true),
                (2023, Bucket::Lobitos, true),
                (2024, Bucket::Lobitos, true),
                (2025, Bucket::Lobitos, true),
                // This year SIIE says so: not an estimate.
                (2026, Bucket::Lobitos, false),
            ]
        );
    }

    #[test]
    fn records_beat_age_and_a_stale_adesao_yields_to_it() {
        // Born 2012. Lobito adesão in 2019, nothing after, and not active: by 2024 (age 12) the
        // old lobito adesão is out of band, so age says explorador.
        let m = MemberDates {
            admitted_on: d("2019-10-10"),
            birth_month: d("2012-05-01"),
            first_steps: per_seccao(Some("2019-10-10"), None, None, None),
            ldates: vec!["2019-10-10".into()],
            ultimoano_censos: Some(2024),
            active: false,
            ..member()
        };
        let y = member_years(&m, TODAY);
        assert_eq!(y[0], (2019, Bucket::Lobitos, false));
        assert_eq!(y.last(), Some(&(2024, Bucket::Exploradores, true)));
        // A pioneiro especialidade dated in 2023/24 beats the age band (11-14: explorador).
        let esp = MemberDates {
            pdates: vec!["2024-05-01".into()],
            ..m.clone()
        };
        assert_eq!(
            member_years(&esp, TODAY)[4],
            (2023, Bucket::Pioneiros, false)
        );
    }

    #[test]
    fn digest_fills_empty_years_and_seccoes_add_up() {
        let old = MemberDates {
            admitted_on: d("2018-10-01"),
            first_steps: per_seccao(Some("2018-10-01"), None, None, None),
            ano_saida: Some(2020),
            active: false,
            ..member()
        };
        let new = MemberDates {
            admitted_on: d("2024-10-01"),
            birth_month: d("2017-01-01"),
            seccao: d("Lobitos"),
            ..member()
        };
        let rows = digest(&[old, new], TODAY);
        assert_eq!(rows.first().map(|r| r.label.as_str()), Some("2018/19"));
        assert_eq!(rows.last().map(|r| r.label.as_str()), Some("2026/27"));
        assert_eq!(
            rows.iter().map(|r| r.total).collect::<Vec<_>>(),
            [1, 1, 0, 0, 0, 0, 1, 1, 1]
        );
        // The new lobito was placed by age in 2024/25 and 2025/26, by SIIE this year.
        let by_age: Vec<u32> = rows.iter().map(|r| r.by_age.lobitos).collect();
        assert_eq!(by_age, [0, 0, 0, 0, 0, 0, 1, 1, 0]);
        for r in &rows {
            let sum = r.lobitos
                + r.exploradores
                + r.pioneiros
                + r.caminheiros
                + r.dirigentes
                + r.sem_registo;
            assert_eq!(sum, r.total, "{}", r.label);
        }
        assert!(digest(&[], TODAY).is_empty());
    }
}

/// Database test: run against DATABASE_URL with `cargo test -- --ignored`. Uses agrupamentos
/// 9201 (whole grant) and 9202 (secção grant), NINs starting with TEST92, and cleans up after.
#[cfg(test)]
mod db_tests {
    use super::*;

    async fn cleanup(db: &sqlx::PgPool) {
        for sql in [
            "delete from members where nin like 'TEST92%'",
            "delete from accounts where label = 'digest db test'",
            "delete from seccoes where agrupamento in (9201, 9202)",
            "delete from agrupamentos where code in (9201, 9202)",
        ] {
            sqlx::query(sql).execute(db).await.unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn digest_counts_and_grants() {
        dotenvy::dotenv().ok();
        let state = AppState::from_env(false).expect("DATABASE_URL and LOGIN_KEY_SECRET");
        let db = &state.db;
        cleanup(db).await;
        let (id,): (Uuid,) = sqlx::query_as(
            "insert into accounts (nif_key, label) values ('\\x03', 'digest db test') returning id",
        )
        .fetch_one(db)
        .await
        .unwrap();
        for sql in [
            "insert into agrupamentos (code, name) values (9201, 'Teste 9201'), (9202, 'Teste 9202')",
            "insert into seccoes (agrupamento, tipo) values (9202, 1)",
            "insert into members (nin, totem, totem_from_siie, agrupamento, situacao, seccao,
                                  categoria, admitted_on, birth_month, present)
             values ('TEST92A', 'A', false, 9201, 'Ativo', 'Exploradores', 'Explorador',
                     '2022-11-05', '2014-02-01', true),
                    ('TEST92B', 'B', false, 9201, 'Inativo', null, null, '2021-10-10', null,
                     false)",
            "insert into member_progress (nin, ladesao, eadesao) values
                ('TEST92A', '2022-11-05', '2025-03-10')",
            "insert into member_progress (nin, ladesao, ano_saida) values
                ('TEST92B', '2021-10-10', 2023)",
            "insert into members (nin, totem, totem_from_siie, agrupamento, situacao, seccao,
                                  categoria, admitted_on, birth_month, present)
             values ('TEST92C', 'C', false, 9201, 'Ativo', 'Dirigentes', 'Dirigente',
                     '2026-10-01', '1990-03-01', true)",
            "insert into member_cargos (siie_id, nin, agrupamento, started_on)
             values (-9201, 'TEST92B', 9201, '2022-10-15')",
            "insert into member_cargos (siie_id, nin, cargo, sigla_cargo, seccao_tipo, agrupamento,
                                        started_on, active)
             values (-9202, 'TEST92C', 'Chefe de Expedição', 'CGE', 2, 9201, '2026-10-01', true)",
            // An instrutor whose cargo has no agrup in SIIE: it counts under the agrupamento it's
            // listed under.
            "insert into members (nin, totem, totem_from_siie, agrupamento, situacao, seccao,
                                  categoria, admitted_on, present)
             values ('TEST92D', 'D', false, 9201, 'Ativo', 'Dirigentes', 'Dirigente',
                     '2026-10-01', true)",
            "insert into member_cargos (siie_id, nin, cargo, sigla_cargo, seccao_tipo, agrupamento,
                                        import_agrupamento, started_on, active)
             values (-9203, 'TEST92D', 'Instrutor', 'I', 2, null, 9201, '2026-10-01', true)",
            "insert into member_especialidades (nin, seccao_tipo, especialidade, achieved_on)
             values ('TEST92A', 2, 'Nadador', '2025-06-01'), ('TEST92A', 1, 'Lobito X', null)",
        ] {
            sqlx::query(sql).execute(db).await.unwrap();
        }
        sqlx::query(
            "insert into account_grants (account_id, agrupamento, seccao_tipo, seccao_numero)
             values ($1, 9201, null, null), ($1, 9202, 1, 1)",
        )
        .bind(id)
        .execute(db)
        .await
        .unwrap();

        // Only the whole grant gets a digest, with the data an import wrote.
        let mut tx = db.begin().await.unwrap();
        let data = crate::member_reports::parse_agrupamento(
            json!({"agrupamento": "9201", "descricao": "Nome SIIE", "estado": "Ativo",
                "tipo": "Terrestre", "paroquia": "P", "patrono": "S", "email": "e@x.pt",
                "emailnotif": null, "telefone": null, "site": null, "cp1": "4800",
                "localidade": "Guimarães", "dtiniciado": null, "osnfiliado": "1, 15-03-1985",
                "nucleo": "0000", "nucnome": null, "regiao": "00", "regnome": null})
            .as_object()
            .unwrap(),
        )
        .unwrap();
        crate::member_reports::write_agrupamento(&mut tx, 9201, &data)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let a = granted(db, id, 9201).await.unwrap().unwrap();
        assert_eq!(a.name.as_deref(), Some("Nome SIIE"));
        assert_eq!(a.filiado_on.as_deref(), Some("1985-03-15"));
        assert_eq!(a.cp4.as_deref(), Some("4800"));
        assert!(granted(db, id, 9202).await.unwrap().is_none());

        let members = load(db, &[9201]).await.unwrap();
        let a = members.iter().find(|m| m.nin == "TEST92A").unwrap();
        // The record arrays: adesão in each secção, nulls dropped, especialidades included.
        assert_eq!(
            a.first_steps,
            vec![
                Some("2022-11-05".into()),
                Some("2025-03-10".into()),
                None,
                None
            ]
        );
        assert_eq!(a.edates, vec!["2025-03-10", "2025-06-01"]);
        assert_eq!(a.ldates, vec!["2022-11-05"]);
        let rows = digest(&members, "2026-10-21");
        // B: lobito 2021/22, dirigente (cargo) 2022/23, left in 2023. A: lobito from 2022/23.
        let r = |label: &str| rows.iter().find(|r| r.label == label).cloned().unwrap();
        assert_eq!(rows[0].label, "2021/22");
        assert_eq!((r("2021/22").total, r("2021/22").lobitos), (1, 1));
        assert_eq!(
            (
                r("2022/23").total,
                r("2022/23").lobitos,
                r("2022/23").dirigentes
            ),
            (2, 1, 1)
        );
        assert_eq!((r("2024/25").total, r("2024/25").exploradores), (1, 1));

        // The secção view: A is the one current explorador, at adesão, with one especialidade.
        let acc = || CurrentAccount {
            id,
            label: String::new(),
            role: "member".into(),
        };
        let Json(v) = seccao(State(state.clone()), acc(), Path((9201, 2)))
            .await
            .unwrap_or_else(|e| panic!("{}", e.1));
        assert_eq!(v["scouts"].as_array().unwrap().len(), 1);
        let a = &v["scouts"][0];
        assert_eq!(
            (a["totem"].as_str(), a["stage"].as_u64()),
            (Some("A"), Some(0))
        );
        assert_eq!(a["steps"][0], "2025-03-10");
        assert_eq!(
            a["especialidades"],
            json!([{"name": "Nadador", "on": "2025-06-01"}])
        );
        // C leads the expedição and D is its instrutor: both on top, with their cargos.
        assert_eq!(v["adults"].as_array().unwrap().len(), 2);
        assert_eq!(v["adults"][1]["totem"], "D");
        assert_eq!(v["adults"][1]["instrutor"], true);
        assert_eq!(
            (
                v["adults"][0]["totem"].as_str(),
                v["adults"][0]["cargos"].as_str()
            ),
            (Some("C"), Some("Chefe de Expedição"))
        );
        // A chefe isn't an instrutor; one explorador makes one (incomplete) sub-unidade.
        assert_eq!(v["adults"][0]["instrutor"], false);
        assert_eq!(v["subunidades"], json!({"sizes": [1], "estimated": true}));
        assert!(!v.to_string().contains("TEST92"), "no NIN in the answer");

        // A's page, by the ref from the secção view.
        let a_ref = v["scouts"][0]["ref"].as_str().unwrap().to_owned();
        let Json(page) = member(State(state.clone()), acc(), Path((9201, a_ref)))
            .await
            .unwrap_or_else(|e| panic!("{}", e.1));
        assert_eq!(page["info"]["totem"], "A");
        assert_eq!(page["info"]["progresso"][1][0], "2025-03-10");
        assert_eq!(page["info"]["especialidades"][0]["name"], "Nadador");
        assert_eq!(page["years"][2]["bucket"], "exploradores");
        let missing = member(State(state.clone()), acc(), Path((9201, "00".into()))).await;
        assert_eq!(missing.err().map(|e| e.0), Some(StatusCode::NOT_FOUND));

        // Adesão concluded, explorador past aspirante with no promessa: flagged this year.
        assert_eq!(
            v["criteria"].as_array().unwrap().last().unwrap()["flags"]["promessa"],
            1
        );

        // The comparison lists only the whole grant.
        let Json(list) = quality_list(State(state.clone()), acc())
            .await
            .unwrap_or_else(|e| panic!("{}", e.1));
        assert_eq!(
            list["defs"].as_array().unwrap().len(),
            criteria::CRITERIA.len()
        );
        let rows = list["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0]["code"].as_i64(), rows[0]["members"].as_u64()),
            (Some(9201), Some(1))
        );
        // One explorador this year, past aspirante with no promessa date (red, 3) and below the
        // minimum (red, 3). D instructs its one sub-unidade, so no instrutores flag.
        assert_eq!(rows[0]["points"], 6);
        assert_eq!(
            rows[0]["failed"],
            json!([{"tipo": 2, "key": "promessa"}, {"tipo": 2, "key": "minimo"}])
        );

        cleanup(db).await;
    }
}
