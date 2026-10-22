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

use crate::{
    AppState,
    auth::{ApiError, CurrentAccount, db_error},
};

/// Where a member is counted in one year.
#[derive(Debug, Clone, Copy, PartialEq)]
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
const SECCOES: [(i16, Bucket, &str, &str); 4] = [
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
    /// Adesão to each secção, in SECCOES order.
    pub adesoes: Vec<Option<String>>,
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

/// Progresso quality of one secção this ano escutista.
#[derive(Serialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Quality {
    pub tipo: i16,
    /// Active members in the secção now.
    pub members: u32,
    /// Of those, how many have the secção's adesão registered.
    pub with_adesao: u32,
    /// Of those, how many are past aspirante (so should have made the promessa)...
    pub promessa_due: u32,
    /// ...and how many of these have a promessa registered, in any secção.
    pub with_promessa: u32,
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
    // 4. The secção of the latest adesão by the year's end, unless their age is out of that
    //    secção's band by more than a year (they moved on, and the next adesão wasn't recorded).
    let age = m
        .birth_month
        .as_deref()
        .and_then(|b| b.get(..4)?.parse::<i32>().ok())
        .map(|born| year - born);
    let adesao = SECCOES
        .iter()
        .zip(&m.adesoes)
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
            label: format!("{year}/{:02}", (year + 1) % 100),
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

/// Progresso quality this ano escutista, one per secção in SECCOES order: of the active members
/// in it now, how many have its adesão registered, and of those past aspirante, a promessa. The
/// promessa is made once, in the secção the member joined (an explorador who promised as a lobito
/// only has the Lobitos date; SIIE's promessa dates match dados completos' Dt. Promessa, checked
/// 2026-10-22), so any secção's promessa counts.
pub fn quality(members: &[MemberDates], today: &str) -> Vec<Quality> {
    let current = scout_year(today);
    SECCOES
        .iter()
        .enumerate()
        .map(|(i, &(tipo, b, ..))| {
            let mut q = Quality {
                tipo,
                ..Default::default()
            };
            for m in members
                .iter()
                .filter(|m| m.active && bucket(m, current, true).0 == b)
            {
                let has = |v: &[Option<String>]| v.get(i).is_some_and(Option::is_some);
                q.members += 1;
                q.with_adesao += u32::from(has(&m.adesoes));
                // Aspirantes haven't made the promessa yet.
                let aspirante = m
                    .categoria
                    .as_deref()
                    .is_some_and(|c| c.to_lowercase().contains("aspirante"));
                if !aspirante {
                    q.promessa_due += 1;
                    q.with_promessa += u32::from(m.promessas.iter().any(Option::is_some));
                }
            }
            q
        })
        .collect()
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
                array[p.ladesao, p.eadesao, p.padesao, p.cadesao]::text[] as adesoes,
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
             from member_cargos where nin = m.nin and agrupamento = m.agrupamento
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
/// progresso quality this year, and the agrupamento's own data. Needs a whole-agrupamento grant:
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
        "quality": quality(&members, &today),
        "undated": undated,
        "lastImportAt": last_import_at,
    })))
}

/// GET /api/agrupamentos/quality: this year's progresso quality of every agrupamento the account
/// holds whole, all secções together, to compare them.
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
    let rows: Vec<Value> = agrupamentos
        .into_iter()
        .map(|(code, name)| {
            let q = quality(by_code.get(&code).map_or(&[][..], Vec::as_slice), &today);
            let sum = |f: fn(&Quality) -> u32| q.iter().map(f).sum::<u32>();
            json!({
                "code": code,
                "name": name,
                "members": sum(|q| q.members),
                "withAdesao": sum(|q| q.with_adesao),
                "promessaDue": sum(|q| q.promessa_due),
                "withPromessa": sum(|q| q.with_promessa),
            })
        })
        .collect();
    Ok(Json(Value::Array(rows)))
}

/// One scout of a secção, as the secção view shows them.
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Scout {
    totem: String,
    categoria: Option<String>,
    /// The date of each of STEPS in this secção (`YYYY-MM-DD`), null if not reached.
    steps: Vec<Option<String>>,
    /// How far they got: 0 = no adesão yet, n = STEPS[n - 1] is the furthest step reached.
    #[sqlx(skip)]
    stage: usize,
    promessa_on: Option<String>,
    /// [{name, on}] earned in this secção, newest first.
    especialidades: sqlx::types::Json<Value>,
}

/// The furthest step reached, by order and not by date (SIIE may hold a later step without an
/// earlier one).
fn stage(steps: &[Option<String>]) -> usize {
    steps.iter().rposition(Option::is_some).map_or(0, |i| i + 1)
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
    let nins = current_members(&members, target, &today);

    // Column names come from SECCOES / STEPS, never from the request.
    let steps: Vec<String> = STEPS
        .iter()
        .map(|c| format!("p.{prefix}{c}::text"))
        .collect();
    let mut scouts: Vec<Scout> = sqlx::query_as(&format!(
        "select m.totem, m.categoria, array[{}] as steps, p.{promessa}::text as promessa_on,
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
    for sc in &mut scouts {
        sc.stage = stage(&sc.steps);
    }

    // The ano escutista these scouts are counted in: from October 1 of its first year.
    let year = scout_year(&today);
    Ok(Json(json!({
        "tipo": tipo,
        "yearLabel": format!("{year}/{:02}", (year + 1) % 100),
        "yearStart": format!("{year}-10-01"),
        "scouts": scouts,
        "quality": quality(&members, &today)[i],
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
            adesoes: vec![None; 4],
            promessas: vec![None; 4],
            ..Default::default()
        }
    }

    fn d(s: &str) -> Option<String> {
        Some(s.into())
    }

    /// Adesões in SECCOES order (Lobitos, Exploradores, Pioneiros, Caminheiros).
    fn adesoes(
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
    fn stage_is_the_furthest_step_reached() {
        assert_eq!(stage(&[None, None, None, None, None]), 0);
        assert_eq!(stage(&[d("2024-10-01"), None, None, None, None]), 1);
        // A later step without an earlier one still counts as reached.
        assert_eq!(
            stage(&[d("2024-10-01"), None, d("2025-06-01"), None, None]),
            3
        );
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
            adesoes: adesoes(Some("2022-11-05"), Some("2025-03-10"), None, None),
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
            adesoes: adesoes(Some("2015-10-10"), None, None, None),
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
            adesoes: adesoes(None, None, None, Some("2020-10-01")),
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
            adesoes: adesoes(Some("2019-10-10"), None, None, None),
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
            adesoes: adesoes(Some("2018-10-01"), None, None, None),
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

    #[test]
    fn quality_counts_adesao_and_promessa_per_seccao() {
        let lobito = |adesao: Option<&str>, promessa: Option<&str>, categoria: &str| MemberDates {
            admitted_on: d("2024-10-01"),
            seccao: d("Lobitos"),
            categoria: d(categoria),
            adesoes: adesoes(adesao, None, None, None),
            promessas: adesoes(promessa, None, None, None),
            ..member()
        };
        let all = [
            lobito(Some("2024-10-01"), Some("2025-05-01"), "Lobito"),
            lobito(None, None, "Lobito"),
            // Aspirantes aren't due a promessa.
            lobito(Some("2026-10-05"), None, "Aspirante a Lobito"),
        ];
        let q = &quality(&all, TODAY)[0];
        assert_eq!(
            (q.members, q.with_adesao, q.promessa_due, q.with_promessa),
            (3, 2, 2, 1)
        );
        assert_eq!(quality(&all, TODAY)[1].members, 0);
        // An explorador who made the promessa as a lobito has made it.
        let explorador = MemberDates {
            admitted_on: d("2022-10-01"),
            seccao: d("Exploradores"),
            categoria: d("Explorador"),
            promessas: adesoes(Some("2023-05-01"), None, None, None),
            ..member()
        };
        let q = &quality(&[explorador], TODAY)[1];
        assert_eq!((q.promessa_due, q.with_promessa), (1, 1));
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
            "insert into member_cargos (siie_id, nin, agrupamento, started_on)
             values (-9201, 'TEST92B', 9201, '2022-10-15')",
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
            a.adesoes,
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
            (Some("A"), Some(1))
        );
        assert_eq!(a["steps"][0], "2025-03-10");
        assert_eq!(
            a["especialidades"],
            json!([{"name": "Nadador", "on": "2025-06-01"}])
        );
        assert_eq!(v["quality"]["withAdesao"], 1);

        // The comparison lists only the whole grant.
        let Json(list) = quality_list(State(state.clone()), acc())
            .await
            .unwrap_or_else(|e| panic!("{}", e.1));
        let list = list.as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0]["code"].as_i64(), list[0]["members"].as_u64()),
            (Some(9201), Some(1))
        );

        cleanup(db).await;
    }
}
