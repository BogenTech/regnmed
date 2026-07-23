-- The marketplace flow: a company asks a verified firm for an oppdrag;
-- the firm accepts or declines. Requests are an insert-only-ish audit
-- trail (only the decision fields may change, exactly once); accepting
-- opens the engagement that the whole authorization model already runs
-- on. Ending an engagement sets valid_to — history is never deleted.

create table engagement_request (
    id           uuid primary key,
    firm_id      uuid not null references firm (id),
    company_id   uuid not null references company (id),
    kind         text not null check (kind in ('regnskap', 'revisjon')),
    message      text,
    status       text not null default 'pending'
        check (status in ('pending', 'accepted', 'declined')),
    requested_by uuid not null references person (id),
    decided_by   uuid references person (id),
    created_at   timestamptz not null default now(),
    decided_at   timestamptz
);

-- At most one open request per firm/company/kind.
create unique index engagement_request_open_idx
    on engagement_request (firm_id, company_id, kind)
    where status = 'pending';

grant select, insert on engagement_request to regnmed_app;
grant update (status, decided_by, decided_at) on engagement_request to regnmed_app;
grant update (valid_to) on engagement to regnmed_app;
