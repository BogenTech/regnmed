-- The last verdict of `regnmed job-watch` (docs/deploy.md). One row, one
-- bit: which job failed stays in the cluster's logs, because this value is
-- served publicly so the monitor outside the cluster needs no credentials.
create table job_health (
    id         boolean primary key default true check (id),
    healthy    boolean not null,
    checked_at timestamptz not null default now()
);

grant select, insert, update on job_health to regnmed_app;
