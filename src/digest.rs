//! Agrupamento digest: how many members an agrupamento had per ano escutista, in total and per
//! secção, rebuilt from each member's admission, progression and exit data. Only counts leave
//! this module, never a NIN, totem or any per-member value.
//!
//! - An ano escutista runs from October 1 to September 30; 2024 means 2024/25.
//! - A member counts in every year from their start to their end, both included ("active at any
//!   time in it").
//! - Their secção in a year is the one they were in on its last day (September 30), so the secções
//!   always add up to the total.

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
    /// No adesão date (or current secção) says where the member was that year.
    SemRegisto,
}

/// The dates one member's years are built from, all `YYYY-MM-DD`. Loaded by `load`.
#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct MemberDates {
    pub admitted_on: Option<String>,
    /// The earliest adesão or promessa in any secção.
    pub first_record: Option<String>,
    /// The latest dated record: admission, adesão, etapa, anilha, promessa or cargo.
    pub last_record: Option<String>,
    /// When YASA first saw the member: the last resort for the start.
    pub first_seen: String,
    /// Present in the latest import and situação "Ativo..." (includes "Ativo (Formação)").
    pub active: bool,
    /// Former members: the year they left (saida-elementos-por-ano).
    pub ano_saida: Option<i32>,
    /// Adesão to each secção: Lobitos, Exploradores, Pioneiros, Caminheiros.
    pub ladesao: Option<String>,
    pub eadesao: Option<String>,
    pub padesao: Option<String>,
    pub cadesao: Option<String>,
    /// The promessa de dirigente or the first cargo in this agrupamento, whichever came first.
    pub dirigente_since: Option<String>,
    /// Current secção and categoria, as SIIE writes them.
    pub seccao: Option<String>,
    pub categoria: Option<String>,
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
}

/// The ano escutista a `YYYY-MM-DD` date falls in: October onwards starts a new one.
pub fn scout_year(date: &str) -> i32 {
    let year: i32 = date[..4].parse().unwrap_or_default();
    let month: u32 = date[5..7].parse().unwrap_or_default();
    if month >= 10 { year } else { year - 1 }
}

/// The years a member counts in, with their bucket in each, up to the current year.
pub fn member_years(m: &MemberDates, today: &str) -> Vec<(i32, Bucket)> {
    let current = scout_year(today);

    // Start: the admission date, else the first adesão/promessa, else when YASA first saw them.
    let start_date = m
        .admitted_on
        .as_deref()
        .or(m.first_record.as_deref())
        .unwrap_or(&m.first_seen);
    let start = scout_year(start_date);

    // End: active members are still here. A former member's ano_saida is read as the calendar
    // year the ano escutista they left in ends (left in 2019 = last year 2018/19). Without it,
    // the year of their latest dated record.
    // TODO(review): check the ano_saida convention against a few real former members.
    let end = if m.active {
        current
    } else if let Some(y) = m.ano_saida {
        y - 1
    } else {
        scout_year(m.last_record.as_deref().unwrap_or(&m.first_seen))
    };
    let end = end.clamp(start, current);

    (start..=end)
        .map(|y| (y, bucket(m, y, y == current)))
        .collect()
}

/// Where a member was on the last day (September 30) of ano escutista `year`.
fn bucket(m: &MemberDates, year: i32, is_current: bool) -> Bucket {
    let year_end = format!("{}-09-30", year + 1);
    let by = |d| on_or_before(d, &year_end);

    // 1. Dirigente from the promessa de dirigente or first cargo onwards.
    if by(&m.dirigente_since).is_some() {
        return Bucket::Dirigentes;
    }
    // 2. This year, an active member is where SIIE says they are now.
    if is_current
        && m.active
        && let Some(b) = current_bucket(m.seccao.as_deref(), m.categoria.as_deref())
    {
        return b;
    }
    // 3. The secção of the latest adesão by the year's end.
    [
        (&m.ladesao, Bucket::Lobitos),
        (&m.eadesao, Bucket::Exploradores),
        (&m.padesao, Bucket::Pioneiros),
        (&m.cadesao, Bucket::Caminheiros),
    ]
    .into_iter()
    .filter_map(|(d, b)| by(d).map(|d| (d, b)))
    .max_by_key(|(d, _)| *d)
    .map_or(Bucket::SemRegisto, |(_, b)| b)
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
    let all: Vec<(i32, Bucket)> = members
        .iter()
        .flat_map(|m| member_years(m, today))
        .collect();
    let Some(first) = all.iter().map(|(y, _)| *y).min() else {
        return Vec::new();
    };
    let mut rows: Vec<YearRow> = (first..=current)
        .map(|year| YearRow {
            year,
            label: format!("{year}/{:02}", (year + 1) % 100),
            ..Default::default()
        })
        .collect();
    for (y, b) in all {
        let r = &mut rows[(y - first) as usize];
        r.total += 1;
        *match b {
            Bucket::Lobitos => &mut r.lobitos,
            Bucket::Exploradores => &mut r.exploradores,
            Bucket::Pioneiros => &mut r.pioneiros,
            Bucket::Caminheiros => &mut r.caminheiros,
            Bucket::Dirigentes => &mut r.dirigentes,
            Bucket::SemRegisto => &mut r.sem_registo,
        } += 1;
    }
    rows
}

/// Every member ever seen in the agrupamento, present or not, with the dates `member_years` needs.
async fn load(db: &sqlx::PgPool, code: i16) -> Result<Vec<MemberDates>, sqlx::Error> {
    // least / greatest skip nulls, so each one is the earliest / latest date the member has.
    sqlx::query_as(
        "select m.admitted_on::text as admitted_on,
                least(p.ladesao, p.eadesao, p.padesao, p.cadesao, p.promessalobito,
                      p.promessaexplorador, p.promessapioneiro, p.promessacaminheiro,
                      p.promessadirigente)::text as first_record,
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
                p.ano_saida,
                p.ladesao::text as ladesao, p.eadesao::text as eadesao,
                p.padesao::text as padesao, p.cadesao::text as cadesao,
                least(p.promessadirigente, c.first_cargo)::text as dirigente_since,
                m.seccao, m.categoria
         from members m
         left join member_progress p on p.nin = m.nin
         left join lateral (
             select min(started_on) as first_cargo,
                    greatest(max(started_on), max(ended_on)) as last_cargo
             from member_cargos where nin = m.nin and agrupamento = $1
         ) c on true
         where m.agrupamento = $1",
    )
    .bind(code)
    .fetch_all(db)
    .await
}

/// GET /api/agrupamentos/{code}/digest: members per ano escutista, in total and per secção, plus
/// the agrupamento's own data. Needs a whole-agrupamento grant: secção-only readers have no
/// import of their agrupamento, so there's nothing to count for them.
pub async fn get(
    State(s): State<AppState>,
    acc: CurrentAccount,
    Path(code): Path<i16>,
) -> Result<Json<Value>, ApiError> {
    let db = &s.db;
    let agrupamento = granted(db, acc.id, code)
        .await
        .map_err(db_error)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::FORBIDDEN,
                "O resumo só aparece com acesso ao agrupamento inteiro.",
            )
        })?;
    let (today,): (String,) = sqlx::query_as("select current_date::text")
        .fetch_one(db)
        .await
        .map_err(db_error)?;
    let members = load(db, code).await.map_err(db_error)?;
    let (last_import_at,): (Option<i64>,) = sqlx::query_as(
        "select extract(epoch from max(finished_at))::bigint from member_imports
         where agrupamento = $1 and status = 'done'",
    )
    .bind(code)
    .fetch_one(db)
    .await
    .map_err(db_error)?;
    Ok(Json(json!({
        "agrupamento": agrupamento,
        "years": digest(&members, &today),
        "lastImportAt": last_import_at,
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
            ..Default::default()
        }
    }

    fn d(s: &str) -> Option<String> {
        Some(s.into())
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
            ladesao: d("2022-11-05"),
            eadesao: d("2025-03-10"),
            seccao: d("Exploradores"),
            ..member()
        };
        assert_eq!(
            member_years(&m, TODAY),
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
    fn former_members_end_at_ano_saida_or_their_last_record() {
        let left = MemberDates {
            admitted_on: d("2015-10-10"),
            ladesao: d("2015-10-10"),
            ano_saida: Some(2018),
            active: false,
            ..member()
        };
        assert_eq!(
            member_years(&left, TODAY)
                .iter()
                .map(|y| y.0)
                .collect::<Vec<_>>(),
            vec![2015, 2016, 2017]
        );
        let inactive = MemberDates {
            admitted_on: d("2019-10-10"),
            last_record: d("2021-05-01"),
            active: false,
            ..member()
        };
        assert_eq!(
            member_years(&inactive, TODAY),
            vec![(2019, Bucket::SemRegisto), (2020, Bucket::SemRegisto)]
        );
    }

    #[test]
    fn dirigentes_from_promessa_or_cargo() {
        let m = MemberDates {
            admitted_on: d("2020-10-01"),
            cadesao: d("2020-10-01"),
            dirigente_since: d("2023-06-01"),
            seccao: d("Caminheiros"),
            ..member()
        };
        let years = member_years(&m, TODAY);
        assert_eq!(years[1], (2021, Bucket::Caminheiros));
        assert_eq!(years[2], (2022, Bucket::Dirigentes));
        assert_eq!(years.last(), Some(&(2026, Bucket::Dirigentes)));
    }

    #[test]
    fn no_adesao_is_sem_registo_except_this_year() {
        let m = MemberDates {
            admitted_on: d("2024-10-01"),
            seccao: d("Pioneiros"),
            ..member()
        };
        assert_eq!(
            member_years(&m, TODAY),
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
    fn digest_fills_empty_years_and_seccoes_add_up() {
        let old = MemberDates {
            admitted_on: d("2018-10-01"),
            ladesao: d("2018-10-01"),
            ano_saida: Some(2020),
            active: false,
            ..member()
        };
        let new = MemberDates {
            admitted_on: d("2024-10-01"),
            ladesao: d("2024-10-01"),
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
                                  admitted_on, present)
             values ('TEST92A', 'A', false, 9201, 'Ativo', 'Exploradores', '2022-11-05', true),
                    ('TEST92B', 'B', false, 9201, 'Inativo', null, '2021-10-10', false)",
            "insert into member_progress (nin, ladesao, eadesao) values
                ('TEST92A', '2022-11-05', '2025-03-10')",
            "insert into member_progress (nin, ladesao, ano_saida) values
                ('TEST92B', '2021-10-10', 2023)",
            "insert into member_cargos (siie_id, nin, agrupamento, started_on)
             values (-9201, 'TEST92B', 9201, '2022-10-15')",
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

        let members = load(db, 9201).await.unwrap();
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

        cleanup(db).await;
    }
}
