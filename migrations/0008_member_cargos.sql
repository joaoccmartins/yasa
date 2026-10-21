-- Cargos, from elementos/cargos/cargos-ativos: every cargo assignment, past and current.
-- siie_id is SIIE's id for the assignment (grid rid), so one can be followed across imports.
-- Changes go to member_changes as 'cargo.<siie_id>' (added / gone) and 'cargo.<siie_id>.<field>'.
create table member_cargos (
    siie_id bigint primary key,
    nin text not null references members (nin) on delete cascade,
    cargo text,
    sigla_cargo text,
    -- grid agrup: the agrupamento the cargo is in; null = outside one (núcleo, região...).
    agrupamento smallint,
    started_on date,
    ended_on date,
    active boolean,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index member_cargos_nin on member_cargos (nin);
create index member_cargos_agrupamento on member_cargos (agrupamento);
