-- Accounts. SIIE is the identity provider (no passwords here) and access is invite-only.
-- A YASA account and a SIIE identity are 1:1, so both halves live in the same row.

create table accounts (
    id uuid primary key default gen_random_uuid(),
    -- HMAC-SHA256(LOGIN_KEY_SECRET, identifier), never the identifier itself.
    -- One person may log in with the NIF (Loja Escutista) or the NIN (SIIE), so either can match.
    nif_key bytea unique,
    nin_key bytea unique,
    -- Admin's own note to recognise the person, e.g. "Joana, Agr. 123".
    label text not null,
    role text not null default 'member' check (role in ('member', 'admin')),
    status text not null default 'active' check (status in ('active', 'disabled')),
    invited_by uuid references accounts (id),
    created_at timestamptz not null default now(),
    -- null = invited but never logged in.
    last_login_at timestamptz,
    -- Access, as SIIE showed it at the last login. Both are replaced on every login, so access
    -- in YASA follows access in SIIE. Data queries filter on `agrupamentos`; the level is a
    -- summary of how wide that list is (see siie::Visibility::level).
    access_level text not null default 'none'
        check (access_level in ('none', 'agrupamento', 'nucleo', 'regiao', 'nacional')),
    agrupamentos text[] not null default '{}',
    access_refreshed_at timestamptz,
    check (nif_key is not null or nin_key is not null)
);

create table sessions (
    -- sha256 of the cookie value: a leaked table can't be replayed as cookies.
    token_hash bytea primary key,
    account_id uuid not null references accounts (id) on delete cascade,
    created_at timestamptz not null default now(),
    -- Hard cap; the cookie itself ends with the browser session.
    expires_at timestamptz not null
);

create index sessions_expires_at on sessions (expires_at);
