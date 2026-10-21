-- Agrupamento general data, from the agrupamentos list (/agrupamentos/list?xml=agrupamentos/
-- agrupamento/agrupamento), read in each member import of the agrupamento. Organisation data,
-- current values only, no history. See member_reports::AGRUPAMENTO_COLUMNS for what is read.
alter table agrupamentos
    -- grid estado: 'Ativo' / 'Extinto'
    add column estado text,
    -- grid tipo: 'Terrestre', 'Marítimo', 'Aéreo'
    add column tipo text,
    add column paroquia text,
    add column patrono text,
    -- The agrupamento's own contacts, as SIIE lists them.
    add column email text,
    add column emailnotif text,
    add column telefone text,
    add column site text,
    -- Sede: postal code first part and localidade only. No street, no mailing address.
    add column cp4 text,
    add column localidade text,
    -- grid dtiniciado: when it became an agrupamento iniciado.
    add column iniciado_on date,
    -- The date in grid osnfiliado ("<OS number>, dd-mm-yyyy"); the OS number is not kept.
    add column filiado_on date,
    -- Last time these columns were read; null = never.
    add column details_updated_at timestamptz;

-- grid nucnome / regnome without the code prefix, e.g. 'Junta de Núcleo de Guimarães'.
alter table nucleos add column name text;
alter table regioes add column name text;
