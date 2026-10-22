-- The secção a cargo belongs to (1 Lobitos, 2 Exploradores, 3 Pioneiros, 4 Caminheiros), so a
-- dirigente's path through the secções is kept over the years (instrutor of the Expedição in
-- 1995, chefe of the Alcateia in 1998...). Set at import by member_reports::cargo_seccao: from
-- the cargo's sigla, else from words in its free-text descrição, which itself is never stored.
-- null = a cargo of no secção (agrupamento, núcleo...) or one SIIE doesn't say.
alter table member_cargos
    add column seccao_tipo smallint check (seccao_tipo between 1 and 4);

-- Cargos already imported: their sigla is enough for the chefes and adjuntos. The rest (e.g.
-- instrutores) get theirs at the next import, from the descrição.
update member_cargos set seccao_tipo = case
    when sigla_cargo in ('CALC', 'CALCADJ') then 1
    when sigla_cargo in ('CGE', 'CGEADJ') then 2
    when sigla_cargo in ('CGP', 'CGPADJ') then 3
    when sigla_cargo in ('CC', 'CCADJ') then 4
end;
