-- More SIIE reports, described in member_reports.rs. Columns of per-member tables keep the grid's
-- names; record tables are keyed by SIIE's id for the record (grid rid).

-- Health conditions (yes/no) and the tetanus vaccine date, from elementos/dadossaude/dados-saude.
-- No descriptions, no SNS number. Current values only, no history.
alter table member_health
    add column alergias boolean,
    add column asma boolean,
    add column diabetes boolean,
    add column epilepsia boolean,
    add column medicacao boolean,
    add column outroprob boolean,
    add column restricoes boolean,
    add column vacinaantitet date;

alter table member_progress
    -- Agrupamentos the member was in before (elementos/elementos/lista-historico), sorted.
    add column histagrupamento smallint[],
    -- Former members (elementos/elementos/saida-elementos-por-ano): year they left, last census.
    add column ano_saida integer,
    add column ultimoano_censos integer;

-- Record tables: import_agrupamento is the agrupamento the record was listed under in SIIE.
-- An import only removes records listed under its own agrupamento.
alter table member_cargos add column import_agrupamento smallint;
create index member_cargos_import on member_cargos (import_agrupamento);

-- Courses (formação), from elementos/cursos/lista-cursos. No ordem de serviço.
create table member_cursos (
    siie_id bigint primary key,
    nin text not null references members (nin) on delete cascade,
    import_agrupamento smallint,
    curso text,
    sigla_curso text,
    -- grid datadoc
    completed_on date,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index member_cursos_nin on member_cursos (nin);
create index member_cursos_import on member_cursos (import_agrupamento);

-- One row per participation, from elementos/actividades/atividades-elementos. No description
-- or place.
create table member_atividades (
    siie_id bigint primary key,
    nin text not null references members (nin) on delete cascade,
    import_agrupamento smallint,
    -- grid idatividade: the activity, shared by its participants.
    atividade text,
    -- grid siglatipo: acamp, acant, bivaq, raid, jcida, jmar, frm, grl
    tipo text,
    -- grid siglagrupo: agr, nuc, reg, nac, int
    nivel text,
    started_on date,
    ended_on date,
    noites integer,
    noites_campo integer,
    conta_noites_campo boolean,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index member_atividades_nin on member_atividades (nin);
create index member_atividades_import on member_atividades (import_agrupamento);
create index member_atividades_atividade on member_atividades (atividade);

-- Awards, from elementos/justica/lista-justica: only the award types in member_reports::AWARDS.
-- Sanctions are never read. No ordem de serviço number, no description.
create table member_distincoes (
    siie_id bigint primary key,
    nin text not null references members (nin) on delete cascade,
    import_agrupamento smallint,
    distincao text,
    sigla text,
    -- grid osdata
    awarded_on date,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index member_distincoes_nin on member_distincoes (nin);
create index member_distincoes_import on member_distincoes (import_agrupamento);

-- Education level, free text as SIIE has it (e.g. "12º ano"), from
-- elementos/habilitacoes/lista-habilitacoes.
create table member_habilitacoes (
    siie_id bigint primary key,
    nin text not null references members (nin) on delete cascade,
    import_agrupamento smallint,
    habilitacoes text,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index member_habilitacoes_nin on member_habilitacoes (nin);
create index member_habilitacoes_import on member_habilitacoes (import_agrupamento);
