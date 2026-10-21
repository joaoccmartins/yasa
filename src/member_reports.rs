//! The SIIE reports read alongside "dados completos" in each agrupamento import, described as
//! data so a new report is one entry here plus a migration:
//!
//! - `COLUMN_REPORTS`: at most one row per member (several for `Kind::CodeList` columns), stored
//!   as columns of a table with one row per member (progress, exits, past agrupamentos, health).
//! - `RECORD_REPORTS`: any number of rows per member, each a SIIE record with its own id (`rid`),
//!   stored as one row per record (cargos, cursos, atividades, distinções, habilitações).
//!
//! Only the NIN, the agrupamento and the listed columns are read from each row. Names, contacts,
//! addresses, internal ids and everything else are dropped as the row is parsed.

use std::collections::{BTreeMap, HashMap, HashSet};

use reqwest::Client;
use secrecy::SecretString;
use serde_json::{Map, Value};
use sqlx::{PgConnection, types::Json};

use crate::{
    members::{AGRUPAMENTO, Change, NIN, changed, date, flag, row_nin, text, value_text},
    siie::{self, SiieError, field_code},
};

/// How a column is read from the grid and stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Text,
    /// ISO date, stored as `date`.
    Date,
    Flag,
    /// Organisation code ("0455" -> 455; all-zero placeholders are null).
    Code,
    Number,
    /// One code per row, gathered over a member's rows into a sorted `smallint[]`.
    CodeList,
}

impl Kind {
    fn sql(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Date => "date",
            Kind::Flag => "boolean",
            Kind::Code => "smallint",
            Kind::Number => "integer",
            Kind::CodeList => "smallint[]",
        }
    }
}

/// A report with (at most) one row per member, stored as columns of `table`, under the grid's
/// own column names.
pub struct ColumnReport {
    pub report: &'static str,
    pub table: &'static str,
    pub columns: &'static [(&'static str, Kind)],
    /// false: the report has no agrupamento column, so it is read whole once per run and
    /// matched by NIN.
    pub by_agrupamento: bool,
}

use Kind::*;

// Keys checked 2026-10-21 with the POC profile. A missing key fails the import.
#[rustfmt::skip]
pub const COLUMN_REPORTS: &[ColumnReport] = &[
    // One secção's steps each: prefix l/e/p/c + adesao, etapa1-3, anilhamerito.
    ColumnReport { report: "elementos/progresso/lista-lobitos", table: "member_progress",
        columns: &[("ladesao", Date), ("letapa1", Date), ("letapa2", Date), ("letapa3", Date),
                   ("lanilhamerito", Date)], by_agrupamento: true },
    ColumnReport { report: "elementos/progresso/lista-exploradores", table: "member_progress",
        columns: &[("eadesao", Date), ("eetapa1", Date), ("eetapa2", Date), ("eetapa3", Date),
                   ("eanilhamerito", Date)], by_agrupamento: true },
    ColumnReport { report: "elementos/progresso/lista-pioneiros", table: "member_progress",
        columns: &[("padesao", Date), ("petapa1", Date), ("petapa2", Date), ("petapa3", Date),
                   ("panilhamerito", Date)], by_agrupamento: true },
    ColumnReport { report: "elementos/progresso/lista-caminheiros", table: "member_progress",
        columns: &[("cadesao", Date), ("cetapa1", Date), ("cetapa2", Date), ("cetapa3", Date),
                   ("canilhamerito", Date)], by_agrupamento: true },
    // The promessa made in each secção.
    ColumnReport { report: "elementos/vidaescutista/lista-geral", table: "member_progress",
        columns: &[("promessalobito", Date), ("promessaexplorador", Date),
                   ("promessapioneiro", Date), ("promessacaminheiro", Date),
                   ("promessadirigente", Date)], by_agrupamento: true },
    // Agrupamentos the member was in before (SIIE keeps no dates for them).
    ColumnReport { report: "elementos/elementos/lista-historico", table: "member_progress",
        columns: &[("histagrupamento", CodeList)], by_agrupamento: true },
    // Former members: the year they left and the last census they were in.
    ColumnReport { report: "elementos/elementos/saida-elementos-por-ano", table: "member_progress",
        columns: &[("ano_saida", Number), ("ultimoano_censos", Number)], by_agrupamento: false },
    // Health conditions (yes/no) and the tetanus vaccine date. The descriptions and the SNS
    // number are not read.
    ColumnReport { report: "elementos/dadossaude/dados-saude", table: "member_health",
        columns: &[("alergias", Flag), ("asma", Flag), ("diabetes", Flag), ("epilepsia", Flag),
                   ("medicacao", Flag), ("outroprob", Flag), ("restricoes", Flag),
                   ("vacinaantitet", Date)], by_agrupamento: true },
];

/// member_changes prefix per column table. Tables not listed keep no history (health).
const HISTORY: &[(&str, &str)] = &[("member_progress", "progresso.")];

/// A report with any number of rows per member, each a SIIE record with its own id (`rid`),
/// stored as one row of `table` keyed by `siie_id`.
pub struct RecordReport {
    pub report: &'static str,
    pub table: &'static str,
    /// member_changes fields: "<label>.<siie_id>" (added / gone), "<label>.<siie_id>.<column>".
    pub label: &'static str,
    /// (grid key, column, kind). The first column names the record in added / gone changes.
    pub columns: &'static [(&'static str, &'static str, Kind)],
    /// If set, only rows whose grid key holds one of these values are kept; the rest are
    /// dropped as they are read.
    pub only: Option<(&'static str, &'static [&'static str])>,
    /// If set, an import only removes records whose this column is its own agrupamento: a
    /// narrower reader may not see a member's núcleo or região cargos, and must not delete them.
    pub own_column: Option<&'static str>,
}

// Distinções e sanções: only these award types are kept (seen 2026-10-21): louvores, Cruz de São
// Jorge, Medalha de Campo, Nó de Mérito, Cruz de Mérito, Dirigente Honorário, Scouts of the World
// Award, other distinctions. Any other type, sanctions included, is never stored.
const AWARDS: &[&str] = &[
    "LAGR", "LNUC", "LREG", "LNAC", "CSJ1", "CSJ2", "CSJ3", "MC3", "NM", "CMR", "DH", "ISWA",
    "OUTROS",
];

#[rustfmt::skip]
pub const RECORD_REPORTS: &[RecordReport] = &[
    // Every cargo assignment, past (activo false, datafim set) and current.
    RecordReport { report: "elementos/cargos/cargos-ativos", table: "member_cargos", label: "cargo",
        columns: &[("cargo", "cargo", Text), ("siglacargo", "sigla_cargo", Text),
                   ("agrup", "agrupamento", Code), ("datainicio", "started_on", Date),
                   ("datafim", "ended_on", Date), ("activo", "active", Flag)],
        only: None, own_column: Some("agrupamento") },
    // Courses (formação). The ordem de serviço is not read.
    RecordReport { report: "elementos/cursos/lista-cursos", table: "member_cursos", label: "curso",
        columns: &[("curso", "curso", Text), ("siglacurso", "sigla_curso", Text),
                   ("datadoc", "completed_on", Date)],
        only: None, own_column: None },
    // One row per participation. The activity's description and place are not read.
    RecordReport { report: "elementos/actividades/atividades-elementos", table: "member_atividades",
        label: "atividade",
        columns: &[("idatividade", "atividade", Text), ("siglatipo", "tipo", Text),
                   ("siglagrupo", "nivel", Text), ("datainicio", "started_on", Date),
                   ("datafim", "ended_on", Date), ("noites", "noites", Number),
                   ("noitescampo", "noites_campo", Number),
                   ("contanoitescampo", "conta_noites_campo", Flag)],
        only: None, own_column: None },
    RecordReport { report: "elementos/justica/lista-justica", table: "member_distincoes",
        label: "distincao",
        columns: &[("distsan", "distincao", Text), ("sigladistsan", "sigla", Text),
                   ("osdata", "awarded_on", Date)],
        only: Some(("sigladistsan", AWARDS)), own_column: None },
    RecordReport { report: "elementos/habilitacoes/lista-habilitacoes", table: "member_habilitacoes",
        label: "habilitacao",
        columns: &[("habilitacoes", "habilitacoes", Text)],
        only: None, own_column: None },
];

/// The reports, opened once per run (each page holds this session's report id).
pub struct Opened {
    columns: Vec<(siie::Report, &'static ColumnReport)>,
    records: Vec<(siie::Report, &'static RecordReport)>,
}

pub async fn open(client: &Client) -> Result<Opened, SiieError> {
    let mut columns = Vec::new();
    for r in COLUMN_REPORTS {
        columns.push((siie::open_report(client, r.report).await?, r));
    }
    let mut records = Vec::new();
    for r in RECORD_REPORTS {
        records.push((siie::open_report(client, r.report).await?, r));
    }
    Ok(Opened { columns, records })
}

/// Rows read from the reports: per column table, NIN -> its columns; per record table,
/// siie_id -> the record (with `siie_id`, `nin` and `import_agrupamento`).
#[derive(Default)]
pub struct Fetched {
    pub columns: HashMap<&'static str, HashMap<String, Map<String, Value>>>,
    pub records: HashMap<&'static str, BTreeMap<i64, Map<String, Value>>>,
}

/// Reads one agrupamento from every report that has an agrupamento column (`filter`), or, with
/// `filter` None, the whole of every report that hasn't (once per run).
pub async fn fetch(
    client: &Client,
    token: &SecretString,
    opened: &Opened,
    filter: Option<&Value>,
) -> Result<Fetched, SiieError> {
    let mut f = Fetched::default();
    for (report, r) in &opened.columns {
        if r.by_agrupamento != filter.is_some() {
            continue;
        }
        siie::for_each_row(client, token, report, filter, |row| {
            f.add_column_row(r, row)
        })
        .await?;
    }
    if filter.is_some() {
        for (report, r) in &opened.records {
            siie::for_each_row(client, token, report, filter, |row| {
                f.add_record_row(r, row)
            })
            .await?;
        }
    }
    Ok(f)
}

impl Fetched {
    /// Adds one row of a column report to its member's columns.
    pub fn add_column_row(
        &mut self,
        r: &'static ColumnReport,
        row: &Map<String, Value>,
    ) -> Result<(), SiieError> {
        require(row, r.columns.iter().map(|(k, _)| *k))?;
        let nin = row_nin(row)?;
        let columns = self
            .columns
            .entry(r.table)
            .or_default()
            .entry(nin.clone())
            .or_insert_with(|| Map::from_iter([(NIN.to_owned(), Value::String(nin))]));
        for &(key, kind) in r.columns {
            let v = read(row, key, kind)?;
            if kind == CodeList {
                // Gather the codes over the member's rows, sorted and without repeats.
                let Value::Number(code) = v else { continue };
                let list = columns
                    .entry(key)
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Value::Array(codes) = list {
                    codes.push(Value::Number(code));
                    codes.sort_by_key(|c| c.as_i64());
                    codes.dedup();
                }
            } else {
                columns.insert(key.to_owned(), v);
            }
        }
        Ok(())
    }

    /// Adds one row of a record report, unless `only` rules it out.
    pub fn add_record_row(
        &mut self,
        r: &'static RecordReport,
        row: &Map<String, Value>,
    ) -> Result<(), SiieError> {
        let keys = ["rid", NIN, AGRUPAMENTO]
            .into_iter()
            .chain(r.columns.iter().map(|(k, _, _)| *k));
        require(row, keys)?;
        if let Some((key, allowed)) = r.only
            && !text(row, key).is_some_and(|v| allowed.contains(&v.as_str()))
        {
            return Ok(());
        }
        let siie_id = row
            .get("rid")
            .and_then(Value::as_i64)
            .ok_or_else(|| SiieError::MemberFieldMissing("rid".to_owned()))?;
        let mut record = Map::from_iter([
            ("siie_id".to_owned(), Value::from(siie_id)),
            (NIN.to_owned(), Value::String(row_nin(row)?)),
            (
                "import_agrupamento".to_owned(),
                read(row, AGRUPAMENTO, Code)?,
            ),
        ]);
        for &(key, column, kind) in r.columns {
            record.insert(column.to_owned(), read(row, key, kind)?);
        }
        self.records
            .entry(r.table)
            .or_default()
            .insert(siie_id, record);
        Ok(())
    }
}

/// Fails with the missing key names (never values) if SIIE renamed a column.
fn require<'a>(
    row: &Map<String, Value>,
    keys: impl Iterator<Item = &'a str>,
) -> Result<(), SiieError> {
    let missing: Vec<&str> = keys.filter(|k| !row.contains_key(*k)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(SiieError::MemberFieldMissing(missing.join(", ")))
    }
}

/// One grid value as the JSON that `jsonb_to_recordset` turns into the column's type.
fn read(row: &Map<String, Value>, key: &'static str, kind: Kind) -> Result<Value, SiieError> {
    Ok(match kind {
        Text => text(row, key).map_or(Value::Null, Value::String),
        Date => date(row, key)?.map_or(Value::Null, Value::String),
        Flag => flag(row, key).map_or(Value::Null, Value::Bool),
        Code | CodeList => field_code(row, key)?.map_or(Value::Null, Value::from),
        Number => text(row, key)
            .and_then(|t| t.parse::<f64>().ok())
            .map_or(Value::Null, |n| Value::from(n.round() as i64)),
    })
}

/// Writes the reports' rows for one import and returns their changes. Runs inside the import's
/// transaction, after `members` is upserted (the tables reference it).
/// - `nins`: this import's members (sorted). Column tables are written for exactly these: a
///   member missing from a report gets nulls there (e.g. no longer in the health list).
/// - `known(nin)`: the member was stored before this import. Nothing is logged for one seen
///   for the first time: their values already are the history.
/// - Records are kept for any member YASA knows (an activity may be listed under an
///   agrupamento the member has since left), and an import only removes the records listed
///   under its own agrupamento.
pub(crate) async fn write(
    tx: &mut PgConnection,
    agrupamento: i16,
    nins: &[String],
    known: &impl Fn(&str) -> bool,
    sources: &[&Fetched],
) -> Result<Vec<Change>, sqlx::Error> {
    let mut changes = Vec::new();

    // Column tables, in COLUMN_REPORTS order.
    let mut tables: Vec<&'static str> = COLUMN_REPORTS.iter().map(|r| r.table).collect();
    tables.dedup();
    for table in tables {
        let cols: Vec<(&str, Kind)> = COLUMN_REPORTS
            .iter()
            .filter(|r| r.table == table)
            .flat_map(|r| r.columns.iter().copied())
            .collect();
        // Every member of the import, every column null unless a report says otherwise.
        let rows: Vec<Map<String, Value>> = nins
            .iter()
            .map(|nin| {
                let mut row = Map::from_iter([(NIN.to_owned(), Value::String(nin.clone()))]);
                for (c, _) in &cols {
                    row.insert((*c).to_owned(), Value::Null);
                }
                for s in sources {
                    if let Some(found) = s.columns.get(table).and_then(|t| t.get(nin)) {
                        row.extend(found.clone());
                    }
                }
                row
            })
            .collect();

        if let Some((_, prefix)) = HISTORY.iter().find(|(t, _)| *t == table) {
            let old = stored_by_nin(tx, table, nins).await?;
            for row in &rows {
                let nin = row.get(NIN).and_then(Value::as_str).unwrap_or_default();
                if let Some(o) = old.get(nin) {
                    changes.extend(changed(o, row, prefix));
                }
            }
        }

        // Column names come from COLUMN_REPORTS, never from SIIE.
        let names: Vec<&str> = cols.iter().map(|(c, _)| *c).collect();
        let defs: Vec<String> = cols
            .iter()
            .map(|(c, k)| format!("{c} {}", k.sql()))
            .collect();
        let sets: Vec<String> = names
            .iter()
            .map(|c| format!("{c} = excluded.{c}"))
            .collect();
        sqlx::query(&format!(
            "insert into {table} (nin, {})
             select * from jsonb_to_recordset($1) as r(nin text, {})
             on conflict (nin) do update set {}, updated_at = now()",
            names.join(", "),
            defs.join(", "),
            sets.join(", ")
        ))
        .bind(Json(&rows))
        .execute(&mut *tx)
        .await?;
    }

    // Record tables.
    let none = BTreeMap::new();
    for r in RECORD_REPORTS {
        let fetched = sources
            .iter()
            .find_map(|s| s.records.get(r.table))
            .unwrap_or(&none);
        // Only members YASA has a row for (the table references members).
        let record_nins: Vec<String> = fetched
            .values()
            .filter_map(|v| value_text(v.get(NIN)))
            .collect();
        let existing: Vec<(String,)> =
            sqlx::query_as("select nin from members where nin = any($1)")
                .bind(&record_nins)
                .fetch_all(&mut *tx)
                .await?;
        let existing: HashSet<String> = existing.into_iter().map(|(n,)| n).collect();
        let new: BTreeMap<i64, Map<String, Value>> = fetched
            .iter()
            .filter(|(_, v)| value_text(v.get(NIN)).is_some_and(|n| existing.contains(&n)))
            .map(|(id, v)| (*id, v.clone()))
            .collect();

        let ids: Vec<i64> = new.keys().copied().collect();
        let old: Vec<(i64, Value)> = sqlx::query_as(&format!(
            "select siie_id, to_jsonb(t) - 'first_seen_at' - 'last_seen_at' from {} t
             where siie_id = any($1) or import_agrupamento = $2",
            r.table
        ))
        .bind(&ids)
        .bind(agrupamento)
        .fetch_all(&mut *tx)
        .await?;
        let old: BTreeMap<i64, Map<String, Value>> = old
            .into_iter()
            .filter_map(|(id, v)| match v {
                Value::Object(m) => Some((id, m)),
                _ => None,
            })
            .collect();
        let own = |o: &Map<String, Value>| {
            let this = Some(&Value::from(agrupamento));
            o.get("import_agrupamento") == this && r.own_column.is_none_or(|c| o.get(c) == this)
        };
        let gone: Vec<i64> = old
            .iter()
            .filter(|(id, o)| !new.contains_key(*id) && own(o))
            .map(|(id, _)| *id)
            .collect();
        changes.extend(record_changes(r, &old, &new, &gone, known));

        let names: Vec<&str> = r.columns.iter().map(|(_, c, _)| *c).collect();
        let defs: Vec<String> = r
            .columns
            .iter()
            .map(|(_, c, k)| format!("{c} {}", k.sql()))
            .collect();
        let sets: Vec<String> = ["nin", "import_agrupamento"]
            .iter()
            .chain(names.iter())
            .map(|c| format!("{c} = excluded.{c}"))
            .collect();
        let rows: Vec<&Map<String, Value>> = new.values().collect();
        sqlx::query(&format!(
            "insert into {table} (siie_id, nin, import_agrupamento, {})
             select * from jsonb_to_recordset($1)
                 as r(siie_id bigint, nin text, import_agrupamento smallint, {})
             on conflict (siie_id) do update set {}, last_seen_at = now()",
            names.join(", "),
            defs.join(", "),
            sets.join(", "),
            table = r.table,
        ))
        .bind(Json(&rows))
        .execute(&mut *tx)
        .await?;
        sqlx::query(&format!("delete from {} where siie_id = any($1)", r.table))
            .bind(&gone)
            .execute(&mut *tx)
            .await?;
    }
    Ok(changes)
}

/// Records added ("<label>.<id>" null -> name; not for a member seen for the first time),
/// changed ("<label>.<id>.<column>") or gone ("<label>.<id>" name -> null). `old` rows are
/// `to_jsonb` of the table without its timestamps.
fn record_changes(
    r: &RecordReport,
    old: &BTreeMap<i64, Map<String, Value>>,
    new: &BTreeMap<i64, Map<String, Value>>,
    gone: &[i64],
    known: &impl Fn(&str) -> bool,
) -> Vec<Change> {
    let name = r.columns[0].1;
    let mut changes = Vec::new();
    for (id, n) in new {
        let nin = value_text(n.get(NIN)).unwrap_or_default();
        match old.get(id) {
            None if known(&nin) => changes.push(Change {
                field: format!("{}.{id}", r.label),
                old_value: None,
                new_value: value_text(n.get(name)),
                nin,
            }),
            Some(o) => changes.extend(changed(o, n, &format!("{}.{id}.", r.label))),
            None => {}
        }
    }
    for id in gone {
        let o = &old[id];
        changes.push(Change {
            nin: value_text(o.get(NIN)).unwrap_or_default(),
            field: format!("{}.{id}", r.label),
            old_value: value_text(o.get(name)),
            new_value: None,
        });
    }
    changes
}

/// The stored rows of a per-member table for these NINs, as JSON objects keyed by NIN.
async fn stored_by_nin(
    tx: &mut PgConnection,
    table: &'static str,
    nins: &[String],
) -> Result<HashMap<String, Map<String, Value>>, sqlx::Error> {
    let rows: Vec<(Value,)> = sqlx::query_as(&format!(
        "select to_jsonb(t) - 'updated_at' from {table} t where nin = any($1)"
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn report(path: &str) -> &'static ColumnReport {
        COLUMN_REPORTS.iter().find(|r| r.report == path).unwrap()
    }

    fn record_report(table: &str) -> &'static RecordReport {
        RECORD_REPORTS.iter().find(|r| r.table == table).unwrap()
    }

    #[test]
    fn column_reports_merge_per_member() {
        let mut f = Fetched::default();
        let lobito = obj(
            json!({"nin": "1", "nome": "X", "ladesao": "2015-01-10T00:00:00",
            "letapa1": null, "letapa2": null, "letapa3": null, "lanilhamerito": null}),
        );
        let explorador = obj(json!({"nin": "1", "eadesao": "2018-02-01T00:00:00",
            "eetapa1": null, "eetapa2": null, "eetapa3": null, "eanilhamerito": null}));
        f.add_column_row(report("elementos/progresso/lista-lobitos"), &lobito)
            .unwrap();
        f.add_column_row(
            report("elementos/progresso/lista-exploradores"),
            &explorador,
        )
        .unwrap();
        // Two past agrupamentos, one repeated: a sorted list without repeats.
        for a in ["0566", "0455", "0566"] {
            let r = obj(json!({"nin": "1", "histagrupamento": a}));
            f.add_column_row(report("elementos/elementos/lista-historico"), &r)
                .unwrap();
        }
        let p = &f.columns["member_progress"]["1"];
        assert_eq!(p["ladesao"], "2015-01-10");
        assert_eq!(p["eadesao"], "2018-02-01");
        assert_eq!(p["histagrupamento"], json!([455, 566]));
        assert!(!p.contains_key("nome"));

        // A renamed column fails loudly, with the key's name.
        let renamed = obj(json!({"nin": "1", "cadesao": null}));
        assert!(matches!(
            f.add_column_row(report("elementos/progresso/lista-caminheiros"), &renamed),
            Err(SiieError::MemberFieldMissing(k)) if k.starts_with("cetapa1")
        ));
    }

    #[test]
    fn health_keeps_flags_and_vaccine_only() {
        let mut f = Fetched::default();
        let r = obj(json!({"nin": "1", "nome": "X", "sns": "123456789",
            "descalergias": "amendoim", "alergias": true, "asma": false, "diabetes": false,
            "epilepsia": false, "medicacao": false, "outroprob": false, "restricoes": false,
            "vacinaantitet": "2020-05-01T00:00:00"}));
        f.add_column_row(report("elementos/dadossaude/dados-saude"), &r)
            .unwrap();
        let h = &f.columns["member_health"]["1"];
        assert_eq!(
            (h["alergias"].clone(), h["vacinaantitet"].clone()),
            (json!(true), json!("2020-05-01"))
        );
        let all = format!("{h:?}");
        assert!(!all.contains("amendoim") && !all.contains("123456789"));
    }

    #[test]
    fn records_keep_listed_columns_and_allowed_values() {
        let mut f = Fetched::default();
        let cargo = obj(
            json!({"rid": 77, "nin": "1", "agrupamento": "0455", "nome": "X",
            "email": "x@y.pt", "cargo": "Chefe de Unidade", "siglacargo": "CU", "agrup": "0455",
            "datainicio": "2020-09-01T00:00:00", "datafim": null, "activo": true}),
        );
        f.add_record_row(record_report("member_cargos"), &cargo)
            .unwrap();
        let c = &f.records["member_cargos"][&77];
        assert_eq!(
            (c["agrupamento"].clone(), c["import_agrupamento"].clone()),
            (json!(455), json!(455))
        );
        assert_eq!(c["started_on"], "2020-09-01");
        assert!(!format!("{c:?}").contains("x@y.pt"));

        // Justiça: awards are kept, anything else is dropped as it is read.
        let distincao = |rid: i64, sigla: &str| {
            obj(
                json!({"rid": rid, "nin": "1", "agrupamento": "0455", "distsan": "X",
                "sigladistsan": sigla, "osdata": null}),
            )
        };
        let r = record_report("member_distincoes");
        f.add_record_row(r, &distincao(1, "LAGR")).unwrap();
        f.add_record_row(r, &distincao(2, "SUSP")).unwrap();
        assert_eq!(
            f.records["member_distincoes"].keys().collect::<Vec<_>>(),
            vec![&1]
        );

        // Activities: negative and fractional night counts are whole numbers.
        let a = obj(
            json!({"rid": 5, "nin": "1", "agrupamento": "0005", "idatividade": "a123",
            "siglatipo": "acamp", "siglagrupo": "agr", "datainicio": null,
            "datafim": "2024-08-10T00:00:00", "noites": -3, "noitescampo": 2.0,
            "contanoitescampo": true}),
        );
        f.add_record_row(record_report("member_atividades"), &a)
            .unwrap();
        let a = &f.records["member_atividades"][&5];
        assert_eq!(
            (a["noites"].clone(), a["noites_campo"].clone()),
            (json!(-3), json!(2))
        );
    }

    #[test]
    fn record_changes_log_additions_edits_and_removals() {
        let r = record_report("member_cargos");
        let rec = |ended: Option<&str>| {
            obj(
                json!({"siie_id": 77, "nin": "1", "import_agrupamento": 455, "cargo": "CU",
                "sigla_cargo": "CU", "agrupamento": 455, "started_on": "2020-09-01",
                "ended_on": ended, "active": ended.is_none()}),
            )
        };
        let none = BTreeMap::new();
        let new = BTreeMap::from([(77, rec(None))]);
        let added = record_changes(r, &none, &new, &[], &|_| true);
        assert_eq!(
            (added[0].field.as_str(), added[0].new_value.as_deref()),
            ("cargo.77", Some("CU"))
        );
        // Not for a member seen for the first time.
        assert!(record_changes(r, &none, &new, &[], &|_| false).is_empty());

        let old = BTreeMap::from([(77, rec(None))]);
        let ended = BTreeMap::from([(77, rec(Some("2025-09-01")))]);
        let fields: Vec<String> = record_changes(r, &old, &ended, &[], &|_| true)
            .into_iter()
            .map(|c| c.field)
            .collect();
        assert_eq!(fields, vec!["cargo.77.active", "cargo.77.ended_on"]);

        let gone = record_changes(r, &old, &none, &[77], &|_| true);
        assert_eq!(
            (gone[0].field.as_str(), gone[0].old_value.as_deref()),
            ("cargo.77", Some("CU"))
        );
    }
}
