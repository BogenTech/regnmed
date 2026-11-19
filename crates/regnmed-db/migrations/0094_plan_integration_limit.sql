-- Package guardrails (docs/abonnement.md §7): a package may cap how many
-- integrations and agents a company can have, next to the voucher and payroll
-- limits from 0093. NULL = no limit. Adding a column writes no row, so the
-- append-only trigger is untouched, and existing versions read as "no limit".
alter table subscription_plan
    add column integration_limit integer
        check (integration_limit is null or integration_limit >= 0);
