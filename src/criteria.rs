//! Quality criteria: conditions on how well an agrupamento keeps SIIE up to date and runs its
//! secções, checked per secção and ano escutista. Each criterion is data (`CRITERIA`): a key, its
//! texts, a level and a check. Adding one is one entry here, and the page shows whatever this list
//! holds, which leaves room for criteria users define themselves later.
//!
//! - A **member** criterion has a number, the members of the secção that break a member check
//!   that year, and a condition on it: the secção fails it when more than `max_share` of its
//!   members do.
//! - A **secção** criterion is a condition on the secção as a whole that year (1 fails, 0 meets).
//!
//! Every criterion a secção fails costs its level's points (`Level::points`); an agrupamento's
//! points are the sum over its secções, so the more points, the worse.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::digest::{
    MemberDates, SECCOES, dated_at, estimate_subunidades, member_years, scout_year, subunidade_min,
    year_label,
};

/// How bad failing a criterion is.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Minor: none yet, but planned.
    #[allow(dead_code)]
    Green,
    /// Worth a look.
    Yellow,
    /// Something is wrong.
    Red,
}

impl Level {
    /// What failing a criterion of this level costs: red 3, yellow 2, green 1.
    pub fn points(self) -> u32 {
        match self {
            Level::Green => 1,
            Level::Yellow => 2,
            Level::Red => 3,
        }
    }
}

/// What a criterion looks at, and when the secção fails it.
pub enum Check {
    /// Each member, in the secção they're counted in that year: counts those that break `check`.
    /// The secção fails when more than `max_share` (0 to 1) of its members do.
    Members {
        check: fn(&MemberYear) -> bool,
        max_share: f64,
    },
    /// The secção as a whole that year: fails when it's true.
    Seccao(fn(&SeccaoYear) -> bool),
}

pub struct Criterion {
    pub key: &'static str,
    pub label: &'static str,
    /// What breaking it means, in a sentence.
    pub hint: &'static str,
    pub level: Level,
    pub check: Check,
}

pub const CRITERIA: &[Criterion] = &[
    Criterion {
        key: "progresso",
        label: "Progresso por registar",
        hint: "Fizeram a promessa, mas não têm nenhuma etapa da secção registada no SIIE.",
        level: Level::Yellow,
        // TODO(review): the user set 70% for the promessa; this one uses the same until decided.
        check: Check::Members {
            check: progresso_missing,
            max_share: 0.7,
        },
    },
    Criterion {
        key: "promessa",
        label: "Promessa por registar",
        hint: "Estão no progresso da secção há mais de um ano escutista, ou já não são aspirantes, mas não têm a promessa no SIIE.",
        level: Level::Red,
        check: Check::Members {
            check: promessa_missing,
            max_share: 0.7,
        },
    },
    Criterion {
        key: "minimo",
        label: "Abaixo do mínimo",
        hint: "A secção tem menos elementos do que uma sub-unidade precisa: 5 nos lobitos, 4 nas outras.",
        level: Level::Red,
        check: Check::Seccao(below_minimum),
    },
    Criterion {
        key: "instrutores",
        label: "Instrutores insuficientes",
        hint: "Menos de um instrutor por sub-unidade (estimadas pelo número de elementos).",
        level: Level::Yellow,
        check: Check::Seccao(too_few_instrutores),
    },
];

/// Progresso por registar: promised, but no step at all in the secção. A member who promised in an
/// earlier secção starts this one in adesão with nothing to register yet, so they're only flagged
/// from their second year in it.
fn progresso_missing(y: &MemberYear) -> bool {
    let settled = dated_at(&y.m.promessas, y.i, &y.year_end).is_some() || y.was_here;
    y.promised() && y.registered().is_none() && settled
}

/// Promessa por registar: in the secção's progresso for a full ano escutista, or already past
/// aspirante this year, but no promessa date.
fn promessa_missing(y: &MemberYear) -> bool {
    let due = y.registered().is_some_and(|d| d < y.year_start.as_str()) || y.promised();
    due && !y.promessa_dated()
}

/// Fewer members than one sub-unidade needs (5 lobitos, 4 otherwise).
fn below_minimum(s: &SeccaoYear) -> bool {
    s.members < subunidade_min(s.tipo)
}

/// Fewer instrutores than (estimated) sub-unidades.
fn too_few_instrutores(s: &SeccaoYear) -> bool {
    s.instrutores < s.subunidades()
}

/// A member, in the secção they're counted in one ano escutista: what member checks look at.
pub struct MemberYear<'a> {
    pub m: &'a MemberDates,
    /// The secção's index in SECCOES.
    pub i: usize,
    pub is_current: bool,
    /// Counted in this secção the year before too.
    pub was_here: bool,
    year_start: String,
    year_end: String,
}

impl MemberYear<'_> {
    /// A promessa dated by the year's end, in any secção. It's made once, in the secção the member
    /// joined (an explorador who promised as a lobito only has the Lobitos date; SIIE's promessa
    /// dates match dados completos' Dt. Promessa, checked 2026-10-22).
    fn promessa_dated(&self) -> bool {
        (0..SECCOES.len()).any(|j| dated_at(&self.m.promessas, j, &self.year_end).is_some())
    }

    /// Promised: a dated promessa, or this year past aspirante (the categoria says so, dated or
    /// not).
    fn promised(&self) -> bool {
        let aspirante = self
            .m
            .categoria
            .as_deref()
            .is_some_and(|c| c.to_lowercase().contains("aspirante"));
        self.promessa_dated() || (self.is_current && self.m.active && !aspirante)
    }

    /// The earliest step registered in this secção by the year's end.
    fn registered(&self) -> Option<&str> {
        dated_at(&self.m.first_steps, self.i, &self.year_end)
    }
}

/// A secção in one ano escutista: what secção checks look at.
pub struct SeccaoYear {
    pub tipo: i16,
    pub members: u32,
    /// Instrutores with a cargo in the secção at some point of the year.
    pub instrutores: u32,
}

impl SeccaoYear {
    fn subunidades(&self) -> u32 {
        estimate_subunidades(self.members, subunidade_min(self.tipo)).len() as u32
    }
}

/// An instrutor cargo in a secção, and when it held (`YYYY-MM-DD`; `to` None = still does).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstrutorSpan {
    pub agrupamento: i16,
    pub tipo: i16,
    pub from: String,
    pub to: Option<String>,
}

/// One secção's criteria in one ano escutista.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CriteriaYear {
    pub year: i32,
    pub label: String,
    /// Members counted in the secção that year.
    pub members: u32,
    /// Per criterion key, its number: members that break the member check (member criteria), or
    /// 1 / 0 (secção criteria).
    pub flags: BTreeMap<&'static str, u32>,
    /// The criteria the secção fails that year (their conditions), in CRITERIA order.
    pub failed: Vec<&'static str>,
    /// The points those cost (`Level::points`).
    pub points: u32,
}

/// The criteria as the page needs them: key, texts, level and its points, scope ("members" or
/// "seccao") and, for member criteria, the share above which the secção fails.
pub fn definitions() -> Vec<Value> {
    CRITERIA
        .iter()
        .map(|c| {
            let (scope, max_share) = match c.check {
                Check::Members { max_share, .. } => ("members", Some(max_share)),
                Check::Seccao(_) => ("seccao", None),
            };
            json!({"key": c.key, "label": c.label, "hint": c.hint, "level": c.level,
                   "points": c.level.points(), "scope": scope, "maxShare": max_share})
        })
        .collect()
}

/// Whether a secção fails a criterion, from that year's numbers.
fn fails(c: &Criterion, year: &CriteriaYear) -> bool {
    let n = year.flags.get(c.key).copied().unwrap_or(0);
    match c.check {
        // More than max_share of its members: 8 of 10 with 0.7 fails, 7 of 10 doesn't.
        Check::Members { max_share, .. } => {
            year.members > 0 && f64::from(n) > max_share * f64::from(year.members)
        }
        Check::Seccao(_) => n > 0,
    }
}

/// Every criterion per secção (SECCOES order) and ano escutista, oldest first, for the years the
/// secção had members. `instrutores` are the agrupamento's instrutor cargos.
pub fn evaluate(
    members: &[MemberDates],
    instrutores: &[InstrutorSpan],
    today: &str,
) -> Vec<Vec<CriteriaYear>> {
    let current = scout_year(today);
    let mut per: Vec<BTreeMap<i32, CriteriaYear>> = vec![BTreeMap::new(); SECCOES.len()];

    // 1. Member criteria, each member in the secção they're counted in that year.
    for m in members {
        let years = member_years(m, today);
        for (k, &(y, b, _)) in years.iter().enumerate() {
            let Some(i) = SECCOES.iter().position(|s| s.1 == b) else {
                continue; // Dirigentes and Sem registo are no secção's members.
            };
            let c = per[i].entry(y).or_insert_with(|| CriteriaYear {
                year: y,
                label: year_label(y),
                members: 0,
                flags: CRITERIA.iter().map(|c| (c.key, 0)).collect(),
                failed: Vec::new(),
                points: 0,
            });
            c.members += 1;
            let my = MemberYear {
                m,
                i,
                is_current: y == current,
                was_here: k > 0 && years[k - 1].1 == b,
                year_start: format!("{y}-10-01"),
                year_end: format!("{}-09-30", y + 1),
            };
            for crit in CRITERIA {
                if let Check::Members { check, .. } = crit.check
                    && check(&my)
                {
                    *c.flags.entry(crit.key).or_default() += 1;
                }
            }
        }
    }

    // 2. Secção criteria, for each year the secção had members.
    for (i, years) in per.iter_mut().enumerate() {
        let tipo = SECCOES[i].0;
        for (y, c) in years.iter_mut() {
            let (start, end) = (format!("{y}-10-01"), format!("{}-09-30", y + 1));
            // Instrutor cargos of the secção that held at some point of the year.
            let instrutores = instrutores
                .iter()
                .filter(|s| {
                    s.tipo == tipo && s.from <= end && s.to.as_ref().is_none_or(|t| *t >= start)
                })
                .count() as u32;
            let s = SeccaoYear {
                tipo,
                members: c.members,
                instrutores,
            };
            for crit in CRITERIA {
                if let Check::Seccao(check) = crit.check {
                    c.flags.insert(crit.key, u32::from(check(&s)));
                }
            }
            // 3. The conditions: which criteria the secção fails that year, and their points.
            for crit in CRITERIA {
                if fails(crit, c) {
                    c.failed.push(crit.key);
                    c.points += crit.level.points();
                }
            }
        }
    }
    per.into_iter().map(|t| t.into_values().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-10-21";

    fn d(s: &str) -> Option<String> {
        Some(s.into())
    }

    /// One value per secção, in SECCOES order.
    fn per_seccao(l: Option<&str>, e: Option<&str>) -> Vec<Option<String>> {
        [l, e, None, None]
            .into_iter()
            .map(|x| x.map(String::from))
            .collect()
    }

    fn member() -> MemberDates {
        MemberDates {
            first_seen: "2026-10-20".into(),
            active: true,
            first_steps: vec![None; 4],
            promessas: vec![None; 4],
            ..Default::default()
        }
    }

    fn flags(c: &CriteriaYear) -> (u32, u32, u32, u32, u32) {
        (
            c.members,
            c.flags["progresso"],
            c.flags["promessa"],
            c.flags["minimo"],
            c.flags["instrutores"],
        )
    }

    #[test]
    fn member_criteria_flag_missing_progresso_and_promessa() {
        // Lobitos since 2024/25 (by age: born 2017), this year all past aspirante.
        let lobito = |step: Option<&str>, promessa: Option<&str>, categoria: &str| MemberDates {
            admitted_on: d("2024-10-15"),
            birth_month: d("2017-01-01"),
            seccao: d("Lobitos"),
            categoria: d(categoria),
            first_steps: per_seccao(step, None),
            promessas: per_seccao(promessa, None),
            ..member()
        };
        let all = [
            // All registered: no flag.
            lobito(Some("2024-12-01"), Some("2025-05-01"), "Lobito"),
            // Promised (by categoria) with nothing registered: both flags this year.
            lobito(None, None, "Lobito"),
            // Promessa registered, no step: progresso flag, in both years.
            lobito(None, Some("2025-05-01"), "Lobito"),
            // A step in 2024/25, no promessa yet and still aspirante: promessa flag from 2025/26,
            // the first full ano escutista after it.
            lobito(Some("2025-06-01"), None, "Aspirante a Lobito"),
        ];
        let lob = &evaluate(&all, &[], TODAY)[0];
        let got: Vec<(i32, u32, u32)> = lob
            .iter()
            .map(|c| (c.year, c.flags["progresso"], c.flags["promessa"]))
            .collect();
        assert_eq!(got, vec![(2024, 1, 0), (2025, 1, 1), (2026, 2, 2)]);
        assert!(evaluate(&all, &[], TODAY)[1].is_empty());

        // An explorador who promised as a lobito and just moved up isn't flagged this year.
        let moved = MemberDates {
            admitted_on: d("2022-10-01"),
            birth_month: d("2016-01-01"),
            seccao: d("Exploradores"),
            categoria: d("Explorador"),
            first_steps: per_seccao(Some("2023-01-01"), None),
            promessas: per_seccao(Some("2023-05-01"), None),
            ..member()
        };
        let exp = &evaluate(&[moved], &[], TODAY)[1];
        assert_eq!(
            (
                exp[0].year,
                exp[0].flags["progresso"],
                exp[0].flags["promessa"]
            ),
            (2026, 0, 0)
        );
    }

    #[test]
    fn seccao_criteria_flag_the_minimum_and_instrutores() {
        let lobito = || MemberDates {
            admitted_on: d("2026-10-05"),
            seccao: d("Lobitos"),
            categoria: d("Aspirante a Lobito"),
            ..member()
        };
        let span = |from: &str, to: Option<&str>| InstrutorSpan {
            agrupamento: 1,
            tipo: 1,
            from: from.into(),
            to: to.map(String::from),
        };
        // 4 lobitos: under the minimum of 5 (one sub-unidade), and no instrutor for it.
        let four = vec![lobito(); 4];
        assert_eq!(flags(&evaluate(&four, &[], TODAY)[0][0]), (4, 0, 0, 1, 1));
        // 12 lobitos = 2 bandos: one instrutor isn't enough, two are. One that ended last year
        // doesn't count.
        let twelve = vec![lobito(); 12];
        let one = [
            span("2020-10-01", None),
            span("2019-10-01", Some("2025-06-30")),
        ];
        assert_eq!(
            flags(&evaluate(&twelve, &one, TODAY)[0][0]),
            (12, 0, 0, 0, 1)
        );
        let two = [span("2020-10-01", None), span("2026-10-10", None)];
        assert_eq!(
            flags(&evaluate(&twelve, &two, TODAY)[0][0]),
            (12, 0, 0, 0, 0)
        );
        // Another secção's instrutor doesn't count here.
        let other = [
            span("2020-10-01", None),
            InstrutorSpan {
                tipo: 2,
                ..span("2020-10-01", None)
            },
        ];
        assert_eq!(flags(&evaluate(&twelve, &other, TODAY)[0][0]).4, 1);
    }

    #[test]
    fn conditions_fail_above_the_share_and_cost_points() {
        let year = |members: u32, promessa: u32, minimo: u32| CriteriaYear {
            year: 2026,
            label: "2026/27".into(),
            members,
            flags: BTreeMap::from([
                ("progresso", 0),
                ("promessa", promessa),
                ("minimo", minimo),
                ("instrutores", 0),
            ]),
            failed: Vec::new(),
            points: 0,
        };
        let promessa = CRITERIA.iter().find(|c| c.key == "promessa").unwrap();
        let minimo = CRITERIA.iter().find(|c| c.key == "minimo").unwrap();
        // 70%: 7 of 10 meets it, 8 of 10 fails it.
        assert!(!fails(promessa, &year(10, 7, 0)));
        assert!(fails(promessa, &year(10, 8, 0)));
        assert!(!fails(promessa, &year(0, 0, 0)));
        assert!(fails(minimo, &year(3, 0, 1)));

        // 4 lobitos with nothing registered and all past aspirante: progresso and promessa both
        // over 70%, under the minimum and no instrutor. 2 + 3 + 3 + 2 points.
        let lobito = MemberDates {
            admitted_on: d("2025-10-05"),
            birth_month: d("2017-01-01"),
            seccao: d("Lobitos"),
            categoria: d("Lobito"),
            ..member()
        };
        let now = evaluate(&vec![lobito; 4], &[], TODAY)[0]
            .last()
            .cloned()
            .unwrap();
        assert_eq!(
            now.failed,
            vec!["progresso", "promessa", "minimo", "instrutores"]
        );
        assert_eq!(now.points, 10);
    }

    #[test]
    fn definitions_carry_level_and_scope() {
        let defs = definitions();
        assert_eq!(defs.len(), CRITERIA.len());
        let minimo = defs.iter().find(|d| d["key"] == "minimo").unwrap();
        assert_eq!(
            (minimo["points"].as_u64(), minimo["maxShare"].is_null()),
            (Some(3), true)
        );
        assert_eq!(
            (minimo["level"].as_str(), minimo["scope"].as_str()),
            (Some("red"), Some("seccao"))
        );
        let progresso = defs.iter().find(|d| d["key"] == "progresso").unwrap();
        assert_eq!(
            (progresso["level"].as_str(), progresso["scope"].as_str()),
            (Some("yellow"), Some("members"))
        );
    }
}
