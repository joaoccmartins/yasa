-- Members seen for the first time in an import, counted apart from changes. null = imports
-- from before this column (their `changes` include the entries).
alter table member_imports add column entries integer;
