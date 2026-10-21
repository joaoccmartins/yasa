-- Promessas per secção, from elementos/vidaescutista/lista-geral, next to the etapas.
alter table member_progress
    add column promessalobito date,
    add column promessaexplorador date,
    add column promessapioneiro date,
    add column promessacaminheiro date,
    add column promessadirigente date;

-- Especialidades, from the four elementos/especialidades/lista-* reports: one row per
-- especialidade a member earned in a secção. seccao_tipo is the report's secção
-- (1 Lobitos, 2 Exploradores, 3 Pioneiros, 4 Caminheiros), not the member's current one.
-- Replaced per member on every import; additions and removals go to member_changes as
-- 'especialidade.<seccao_tipo>.<especialidade>'.
create table member_especialidades (
    nin text not null references members (nin) on delete cascade,
    seccao_tipo smallint not null check (seccao_tipo between 1 and 4),
    especialidade text not null,
    -- grid datarealizacao
    achieved_on date,
    primary key (nin, seccao_tipo, especialidade)
);
create index member_especialidades_name on member_especialidades (especialidade);
