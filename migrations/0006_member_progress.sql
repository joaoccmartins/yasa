-- Progress: per member, the date of each step in each secção, from the four
-- elementos/progresso/lista-* reports. Columns keep the grid's names: prefix l Lobitos,
-- e Exploradores, p Pioneiros, c Caminheiros, then adesao, etapa1, etapa2, etapa3, anilhamerito
-- (anilha de mérito). One row per member covers their whole path through the secções.
-- Changes after the first import go to member_changes as 'progresso.<column>'.
create table member_progress (
    nin text primary key references members (nin) on delete cascade,
    ladesao date, letapa1 date, letapa2 date, letapa3 date, lanilhamerito date,
    eadesao date, eetapa1 date, eetapa2 date, eetapa3 date, eanilhamerito date,
    padesao date, petapa1 date, petapa2 date, petapa3 date, panilhamerito date,
    cadesao date, cetapa1 date, cetapa2 date, cetapa3 date, canilhamerito date,
    updated_at timestamptz not null default now()
);
