//! Access: what each account may see, read from the SIIE grid and kept for
//! ACCESS_VALIDITY_DAYS.
//!
//! - At login, `claim_refresh` decides whether the access must be re-read (never read, or
//!   expired) and, if so, marks it 'running' so two logins don't read twice.
//! - `read_access` then runs after the login response is sent (see `in_background`). It holds
//!   the SIIE session only in memory and drops it when done.
//! - Until a re-read finishes, the previous grants stay in force (stale while refreshing).
//! - The expiry is not stored: `refreshed_at + ACCESS_VALIDITY_DAYS`, so changing the setting
//!   applies to everyone at once.

use reqwest::Client;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState,
    admin::log_event,
    auth::{Method, login_key},
    siie::{self, LoginOk, OwnId, Visibility},
};

// A run that started longer ago than this died with its function (maxDuration is 300 s on
// Hobby), so the next login may claim it again.
const STALE_RUN_MINUTES: i32 = 10;

/// What a login found out about the account's access.
pub enum Claim {
    /// Read within the validity period: nothing to do.
    Fresh,
    /// Never read or expired: this login reads it in the background.
    Started,
    /// Never read or expired, but another login is already reading it.
    AlreadyRunning,
}

impl Claim {
    /// For the "login" event detail.
    pub fn as_str(&self) -> &'static str {
        match self {
            Claim::Fresh => "fresh",
            Claim::Started => "started",
            Claim::AlreadyRunning => "already_running",
        }
    }
}

/// Decides whether this login must read the access, and claims the read if so.
pub async fn claim_refresh(
    db: &PgPool,
    account_id: Uuid,
    validity_days: i32,
) -> Result<Claim, sqlx::Error> {
    // Every account gets its access row on first login.
    sqlx::query("insert into account_access (account_id) values ($1) on conflict do nothing")
        .bind(account_id)
        .execute(db)
        .await?;

    // Claim = flip to 'running', only when (a) never read or expired and (b) nobody else is
    // reading right now (or their run is stale). `fresh` is read from the same snapshot.
    let (claimed, fresh): (bool, bool) = sqlx::query_as(
        "with claimed as (
            update account_access
            set refresh_status = 'running', refresh_started_at = now()
            where account_id = $1
              and (refreshed_at is null or refreshed_at + make_interval(days => $2) < now())
              and (refresh_status <> 'running'
                   or refresh_started_at < now() - make_interval(mins => $3))
            returning 1
         )
         select exists (select 1 from claimed),
                coalesce(refreshed_at + make_interval(days => $2) >= now(), false)
         from account_access where account_id = $1",
    )
    .bind(account_id)
    .bind(validity_days)
    .bind(STALE_RUN_MINUTES)
    .fetch_one(db)
    .await?;

    Ok(if claimed {
        Claim::Started
    } else if fresh {
        Claim::Fresh
    } else {
        Claim::AlreadyRunning
    })
}

/// The background read: whole grid -> structure + grants + level. Never panics the caller;
/// failures are written to `account_access` so the Painel can show them.
pub async fn read_access(
    state: AppState,
    account_id: Uuid,
    method: Method,
    client: Client,
    login: LoginOk,
    own: OwnId,
) {
    let read = siie::visibility(&client, &login.token, &own).await;
    // The SIIE session ends here, before any DB work.
    drop(login);
    drop(client);
    drop(own);

    let failure = match read {
        Ok(vis) => match store(&state, account_id, method, vis).await {
            Ok(()) => return,
            Err(e) => {
                // Logged for us (no personal data in SQL errors here: only codes and ids).
                eprintln!("access: storing account {account_id} failed: {e}");
                "Erro na base de dados ao guardar o acesso.".to_owned()
            }
        },
        // SiieError messages hold organisation codes and column names, never member data.
        Err(e) => e.to_string(),
    };

    let marked = sqlx::query(
        "update account_access set refresh_status = 'failed', refresh_error = $2
         where account_id = $1",
    )
    .bind(account_id)
    .bind(&failure)
    .execute(&state.db)
    .await;
    let logged = log_event(&state.db, None, account_id, "access_failed", json!({})).await;
    if let Err(e) = marked.and(logged) {
        eprintln!("access: recording the failure for account {account_id} failed: {e}");
    }
}

/// Writes one successful read in a single transaction. Arrays are unnested in sorted order (they
/// come from BTree maps), so two accounts refreshing overlapping structure lock rows in the same
/// order and can't deadlock.
async fn store(
    state: &AppState,
    account_id: Uuid,
    method: Method,
    mut vis: Visibility,
) -> Result<(), sqlx::Error> {
    let level = vis.level();
    let grants = vis.grants();
    // Hashed right away; the plain identifier goes no further.
    let other_key = vis
        .other_identifier
        .take()
        .map(|o| login_key(state.login_key_secret.expose_secret(), &o));

    // Structure as parallel arrays for `unnest`.
    let regioes: Vec<i16> = vis.regioes.iter().copied().collect();
    let (nucleo_codes, nucleo_regioes): (Vec<i16>, Vec<Option<i16>>) =
        vis.nucleos.iter().map(|(&n, &r)| (n, r)).unzip();
    let mut agr_codes = Vec::new();
    let mut agr_regioes = Vec::new();
    let mut agr_nucleos = Vec::new();
    let mut agr_names = Vec::new();
    let (mut sec_agr, mut sec_tipo, mut sec_numero) = (Vec::new(), Vec::new(), Vec::new());
    for (&code, seen) in &vis.agrupamentos {
        agr_codes.push(code);
        agr_regioes.push(seen.regiao);
        agr_nucleos.push(seen.nucleo);
        agr_names.push(seen.name.clone());
        for s in &seen.seccoes {
            sec_agr.push(code);
            sec_tipo.push(s.tipo);
            sec_numero.push(s.numero);
        }
    }
    let grant_agr: Vec<i16> = grants.iter().map(|g| g.agrupamento).collect();
    let grant_tipo: Vec<Option<i16>> = grants.iter().map(|g| g.seccao.map(|s| s.tipo)).collect();
    let grant_numero: Vec<Option<i16>> =
        grants.iter().map(|g| g.seccao.map(|s| s.numero)).collect();

    let mut tx = state.db.begin().await?;

    // 1. Structure, parents first (FKs). Known values win over unknown ones on conflict.
    sqlx::query(
        "insert into regioes (code) select unnest($1::smallint[])
         on conflict (code) do update set seen_at = now()",
    )
    .bind(&regioes)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "insert into nucleos (code, regiao) select * from unnest($1::smallint[], $2::smallint[])
         on conflict (code) do update
         set regiao = coalesce(excluded.regiao, nucleos.regiao), seen_at = now()",
    )
    .bind(&nucleo_codes)
    .bind(&nucleo_regioes)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "insert into agrupamentos (code, regiao, nucleo, name)
         select * from unnest($1::smallint[], $2::smallint[], $3::smallint[], $4::text[])
         on conflict (code) do update
         set regiao = coalesce(excluded.regiao, agrupamentos.regiao),
             nucleo = coalesce(excluded.nucleo, agrupamentos.nucleo),
             name = coalesce(excluded.name, agrupamentos.name),
             seen_at = now()",
    )
    .bind(&agr_codes)
    .bind(&agr_regioes)
    .bind(&agr_nucleos)
    .bind(&agr_names)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "insert into seccoes (agrupamento, tipo, numero)
         select * from unnest($1::smallint[], $2::smallint[], $3::smallint[])
         on conflict (agrupamento, tipo, numero) do update set seen_at = now()",
    )
    .bind(&sec_agr)
    .bind(&sec_tipo)
    .bind(&sec_numero)
    .execute(&mut *tx)
    .await?;

    // 2. Grants: replaced wholesale, so anything SIIE no longer shows is gone.
    sqlx::query("delete from account_grants where account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "insert into account_grants (account_id, agrupamento, seccao_tipo, seccao_numero)
         select $1, * from unnest($2::smallint[], $3::smallint[], $4::smallint[])",
    )
    .bind(account_id)
    .bind(&grant_agr)
    .bind(&grant_tipo)
    .bind(&grant_numero)
    .execute(&mut *tx)
    .await?;

    // 3. Access state. Upsert, so a read never writes nothing even without a prior claim row.
    sqlx::query(
        "insert into account_access (account_id, level, refreshed_at)
         values ($1, $2, now())
         on conflict (account_id) do update
         set level = excluded.level, refreshed_at = now(),
             refresh_status = 'idle', refresh_error = null",
    )
    .bind(account_id)
    .bind(level.as_str())
    .execute(&mut *tx)
    .await?;

    // 4. NIF/NIN link, only if this account has no such identifier yet and no other account uses
    //    it (an admin may have invited the same person twice; that's theirs to sort out).
    if let Some(other_key) = other_key {
        let other = method.other();
        let col = other.key_column();
        let linked = sqlx::query(&format!(
            "update accounts set {col} = $2
             where id = $1 and {col} is null
               and not exists (select 1 from accounts where {col} = $2)"
        ))
        .bind(account_id)
        .bind(&other_key)
        .execute(&mut *tx)
        .await?;
        if linked.rows_affected() == 1 {
            let detail = json!({ "kind": other.kind() });
            log_event(&mut *tx, None, account_id, "identifier_linked", detail).await?;
        }
    }

    // 5. Event: how wide the access was, not where.
    let seccoes: usize = vis.agrupamentos.values().map(|a| a.seccoes.len()).sum();
    let detail = json!({
        "accessLevel": level.as_str(),
        "agrupamentos": vis.agrupamentos.len(),
        "nucleos": vis.nucleos.len(),
        "regioes": vis.regioes.len(),
        "seccoes": seccoes,
    });
    log_event(&mut *tx, None, account_id, "access_refreshed", detail).await?;

    tx.commit().await
}

/// One `account_access` row, as /api/auth/me needs it (times in Unix seconds).
#[derive(sqlx::FromRow)]
struct AccessRow {
    level: String,
    refreshed_at: Option<i64>,
    valid_until: Option<i64>,
    refresh_status: String,
    refresh_error: Option<String>,
    expired: bool,
}

/// One grant with its place in the structure.
#[derive(sqlx::FromRow)]
struct GrantRow {
    agrupamento: i16,
    name: Option<String>,
    nucleo: Option<i16>,
    regiao: Option<i16>,
    seccao_tipo: Option<i16>,
    seccao_numero: Option<i16>,
}

/// The `access` part of /api/auth/me.
pub async fn summary(state: &AppState, account_id: Uuid) -> Result<Value, sqlx::Error> {
    let row: Option<AccessRow> = sqlx::query_as(
        "select level,
                extract(epoch from refreshed_at)::bigint as refreshed_at,
                extract(epoch from refreshed_at + make_interval(days => $2))::bigint as valid_until,
                refresh_status, refresh_error,
                coalesce(refreshed_at + make_interval(days => $2) < now(), false) as expired
         from account_access where account_id = $1",
    )
    .bind(account_id)
    .bind(state.access_validity_days)
    .fetch_optional(&state.db)
    .await?;
    let row = row.unwrap_or(AccessRow {
        level: "none".into(),
        refreshed_at: None,
        valid_until: None,
        refresh_status: "idle".into(),
        refresh_error: None,
        expired: false,
    });

    // One status for the UI. "running"/"failed" win; otherwise never read, expired or ready.
    // An expired access still works until the next login re-reads it.
    let status = match row.refresh_status.as_str() {
        "running" => "running",
        "failed" => "failed",
        _ if row.refreshed_at.is_none() => "never",
        _ if row.expired => "expired",
        _ => "ready",
    };

    let grants: Vec<GrantRow> = sqlx::query_as(
        "select g.agrupamento, a.name, a.nucleo, a.regiao, g.seccao_tipo, g.seccao_numero
         from account_grants g join agrupamentos a on a.code = g.agrupamento
         where g.account_id = $1
         order by g.agrupamento, g.seccao_tipo, g.seccao_numero",
    )
    .bind(account_id)
    .fetch_all(&state.db)
    .await?;

    // Núcleos and regiões follow from the agrupamentos (via the structure tables).
    let mut nucleos: Vec<i16> = grants.iter().filter_map(|g| g.nucleo).collect();
    let mut regioes: Vec<i16> = grants.iter().filter_map(|g| g.regiao).collect();
    nucleos.sort_unstable();
    nucleos.dedup();
    regioes.sort_unstable();
    regioes.dedup();

    let agrupamentos: Vec<Value> = grants
        .iter()
        .map(|g| {
            let seccao = g
                .seccao_tipo
                .zip(g.seccao_numero)
                .map(|(tipo, numero)| json!({ "tipo": tipo, "numero": numero }));
            json!({
                "code": g.agrupamento,
                "name": g.name,
                "nucleo": g.nucleo,
                "regiao": g.regiao,
                "seccao": seccao,
            })
        })
        .collect();

    Ok(json!({
        "level": row.level,
        "status": status,
        "refreshedAt": row.refreshed_at,
        "validUntil": row.valid_until,
        "error": row.refresh_error,
        "agrupamentos": agrupamentos,
        "nucleos": nucleos,
        "regioes": regioes,
    }))
}

/// Database tests: run against DATABASE_URL (local yasa_dev) with `cargo test -- --ignored`.
/// They use a throwaway account and structure codes in the 9xxx range, and clean up after.
#[cfg(test)]
mod db_tests {
    use super::*;

    const TEST_AGRS: [i16; 2] = [9001, 9002];
    const TEST_NUCLEO: i16 = 9901;
    const TEST_REGIAO: i16 = 99;

    async fn setup() -> (AppState, Uuid) {
        dotenvy::dotenv().ok();
        let state = AppState::from_env(false).expect("DATABASE_URL and LOGIN_KEY_SECRET");
        cleanup(&state, None).await;
        // A NIF-only account, so the NIN found in its own row can be linked.
        let (id,): (Uuid,) = sqlx::query_as(
            "insert into accounts (nif_key, label) values ('\\x01', 'access db test') returning id",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        (state, id)
    }

    async fn cleanup(state: &AppState, id: Option<Uuid>) {
        let db = &state.db;
        let q = |sql: &'static str| sqlx::query(sql);
        if let Some(id) = id {
            q("delete from accounts where id = $1")
                .bind(id)
                .execute(db)
                .await
                .unwrap();
        }
        q("delete from accounts where label = 'access db test'")
            .execute(db)
            .await
            .unwrap();
        q("delete from seccoes where agrupamento >= 9000")
            .execute(db)
            .await
            .unwrap();
        q("delete from agrupamentos where code >= 9000")
            .execute(db)
            .await
            .unwrap();
        q("delete from nucleos where code >= 9000")
            .execute(db)
            .await
            .unwrap();
        q("delete from regioes where code = 99")
            .execute(db)
            .await
            .unwrap();
    }

    fn visibility(agrupamentos: &[i16]) -> Visibility {
        let own = OwnId {
            loja_escutista: true,
            identifier: "999999999".to_owned(),
        };
        let mut vis = Visibility::default();
        for (i, a) in agrupamentos.iter().enumerate() {
            // The first row is the person's own: its NIN should get linked.
            let row = json!({
                "agrupamento": format!("{a:04}"), "nucleo": TEST_NUCLEO.to_string(),
                "regiao": format!("{TEST_REGIAO:02}"), "agrupdesc": format!("Teste {a}"),
                "nif": if i == 0 { "999999999" } else { "111" }, "nin": "8888",
            });
            vis.add_row(row.as_object().unwrap(), &own).unwrap();
        }
        vis
    }

    async fn count(state: &AppState, sql: &str, id: Uuid) -> i64 {
        let (n,): (i64,) = sqlx::query_as(sql)
            .bind(id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        n
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn store_writes_structure_grants_access_and_link() {
        let (state, id) = setup().await;

        store(&state, id, Method::Loja, visibility(&TEST_AGRS))
            .await
            .unwrap();

        // Structure: both agrupamentos under the núcleo and região, with names.
        let (n, nucleo, regiao): (i64, Option<i16>, Option<i16>) = sqlx::query_as(
            "select count(*), min(nucleo), min(regiao) from agrupamentos
             where code = any($1) and name like 'Teste %'",
        )
        .bind(&TEST_AGRS[..])
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(
            (n, nucleo, regiao),
            (2, Some(TEST_NUCLEO), Some(TEST_REGIAO))
        );
        // Grants: two whole agrupamentos.
        let grants =
            "select count(*) from account_grants where account_id = $1 and seccao_tipo is null";
        assert_eq!(count(&state, grants, id).await, 2);
        // Access state and the NIN link.
        let (level, status, has_nin): (String, String, bool) = sqlx::query_as(
            "select x.level, x.refresh_status, a.nin_key is not null
             from account_access x join accounts a on a.id = x.account_id where a.id = $1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(
            (level.as_str(), status.as_str(), has_nin),
            ("nucleo", "idle", true)
        );
        let events = "select count(*) from account_events where account_id = $1
                      and action in ('access_refreshed', 'identifier_linked')";
        assert_eq!(count(&state, events, id).await, 2);

        // Summary as /api/auth/me shows it.
        let s = summary(&state, id).await.unwrap();
        assert_eq!(s["status"], "ready");
        assert_eq!(s["level"], "nucleo");
        assert_eq!(s["agrupamentos"].as_array().unwrap().len(), 2);
        assert_eq!(s["agrupamentos"][0]["name"], "Teste 9001");
        assert_eq!(s["nucleos"], json!([TEST_NUCLEO]));
        assert_eq!(s["regioes"], json!([TEST_REGIAO]));
        let valid = s["validUntil"].as_i64().unwrap() - s["refreshedAt"].as_i64().unwrap();
        assert_eq!(valid, i64::from(state.access_validity_days) * 86_400);

        // A later read with less access replaces the grants (and keeps the structure).
        store(&state, id, Method::Loja, visibility(&TEST_AGRS[..1]))
            .await
            .unwrap();
        assert_eq!(count(&state, grants, id).await, 1);
        assert_eq!(summary(&state, id).await.unwrap()["level"], "agrupamento");

        cleanup(&state, Some(id)).await;
    }

    #[tokio::test]
    #[ignore = "needs DATABASE_URL: cargo test -- --ignored"]
    async fn claim_refresh_lifecycle() {
        let (state, id) = setup().await;
        let days = state.access_validity_days;
        let db = state.db.clone();
        let claim = || claim_refresh(&db, id, days);
        let set = |sql: &'static str| sqlx::query(sql).bind(id);

        // Never read: this login reads it; a second login meanwhile doesn't.
        assert!(matches!(claim().await.unwrap(), Claim::Started));
        assert_eq!(summary(&state, id).await.unwrap()["status"], "running");
        assert!(matches!(claim().await.unwrap(), Claim::AlreadyRunning));

        // Read done: fresh until it expires.
        store(&state, id, Method::Loja, visibility(&TEST_AGRS))
            .await
            .unwrap();
        assert!(matches!(claim().await.unwrap(), Claim::Fresh));

        // Expired: the old grants stay while this login re-reads.
        set("update account_access set refreshed_at = now() - interval '400 days' where account_id = $1")
            .execute(&state.db).await.unwrap();
        assert_eq!(summary(&state, id).await.unwrap()["status"], "expired");
        assert!(matches!(claim().await.unwrap(), Claim::Started));
        assert_eq!(
            summary(&state, id).await.unwrap()["agrupamentos"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        // A run that died (started 11 minutes ago) is reclaimed.
        set("update account_access set refresh_started_at = now() - interval '11 minutes' where account_id = $1")
            .execute(&state.db).await.unwrap();
        assert!(matches!(claim().await.unwrap(), Claim::Started));

        cleanup(&state, Some(id)).await;
    }
}
