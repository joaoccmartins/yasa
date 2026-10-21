-- Members: a pseudonymised copy of SIIE's "dados completos" grid, imported per agrupamento.
-- No names, NIFs, contacts or street addresses: those are dropped in memory, before the DB.
-- The NIN is the key; the totem is the human-friendly label (SIIE's own, or one derived from
-- the NIN, see members::totem).

create table members (
    nin text primary key,
    totem text not null,
    -- true: SIIE's own totem; false: generated from the NIN.
    totem_from_siie boolean not null,
    -- grid siglasexo: M / F
    sexo text,
    -- grid datanascimento cut to the 1st of its month: age to the month, not the day.
    birth_month date,
    agrupamento smallint,
    nucleo smallint,
    regiao smallint,
    -- Also part of a núcleo / região / national team (grid elemnucleo, elemregiao, elemNacional).
    el_nucleo smallint,
    el_regiao smallint,
    el_nacional boolean,
    -- As SIIE writes them, e.g. 'Ativo', 'Inativo (E:MS)', 'Noviço Pioneiro / Marinheiro / Tripulante'.
    situacao text,
    categoria text,
    -- grid ordem: the categoria's place in the progression (10 Aspirante a Lobito ... 160 Auxiliar).
    categoria_ordem smallint,
    seccao text,
    cartao boolean,
    admitted_on date,
    promised_on date,
    profissao text,
    formador boolean,
    diretor_formacao boolean,
    -- grid imagensnao: refuses the use of their image/voice.
    refuses_image boolean,
    -- false once an import of its agrupamento no longer lists it (left, or moved elsewhere).
    present boolean not null default true,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index members_agrupamento on members (agrupamento);

-- Kept apart so the member data can be read without the place people live in.
-- Current values only, no history: nobody can follow someone's moves.
create table member_addresses (
    nin text primary key references members (nin) on delete cascade,
    cp4 text,
    cp3 text,
    localidade text,
    -- grid codigopostal: the post office name of the postal code.
    cp_localidade text,
    naturalidade text,
    updated_at timestamptz not null default now()
);

-- Health data, apart and with no history. More columns come from the member ficha later.
create table member_health (
    nin text primary key references members (nin) on delete cascade,
    deficiencia text,
    sigla_deficiencia text,
    updated_at timestamptz not null default now()
);

-- One row per agrupamento import: the job queue and the import history in one table.
-- An agrupamento's freshness is its latest 'done' row's finished_at.
create table member_imports (
    id bigint generated always as identity primary key,
    agrupamento smallint not null,
    -- Whose login queued it. null = that account was deleted.
    account_id uuid references accounts (id) on delete set null,
    status text not null default 'queued'
        check (status in ('queued', 'running', 'done', 'failed')),
    queued_at timestamptz not null default now(),
    started_at timestamptz,
    finished_at timestamptz,
    members_seen integer,
    changes integer,
    exits integer,
    -- SIIE / grid problem. Never personal data.
    error text
);
-- At most one queued or running import per agrupamento: two logins can't import it at once.
create unique index member_imports_active on member_imports (agrupamento)
    where status in ('queued', 'running');
create index member_imports_agrupamento on member_imports (agrupamento, id desc);

-- History: one row per changed field per import. field 'present' records entries (null -> true),
-- exits (true -> false) and comebacks (false -> true). The first sighting only records 'present';
-- the values at that time are the old_value of each field's first change.
create table member_changes (
    id bigint generated always as identity primary key,
    nin text not null references members (nin) on delete cascade,
    import_id bigint references member_imports (id) on delete set null,
    at timestamptz not null default now(),
    field text not null,
    old_value text,
    new_value text
);
create index member_changes_nin on member_changes (nin, at);
create index member_changes_field on member_changes (field, at);
