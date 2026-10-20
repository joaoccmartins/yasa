# YASA (Yet Another Scout Aggregator)

A data analysis tool for scouting data from SIIE (https://siie.escutismo.pt). You log in with
your own SIIE account, and YASA shows only the data that account gives you access to.

Status: infrastructure and accounts are done (invite-only, SIIE-verified logins, scope from
SIIE, Access page for admins). Data sync, anonymization and charts come next.

## Architecture

```
Browser ──> Vercel (Hobby, region fra1)
            ├─ /*       web/dist       Vite + React + TS (static)
            └─ /api/*   api/yasa.rs    one Rust function (axum on vercel_runtime)
                           ├─> SIIE    login check + scope, with the user's credentials
                           └─> Neon Postgres (Frankfurt)
```

- Frontend and API share one domain, so there is no CORS and the session cookie is first-party.
- The axum router lives in [src/lib.rs](src/lib.rs). [api/yasa.rs](api/yasa.rs) serves it on
  Vercel, and [src/bin/dev.rs](src/bin/dev.rs) serves the same router locally.

## Access model

SIIE is the identity provider: YASA has no passwords. A YASA account and a SIIE identity are
1:1, so both halves live in one `accounts` row: the SIIE link (the NIF and/or NIN, stored only as
`HMAC-SHA256(LOGIN_KEY_SECRET, identifier)`) plus YASA fields (label, role, status). Having a SIIE
login is not enough: an admin has to invite you.

`POST /api/auth/login` `{identifier, password, method: "loja" | "siie"}`:

```
0. login limits                       too many recent SIIE failures -> 429, SIIE not contacted
1. SIIE login                         fails -> 401 (also covers "removed from SIIE")
2. HMAC(identifier) -> account        by nif_key (Loja) or nin_key (SIIE)
                                      none / disabled -> 403, and nothing is stored
3. access never read, or expired?      claim the read and run it in the background
                                      (the login does not wait for it, see "Access" below)
4. session + "login" event            cookie ends with the browser, DB caps it at 12h; 204 now
```

SIIE is checked before the invite list, so the endpoint can't be used to probe which NIFs are
invited. The SIIE password is dropped as soon as SIIE answers; the SIIE session (cookies + token)
lives only in memory, until the background read ends or, if none is needed, the request ends.

**Login limits.** Each SIIE rejection is recorded in `login_failures` under an HMAC of the
identifier and an HMAC of the client IP (`x-forwarded-for`, which Vercel sets). 5 failures for one
identifier, or 20 from one IP, within 15 minutes return 429 without contacting SIIE, so YASA can't
be used to guess SIIE passwords. A correct password clears that identifier's failures. SIIE outages
(502) don't count. Limits are constants at the top of [src/auth.rs](src/auth.rs).

**NIF and NIN, one account.** If the person appears in their own grid (a row whose `nif` or `nin`
equals what they typed), the other identifier is hashed onto their account. From then on both
login options reach it. It's skipped if another account already uses that identifier.

| What happens | Effect in YASA |
| --- | --- |
| Removed from SIIE | The next login fails at step 1 |
| SIIE shows fewer agrupamentos | The first login after the access expires re-reads it and shrinks the grants |
| An admin disables the account | Its sessions are deleted at once |
| Too many wrong passwords | 429 for 15 minutes, per identifier and per IP |
| Browser closes | The cookie is gone (server cap: 12h) |

### Access

**Background read.** Reading the whole SIIE grid takes seconds (minutes for wide access), so the
login doesn't wait for it. On Vercel, `vercel_runtime`'s `wait_until` keeps the function running
after the response until the read finishes (bounded by maxDuration, 300 s on Hobby). The crate's
axum adapter drops the state that carries it, so [api/yasa.rs](api/yasa.rs) copies it into the
request extensions first. Locally it's a plain `tokio::spawn` (`in_background` in
[src/lib.rs](src/lib.rs)). The Painel shows "a ser processado" and polls `/api/auth/me` every 3 s
until the read is done.

**Validity.** A read stays valid for `ACCESS_VALIDITY_DAYS` days (env var, default 7). Only the
read time is stored; the expiry is computed, so changing the variable applies to everyone at once.
The first login after expiry re-reads it, and the old grants stay in force until the new read
finishes. A claim (`refresh_status = 'running'`) stops two logins reading at once; a run that
started more than 10 minutes ago is considered dead and can be claimed again.

**Level.** SIIE doesn't expose the role, so it is read from how widely the grid's rows spread
(`siie::Visibility::level`), checked in this order:
1. more than 1 região: nacional
2. more than 1 núcleo: regiao
3. exactly 1 agrupamento: **seccao** if all its rows sit in one single secção, else agrupamento
   (checked before núcleo, since one agrupamento also means one núcleo)
4. 1 núcleo, or several agrupamentos: nucleo
5. otherwise: none

All-zero codes (região `00`, núcleo or agrupamento `0000`) are SIIE's way of saying "none" and are
skipped. Blind spots: núcleo access to a núcleo with a single agrupamento reads as "agrupamento";
an agrupamento whose only rows are one secção reads as "seccao". The grants are exact either way.

**Secções.** Types: 1 Alcateia (Lobitos), 2 Expedição (Exploradores), 3 Comunidade (Pioneiros),
4 Clã (Caminheiros); `numero` tells apart two of the same type in one agrupamento (ultra rare).
How the grid's `seccao` / `siglaseccao` columns encode them is still to be confirmed (POC page,
"Secções nesta página"); until `siie::seccao_from_row` is filled in, no secções are collected and
every grant is a whole agrupamento.

**Tables** (migration 0004). All organisation codes are `smallint`; SIIE's zero-padded strings are
parsed on the way in and padded again only for display.

| Table | Holds |
| --- | --- |
| `regioes`, `nucleos`, `agrupamentos`, `seccoes` | The organisation tree as SIIE shows it (codes, parents, agrupamento names). Reference data, no personal data, upserted by every read. Basis for núcleo/região digests. |
| `account_access` | Per account: `level`, `refreshed_at`, `refresh_status` (idle/running/failed), `refresh_error`. |
| `account_grants` | Per account: one row per whole agrupamento (`seccao_*` null), or one row for a single secção. **Data queries must filter on this table.** An account's núcleos and regiões follow from it via `agrupamentos`. |

Endpoints:

| Route | Who | What |
| --- | --- | --- |
| `POST /api/auth/login`, `POST /api/auth/logout` | anyone | see above |
| `GET /api/auth/me` | logged in | `{accountId, label, role, access: {level, status, refreshedAt, validUntil, error, agrupamentos[], nucleos[], regioes[]}}` or 401 |
| `GET /api/admin/accounts` | admin | everyone with access, last login, access level, status, validity, agrupamento/núcleo/região counts |
| `POST /api/admin/accounts` | admin | invite `{method, identifier, label, role}`; 409 if already invited |
| `PATCH /api/admin/accounts/{id}` | admin | `{status?, role?}`; refuses to leave YASA without an active admin |
| `GET /api/admin/events` | admin | the latest 100 events, newest first |

**History.** `account_events` records invites, logins (with the access level and counts they got),
enable/disable, role changes and identifier links, each with who did it (`actor_id`; null = the
`invite` command or the system). `detail` holds role names and counts only, never identifiers.
The Acessos page shows it as "Histórico".

In Rust, handlers take `CurrentAccount` or `AdminAccount` ([src/auth.rs](src/auth.rs)) to require
a login or an admin.

The first admin has to come from the command line (afterwards, use the Access page):

```sh
cargo run --bin invite -- --method loja --identifier <NIF> --label "João" --admin
```

Access is read from the grid columns `agrupamento`, `nucleo` and `regiao` (constants at the top
of [src/siie.rs](src/siie.rs)). If SIIE renames them, login fails with an error that lists the
grid's real field names.

## Local development

Needs Rust, Node 24 and a Postgres (local, or a Neon dev branch).

```sh
cp .env.example .env              # fill in DATABASE_URL and LOGIN_KEY_SECRET (openssl rand -hex 32)
cargo run --bin migrate           # applies migrations/
cargo run --bin invite -- --method loja --identifier <your NIF> --label "Me" --admin
cargo run --bin dev               # API on http://127.0.0.1:3000
cd web && npm install && npm run dev   # UI on http://localhost:5173, proxies /api to :3000
```

The original POC grid explorer still runs with `cargo run --bin poc` (http://127.0.0.1:3000,
so stop `dev` first).

Checks (the same as CI):

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd web && npm run lint && npm run build
```

Database tests (store, claim lifecycle) need a migrated local database and are skipped by default:
`cargo test -- --ignored --test-threads=1`.

## Deploy (Vercel + Neon)

One-time setup:

1. Push this repo to GitHub and import it in Vercel. Leave **Root Directory as the repo root**
   (not `web`): the Rust function lives in `api/` and `vercel.json` builds `web/` itself. It also
   sets the build, the `fra1` region and the rewrites, so the framework preset doesn't matter.
2. In the Vercel project: Storage > Neon, region `aws-eu-central-1` (Frankfurt). This adds
   `DATABASE_URL` (pooled) and `DATABASE_URL_UNPOOLED`.
3. `vercel env add LOGIN_KEY_SECRET` for Production and Preview (`openssl rand -hex 32`).
   Keep it stable: changing it makes every invite stop matching.
4. Run the migrations against Neon: `vercel env pull .env` then `cargo run --bin migrate`.
5. Invite the first admin against Neon with `cargo run --bin invite` (same `.env`).

After that, every push to `main` deploys to production and other branches get preview URLs.
Migrations are never run by the function itself. Run `cargo run --bin migrate` before deploying
code that needs a new migration.

### Known risks

- The Rust runtime on Vercel is in public beta.
- SIIE has bot detection and might block Vercel's IP ranges. If login works locally but fails on
  Vercel, run the same router as a container instead (Render or Koyeb free tier): build
  `src/bin/dev.rs` with the bind address set to `0.0.0.0:$PORT`, and keep the Vite site on Vercel
  with a rewrite from `/api/*` to the container.
- The Hobby plan is for non-commercial use.

## If SIIE changes

All SIIE constants are at the top of [src/siie.rs](src/siie.rs):

| Constant | What it is |
| --- | --- |
| `TOKEN_PATH` | Login endpoint (`/api/logintoken`) |
| `USER_FIELD`, `PASS_FIELD`, `ADAUTH_FIELD` | Field names `TokenLogin()` posts. `ADAUTH_FIELD` is `true` for Loja Escutista (NIF) and `false` for SIIE (NIN) |
| `LOGIN_PAGE`, `LOGIN_PATH` | Login page URL, and the path used to detect a bounce to login |
| `GRID_PAGE` | Page hosting the grid (used by the POC) |
| `GRID_API_MARKER` | Text searched in that page to find the encrypted report id |
| `USER_AGENT` | Desktop browser UA sent on every request (the site has bot detection) |

To re-check the login flow, open the login page source, find `/Scripts/xSIIE.<version>.js` and
read the `TokenLogin` function. Notes from the POC:

- The login `<form>` inputs have no `name`, so posting the form never logs you in. The real login
  is `POST /api/logintoken`.
- Bad credentials return HTTP 404 with "O Utilizador ou password inseridos não são válidos."
- The grid's `xml` report id is encrypted per session. A value copied from the browser fails with
  "Padding is invalid and cannot be removed."
