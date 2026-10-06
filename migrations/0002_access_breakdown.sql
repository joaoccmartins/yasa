-- The núcleos and regiões SIIE showed each account at its last login, next to the agrupamentos.
-- They are what siie::Visibility::level reads the access level from, so keeping them makes the
-- level checkable. Empty until the account's next login.
alter table accounts
    add column nucleos text[] not null default '{}',
    add column regioes text[] not null default '{}';
