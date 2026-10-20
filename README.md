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
1. SIIE login                         fails -> 401 (also covers "removed from SIIE")
2. HMAC(identifier) -> account        by nif_key (Loja) or nin_key (SIIE)
                                      none / disabled -> 403, and nothing is stored
3. read the members grid (all pages)  fails -> 502 (can't verify access = no access)
   -> agrupamentos + access_level     replace them on the account
4. session                            cookie ends with the browser, DB caps it at 12h
```

SIIE is checked before the invite list, so the endpoint can't be used to probe which NIFs are
invited. The SIIE password and token are dropped at the end of the request.

| What happens | Effect in YASA |
| --- | --- |
| Removed from SIIE | The next login fails at step 1 |
| SIIE shows fewer agrupamentos | The next login shrinks `agrupamentos` (and maybe the level) |
| An admin disables the account | Its sessions are deleted at once |
| Browser closes | The cookie is gone (server cap: 12h) |

Each account's access is two fields, both refreshed from SIIE at every login:

- `agrupamentos` (`text[]`): the agrupamento numbers in the grid SIIE shows them. Access is cut on
  this list. The data phase must filter every query with `agrupamento = any(<account's list>)`.
- `access_level`: `none`, `agrupamento`, `nucleo`, `regiao` or `nacional`. SIIE doesn't expose the
  role, so it is read from how widely the visible rows spread (`siie::Visibility::level`),
  checked in this order:
  1. more than 1 região: nacional
  2. more than 1 núcleo: regiao
  3. exactly 1 agrupamento: agrupamento (before núcleo, since one agrupamento also means one núcleo)
  4. 1 núcleo, or several agrupamentos: nucleo
  5. otherwise: none

  All-zero codes (região `00`, núcleo or agrupamento `0000`) are SIIE's way of saying "none" and
  are skipped everywhere, including the lists. Blind spot: núcleo access to a núcleo with a single
  agrupamento reads as "agrupamento"; the agrupamentos list is exact either way.
- `nucleos`, `regioes` (`text[]`): the núcleo and região codes in the same grid. They are kept so
  the level can be checked: the dashboard shows all three lists, and the Access page shows the counts.

Endpoints:

| Route | Who | What |
| --- | --- | --- |
| `POST /api/auth/login`, `POST /api/auth/logout` | anyone | see above |
| `GET /api/auth/me` | logged in | `{accountId, label, role, accessLevel, agrupamentos, nucleos, regioes}` or 401 |
| `GET /api/admin/accounts` | admin | everyone with access, last login, access level, agrupamento/núcleo/região counts |
| `POST /api/admin/accounts` | admin | invite `{method, identifier, label, role}`; 409 if already invited |
| `PATCH /api/admin/accounts/{id}` | admin | `{status?, role?}`; refuses to leave YASA without an active admin |

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
