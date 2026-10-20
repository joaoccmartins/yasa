-- Access as structure instead of flat arrays: the organisation tree (regiões, núcleos,
-- agrupamentos, secções) plus, per account, its access state and grants.
-- All organisation codes are smallint: SIIE's zero-padded strings ("0053") are parsed on the way
-- in, and padded again only for display.

-- Organisation structure as SIIE shows it. Reference data, no personal data. Upserted by every
-- access read, so it grows to cover every unit any YASA user can see. Needed for núcleo/região
-- digests and to derive an account's núcleos/regiões from its agrupamentos.
create table regioes (
    code smallint primary key,
    seen_at timestamptz not null default now()
);

create table nucleos (
    code smallint primary key,
    regiao smallint references regioes (code),
    seen_at timestamptz not null default now()
);

create table agrupamentos (
    code smallint primary key,
    regiao smallint references regioes (code),
    -- null: a região without núcleos (the agrupamento hangs directly from it).
    nucleo smallint references nucleos (code),
    -- grid `agrupdesc`
    name text,
    seen_at timestamptz not null default now()
);

-- Secção types: 1 Alcateia (Lobitos), 2 Expedição (Exploradores), 3 Comunidade (Pioneiros),
-- 4 Clã (Caminheiros). An agrupamento may have several of one type (ultra rare): `numero` tells
-- them apart, 1 for the usual single one.
create table seccoes (
    agrupamento smallint not null references agrupamentos (code),
    tipo smallint not null check (tipo between 1 and 4),
    numero smallint not null default 1,
    seen_at timestamptz not null default now(),
    primary key (agrupamento, tipo, numero)
);

-- Per account: the state of its access. The expiry is not stored: it is
-- refreshed_at + ACCESS_VALIDITY_DAYS, so changing that setting applies to everyone at once.
create table account_access (
    account_id uuid primary key references accounts (id) on delete cascade,
    level text not null default 'none'
        check (level in ('none', 'seccao', 'agrupamento', 'nucleo', 'regiao', 'nacional')),
    -- Last successful read of the SIIE grid; null = never read.
    refreshed_at timestamptz,
    -- 'running' while a background read is in progress (claimed at login).
    refresh_status text not null default 'idle'
        check (refresh_status in ('idle', 'running', 'failed')),
    refresh_started_at timestamptz,
    -- Last failure message (SIIE / grid problem). Never personal data.
    refresh_error text
);

-- What the account may see. Data queries filter on this, and only this.
create table account_grants (
    account_id uuid not null references accounts (id) on delete cascade,
    agrupamento smallint not null references agrupamentos (code),
    -- Both null = the whole agrupamento; both set = only that secção.
    seccao_tipo smallint,
    seccao_numero smallint,
    check ((seccao_tipo is null) = (seccao_numero is null)),
    foreign key (agrupamento, seccao_tipo, seccao_numero)
        references seccoes (agrupamento, tipo, numero)
);
-- One whole-agrupamento grant, or one row per allowed secção. Two partial indexes instead of
-- `unique nulls not distinct`, which needs Postgres 15 (local dev runs 13).
create unique index account_grants_whole on account_grants (account_id, agrupamento)
    where seccao_tipo is null;
create unique index account_grants_seccao
    on account_grants (account_id, agrupamento, seccao_tipo, seccao_numero)
    where seccao_tipo is not null;

-- The old flat text-array columns go; existing accounts simply re-read at their next login.
alter table accounts
    drop column access_level,
    drop column agrupamentos,
    drop column nucleos,
    drop column regioes,
    drop column access_refreshed_at;

alter table account_events
    drop constraint account_events_action_check,
    add constraint account_events_action_check check (action in (
        'invited', 'login', 'enabled', 'disabled', 'role_changed', 'identifier_linked',
        'access_refreshed', 'access_failed'
    ));
