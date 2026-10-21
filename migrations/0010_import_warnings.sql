-- Reports skipped in an import that otherwise succeeded, and why (SIIE errors: report paths,
-- column names and SIIE's own messages, never member data). null = nothing skipped.
alter table member_imports add column warning text;
