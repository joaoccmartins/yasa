-- Who did what to which account: invites, logins, enable/disable, role changes, identifier links.
create table account_events (
    id bigint generated always as identity primary key,
    at timestamptz not null default now(),
    -- null = the command line (`cargo run --bin invite`) or the system itself.
    actor_id uuid references accounts (id) on delete set null,
    account_id uuid not null references accounts (id) on delete cascade,
    action text not null
        check (action in ('invited', 'login', 'enabled', 'disabled', 'role_changed', 'identifier_linked')),
    -- Small facts about the event, e.g. {"from": "member", "to": "admin"} or the access level at login.
    -- Never identifiers or anything personal.
    detail jsonb not null default '{}'
);
create index account_events_account on account_events (account_id, at desc);
create index account_events_at on account_events (at desc);

-- Failed SIIE logins, to stop YASA being used to guess SIIE passwords. `key` is an HMAC of the
-- identifier or of the client IP, never the value itself. Rows older than the window are deleted
-- on the next failure.
create table login_failures (
    key bytea not null,
    at timestamptz not null default now()
);
create index login_failures_key_at on login_failures (key, at);
