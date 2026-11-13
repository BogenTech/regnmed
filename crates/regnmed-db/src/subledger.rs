//! Reskontro: kunde- og leverandørspesifikasjon and åpne poster.
//!
//! Parties are master data (name editable); the party *number* is part
//! of the v2 hash chain and immutable once referenced. "Open" is always
//! computed — an item is open while the sum matched against it is below
//! its own amount — never stored state.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::money::Ore;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Auto-numbering start per kind (SAF-T-friendly ranges).
fn first_party_no(kind: &str) -> i64 {
    match kind {
        "kunde" => 10_000,
        _ => 50_000,
    }
}

pub async fn create_party(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    name: &str,
    orgnumber: Option<&str>,
    party_no: Option<&str>,
) -> Result<(Uuid, String)> {
    ensure!(
        kind == "kunde" || kind == "leverandor",
        "kind must be 'kunde' or 'leverandor'"
    );
    let party_no = match party_no {
        Some(no) => {
            ensure!(
                !no.is_empty() && no.chars().all(|c| c.is_ascii_digit()),
                "party_no must be digits"
            );
            no.to_string()
        }
        None => {
            let next: i64 = sqlx::query_scalar(
                "select coalesce(max(party_no::bigint) + 1, $2)
                 from party where company_id = $1 and kind = $3",
            )
            .bind(company_id)
            .bind(first_party_no(kind))
            .bind(kind)
            .fetch_one(pool)
            .await?;
            next.to_string()
        }
    };
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into party (id, company_id, party_no, kind, name, orgnr)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&party_no)
    .bind(kind)
    .bind(name)
    .bind(orgnumber)
    .execute(pool)
    .await
    .context("creating party (duplicate party_no?)")?;
    Ok((id, party_no))
}

#[derive(Debug)]
pub struct PartyRow {
    pub id: Uuid,
    pub party_no: String,
    pub kind: String,
    pub name: String,
    pub orgnumber: Option<String>,
    pub address: Option<String>,
    pub email: Option<String>,
    pub bank_account: Option<String>,
    /// The customer's standing reference ("Deres referanse"), used on an
    /// invoice that does not name its own (#221).
    pub buyer_reference: Option<String>,
    /// SUM(amount_ore) over the party's entries — the reskontro saldo.
    pub balance_ore: i64,
    /// Overdue and unpaid: for a kunde, what remains on its overdue
    /// invoices (the invoice list's own status); for a leverandør, what
    /// remains on posts past their recorded due date (payable_terms). None
    /// when the listing did not ask for it (an unpaged listing).
    pub overdue_ore: Option<i64>,
}

/// Which parties a listing asks for. Default = every party of every
/// kind, by number -- the spesifikasjon.
#[derive(Debug, Default)]
pub struct PartyFilter<'a> {
    pub kind: Option<&'a str>,
    /// One party by id (the portal's customer page).
    pub id: Option<Uuid>,
    /// Exact party number or orgnr, or part of the name or e-mail.
    pub search: Option<&'a str>,
    /// `number` (default), `name`, `balance` or `overdue`.
    pub sort: Option<&'a str>,
    pub descending: bool,
    pub limit: Option<i64>,
    pub offset: i64,
}

#[derive(Debug, Default)]
pub struct PartyPage {
    pub rows: Vec<PartyRow>,
    /// Matching the filter, before limit/offset.
    pub total: i64,
}

/// Parties with their saldo and overdue amount, searched, sorted and
/// paged in SQL so the portal never downloads the whole register.
pub async fn list_parties(
    pool: &PgPool,
    company_id: Uuid,
    filter: &PartyFilter<'_>,
) -> Result<PartyPage> {
    let sort = filter.sort.unwrap_or("number");
    let order = match sort {
        // Party numbers are digits stored as text: 9 sorts before 10.
        "number" => "length(party_no), party_no",
        "name" => "lower(name), length(party_no), party_no",
        "balance" => "saldo_ore, length(party_no), party_no",
        "overdue" => "overdue_ore, length(party_no), party_no",
        other => bail!("ukjent sortering «{other}» (number, name, balance, overdue)"),
    };
    let direction = if filter.descending { "desc" } else { "asc" };
    // Every sort key gets the direction, the party-number tie-break too.
    let order = order
        .split(", ")
        .map(|k| format!("{k} {direction}"))
        .collect::<Vec<_>>()
        .join(", ");
    let search = filter.search.map(str::trim).filter(|s| !s.is_empty());

    // The overdue amount needs every invoice's reskontro status, which is
    // the expensive part. Sorting by it computes it for every party;
    // otherwise only for the page being returned, and not at all for an
    // unpaged listing (the pickers that just need names).
    let by_overdue = sort == "overdue";
    let (cte, overdue_col, overdue_join, first) = if by_overdue {
        (
            format!(
                "{},
                 overdue as (
                     select party_no, sum(remaining_ore)::bigint as overdue_ore
                     from inv where status = 'overdue' group by party_no
                 ),
                 supplier_overdue as ({}),",
                crate::invoice::INVOICE_STATUS_CTE,
                crate::payments::SUPPLIER_OVERDUE_SQL,
            ),
            "case when p.kind = 'kunde' then coalesce(o.overdue_ore, 0)
                  else coalesce(so.overdue_ore, 0) end",
            "left join overdue o on o.party_no = p.party_no and p.kind = 'kunde'
             left join supplier_overdue so on so.party_no = p.party_no and p.kind = 'leverandor'",
            4,
        )
    } else {
        ("with".to_string(), "null::bigint", "", 2)
    };
    let n = |i: usize| format!("${}", first + i);
    let sql = format!(
        "{cte}
         parties as (
             select p.id, p.party_no, p.kind, p.name, p.orgnr, p.address, p.email,
                    p.bank_account, p.buyer_reference,
                    coalesce((select sum(e.amount_ore) from entry e where e.party_id = p.id),
                             0)::bigint as saldo_ore,
                    {overdue_col} as overdue_ore
             from party p
             {overdue_join}
             where p.company_id = $1
               and ({kind}::text is null or p.kind = {kind})
               and ({id}::uuid is null or p.id = {id})
               and ({q}::text is null
                    or p.party_no = {q}
                    or replace(coalesce(p.orgnr, ''), ' ', '') = replace({q}, ' ', '')
                    or position(lower({q}) in lower(p.name)) > 0
                    or position(lower({q}) in lower(coalesce(p.email, ''))) > 0)
         )
         select *, count(*) over () as total from parties
         order by {order}
         limit {limit} offset {offset}",
        kind = n(0),
        id = n(1),
        q = n(2),
        limit = n(3),
        offset = n(4),
    );
    let mut query = sqlx::query(&sql).bind(company_id);
    if by_overdue {
        // The invoice CTE's own search and customer filter.
        query = query.bind(None::<&str>).bind(None::<&str>);
    }
    let rows = query
        .bind(filter.kind)
        .bind(filter.id)
        .bind(search)
        .bind(filter.limit)
        .bind(filter.offset.max(0))
        .fetch_all(pool)
        .await?;
    let total = rows.first().map(|r| r.get("total")).unwrap_or(0);
    let mut rows: Vec<PartyRow> = rows
        .iter()
        .map(|r| PartyRow {
            id: r.get("id"),
            party_no: r.get("party_no"),
            kind: r.get("kind"),
            name: r.get("name"),
            orgnumber: r.get("orgnr"),
            address: r.get("address"),
            email: r.get("email"),
            bank_account: r.get("bank_account"),
            buyer_reference: r.get("buyer_reference"),
            balance_ore: r.get("saldo_ore"),
            overdue_ore: r.get("overdue_ore"),
        })
        .collect();

    let page_wants_overdue = filter.limit.is_some() || filter.id.is_some();
    if !by_overdue && page_wants_overdue && !rows.is_empty() {
        let customers: Vec<String> = rows
            .iter()
            .filter(|r| r.kind == "kunde")
            .map(|r| r.party_no.clone())
            .collect();
        let overdue: std::collections::HashMap<String, i64> = if customers.is_empty() {
            Default::default()
        } else {
            sqlx::query(&format!(
                "{}
                 select party_no, sum(remaining_ore)::bigint as overdue_ore
                 from inv where status = 'overdue' and party_no = any($4)
                 group by party_no",
                crate::invoice::INVOICE_STATUS_CTE
            ))
            .bind(company_id)
            .bind(None::<&str>)
            .bind(None::<&str>)
            .bind(&customers)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| (r.get("party_no"), r.get("overdue_ore")))
            .collect()
        };
        let suppliers: std::collections::HashMap<String, i64> =
            if rows.iter().any(|r| r.kind == "leverandor") {
                let numbers: Vec<String> = rows
                    .iter()
                    .filter(|r| r.kind == "leverandor")
                    .map(|r| r.party_no.clone())
                    .collect();
                sqlx::query(&format!(
                    "with so as ({}) select party_no, overdue_ore from so
                     where party_no = any($2)",
                    crate::payments::SUPPLIER_OVERDUE_SQL
                ))
                .bind(company_id)
                .bind(&numbers)
                .fetch_all(pool)
                .await?
                .iter()
                .map(|r| (r.get("party_no"), r.get("overdue_ore")))
                .collect()
            } else {
                Default::default()
            };
        for row in &mut rows {
            let map = if row.kind == "kunde" {
                &overdue
            } else {
                &suppliers
            };
            row.overdue_ore = Some(map.get(&row.party_no).copied().unwrap_or(0));
        }
    }
    Ok(PartyPage { total, rows })
}

#[derive(Debug)]
pub struct PartyItem {
    pub entry_id: Uuid,
    pub voucher_id: Uuid,
    pub voucher_label: String,
    pub date: NaiveDate,
    pub description: Option<String>,
    pub amount_ore: i64,
    /// Sum already matched against this entry (signed like the entry).
    pub matched_ore: i64,
    /// `amount - matched`; the item is open while this is non-zero.
    pub remaining_ore: i64,
    /// From the supplier's document (payable_terms, newest row).
    pub due_date: Option<NaiveDate>,
    pub kid: Option<String>,
    /// Part of a payment list that is not yet paid (utkast/godkjent).
    pub in_run: bool,
}

/// All items for one party, oldest first, with their open remainder.
pub async fn party_items<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    company_id: Uuid,
    party_id: Uuid,
    open_only: bool,
) -> Result<Vec<PartyItem>> {
    let rows = sqlx::query(
        "select e.id, v.id as voucher_id, v.fiscal_year, v.voucher_number, v.voucher_date,
                terms.due_date, terms.kid,
                exists (select 1 from payment_run_item i
                        join payment_run r on r.id = i.run_id
                        where i.entry_id = e.id
                          and r.status in ('utkast', 'godkjent')) as in_run,
                coalesce(e.description, v.description) as description, e.amount_ore,
                (o.amount_ore - o.remaining_ore)::bigint as matched_ore
         from entry e
         join voucher v on v.id = e.voucher_id
         join subledger_entry_open o on o.entry_id = e.id
         left join lateral (
             select t.due_date, t.kid from payable_terms t
             where t.entry_id = e.id
             order by t.recorded_at desc, t.id desc limit 1
         ) terms on true
         where v.company_id = $1 and e.party_id = $2
         order by v.voucher_date, v.voucher_number, e.line_no",
    )
    .bind(company_id)
    .bind(party_id)
    .fetch_all(executor)
    .await?;

    let items: Vec<PartyItem> = rows
        .iter()
        .map(|r| {
            let amount_ore: i64 = r.get("amount_ore");
            let matched_ore: i64 = r.get("matched_ore");
            PartyItem {
                entry_id: r.get("id"),
                voucher_id: r.get("voucher_id"),
                due_date: r.get("due_date"),
                kid: r.get("kid"),
                in_run: r.get("in_run"),
                voucher_label: format!(
                    "{}-{}",
                    r.get::<i32, _>("fiscal_year"),
                    r.get::<i64, _>("voucher_number")
                ),
                date: r.get("voucher_date"),
                description: r.get("description"),
                amount_ore,
                matched_ore,
                remaining_ore: amount_ore - matched_ore,
            }
        })
        .filter(|item| !open_only || item.remaining_ore != 0)
        .collect();
    Ok(items)
}

/// Matches an invoice-side entry (`entry_a`, positive remainder) against
/// a settlement-side entry (`entry_b`, negative remainder) for
/// `amount_ore` (> 0, ≤ both remainders). Both entries must belong to
/// the same company, party and account.
pub async fn match_items(
    pool: &PgPool,
    company_id: Uuid,
    entry_a: Uuid,
    entry_b: Uuid,
    amount_ore: i64,
    matched_by: &str,
) -> Result<Uuid> {
    ensure!(amount_ore > 0, "beløpet må være positivt");
    let mut tx = pool.begin().await?;
    let party_id: Option<Uuid> = sqlx::query_scalar(
        "select a.party_id
         from entry a
         join voucher va on va.id = a.voucher_id and va.company_id = $1
         join entry b on b.id = $3
         join voucher vb on vb.id = b.voucher_id and vb.company_id = $1
         where a.id = $2
           and a.party_id is not null and a.party_id = b.party_id
           and a.account_id = b.account_id",
    )
    .bind(company_id)
    .bind(entry_a)
    .bind(entry_b)
    .fetch_optional(&mut *tx)
    .await?;
    let party_id =
        party_id.context("postene må tilhøre samme part og samme reskontrokonto i selskapet")?;
    // Matches on one party take turns: the remainders are read and the
    // match written under the same lock, so two simultaneous matches
    // cannot both consume the same open amount.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(party_id)
        .execute(&mut *tx)
        .await?;

    let items = party_items(&mut *tx, company_id, party_id, false).await?;
    let remaining = |id: Uuid| {
        items
            .iter()
            .find(|i| i.entry_id == id)
            .map(|i| i.remaining_ore)
    };
    let remaining_a = remaining(entry_a).context("posten finnes ikke")?;
    let remaining_b = remaining(entry_b).context("posten finnes ikke")?;
    ensure!(
        remaining_a >= amount_ore,
        "den ene posten har bare {} kr åpent, så {} kr kan ikke avstemmes mot den",
        Ore(remaining_a.max(0)),
        Ore(amount_ore)
    );
    ensure!(
        -remaining_b >= amount_ore,
        "den andre posten har bare {} kr åpent, så {} kr kan ikke avstemmes mot den",
        Ore((-remaining_b).max(0)),
        Ore(amount_ore)
    );

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(entry_a)
    .bind(entry_b)
    .bind(amount_ore)
    .bind(matched_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn unmatch_items(pool: &PgPool, company_id: Uuid, match_id: Uuid) -> Result<()> {
    // A currency settlement posted its agio voucher in the same
    // transaction as the match rows; undoing one row would leave that
    // voucher standing against nothing. It is corrected with a reversal.
    let currency: Option<bool> = sqlx::query_scalar(
        // The settlement row carries valuta_cent; the row closing the agio
        // transfer was written in the SAME transaction (equal created_at,
        // which is the transaction's now()) and shares an entry with it.
        "select m.valuta_cent is not null or exists (
                    select 1 from reskontro_match x
                    where x.valuta_cent is not null and x.created_at = m.created_at
                      and (x.entry_a in (m.entry_a, m.entry_b) or x.entry_b in (m.entry_a, m.entry_b))
                )
         from reskontro_match m
         join entry ea on ea.id = m.entry_a
         join voucher v on v.id = ea.voucher_id
         where m.id = $1 and v.company_id = $2",
    )
    .bind(match_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    ensure!(currency.is_some(), "avstemmingen finnes ikke");
    ensure!(
        currency == Some(false),
        "et valutaoppgjør med kursdifferanse kan ikke løses opp her — reverser kursdifferansebilaget"
    );
    let removed = sqlx::query(
        "delete from reskontro_match m
         using entry e, voucher v
         where m.id = $2 and e.id = m.entry_a and v.id = e.voucher_id and v.company_id = $1",
    )
    .bind(company_id)
    .bind(match_id)
    .execute(pool)
    .await?;
    ensure!(removed.rows_affected() == 1, "avstemmingen finnes ikke");
    Ok(())
}

/// Flags an account as a reskontro account (or clears the flag).
pub async fn set_account_subledger(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    kind: Option<&str>,
) -> Result<()> {
    let updated =
        sqlx::query("update account set reskontro_kind = $3 where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(account_number)
            .bind(kind)
            .execute(pool)
            .await?;
    ensure!(updated.rows_affected() == 1, "no account {account_number}");
    Ok(())
}

/// Matches two entries of the SAME valuta by valutabeløp and posts the
/// realized agio/disagio as an ordinary voucher IN THE SAME
/// TRANSACTION as the match rows (docs/valuta.md):
///
/// - the NOK each side consumes for `valuta_cent` is proportional to
///   its own booked relation (amount / valutabeløp), so no external
///   kurs enters the settlement;
/// - the difference posts against gevinstkonto (8060) / tapskonto
///   (8160) with a party-carrying transfer entry on the reskontro
///   account, and a second match row closes that transfer — both
///   sides' NOK remainders reach exactly zero, never drift.
#[allow(clippy::too_many_arguments)]
pub async fn match_valuta(
    pool: &PgPool,
    company_id: Uuid,
    entry_a: Uuid,
    entry_b: Uuid,
    valuta_cent: i64,
    gain_account: &str,
    loss_account: &str,
    matched_by: &str,
) -> Result<i64> {
    ensure!(valuta_cent > 0, "valutabeløpet må være positivt");
    // Same per-party lock as match_items: the remainders read below and
    // the rows written at the end must not interleave with another match.
    let mut tx = pool.begin().await?;
    let party_id: Option<Uuid> = sqlx::query_scalar(
        "select e.party_id from entry e join voucher v on v.id = e.voucher_id
         where e.id = $1 and v.company_id = $2",
    )
    .bind(entry_a)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    if let Some(party_id) = party_id {
        sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 0))")
            .bind(party_id)
            .execute(&mut *tx)
            .await?;
    }
    let pair = sqlx::query(
        "select a.amount_ore as amount_a, a.valuta as valuta_a,
                a.valutabelop_cent as belop_a,
                b.amount_ore as amount_b, b.valuta as valuta_b,
                b.valutabelop_cent as belop_b,
                p.party_no, acc.number as account_number, va.voucher_date
         from entry a
         join voucher va on va.id = a.voucher_id and va.company_id = $1
         join entry b on b.id = $3
         join voucher vb on vb.id = b.voucher_id and vb.company_id = $1
         join party p on p.id = a.party_id
         join account acc on acc.id = a.account_id
         where a.id = $2
           and a.party_id is not null and a.party_id = b.party_id
           and a.account_id = b.account_id",
    )
    .bind(company_id)
    .bind(entry_a)
    .bind(entry_b)
    .fetch_optional(pool)
    .await?
    .context("entries must belong to the same company, party and reskontro account")?;
    let valuta_a: Option<String> = pair.get("valuta_a");
    let valuta_b: Option<String> = pair.get("valuta_b");
    ensure!(
        valuta_a.is_some() && valuta_a == valuta_b,
        "begge posteringene må være i samme valuta"
    );
    let currency_amount_a: i64 = pair
        .get::<Option<i64>, _>("belop_a")
        .context("entry_a mangler valutabeløp")?;
    let currency_amount_b: i64 = pair
        .get::<Option<i64>, _>("belop_b")
        .context("entry_b mangler valutabeløp")?;
    let amount_a: i64 = pair.get("amount_a");
    let amount_b: i64 = pair.get("amount_b");
    ensure!(
        currency_amount_a > 0 && currency_amount_b < 0,
        "entry_a må være debetsiden (positivt valutabeløp), entry_b kreditsiden"
    );

    // Open remainders in valuta on both sides.
    let matched_cent = |entry: Uuid, as_a: bool| async move {
        let sum: i64 = sqlx::query_scalar(if as_a {
            "select coalesce(sum(valuta_cent), 0)::bigint from reskontro_match where entry_a = $1"
        } else {
            "select coalesce(sum(valuta_cent), 0)::bigint from reskontro_match where entry_b = $1"
        })
        .bind(entry)
        .fetch_one(pool)
        .await?;
        anyhow::Ok(sum)
    };
    let rest_a = currency_amount_a - matched_cent(entry_a, true).await?;
    let rest_b = -currency_amount_b - matched_cent(entry_b, false).await?;
    ensure!(
        valuta_cent <= rest_a && valuta_cent <= rest_b,
        "åpent valutabeløp er {rest_a} / {rest_b} cent — kan ikke matche {valuta_cent}"
    );

    // NOK consumed per side, proportional to each entry's own booking.
    let andel = |amount: i64, currency_amount: i64| -> i64 {
        let product = i128::from(valuta_cent) * i128::from(amount.abs());
        let currency_amount = i128::from(currency_amount.abs());
        i64::try_from((product + currency_amount / 2) / currency_amount)
            .expect("NOK share fits in i64")
    };
    let nok_a = andel(amount_a, currency_amount_a);
    let nok_b = andel(amount_b, currency_amount_b);
    let agio = nok_b - nok_a; // > 0: settlement brought more NOK → gevinst
    let felles = nok_a.min(nok_b);

    if felles > 0 {
        sqlx::query(
            "insert into reskontro_match (id, entry_a, entry_b, amount_ore, valuta_cent, matched_by)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(entry_a)
        .bind(entry_b)
        .bind(felles)
        .bind(valuta_cent)
        .bind(matched_by)
        .execute(&mut *tx)
        .await?;
    }
    if agio != 0 {
        use regnmed_core::Ore;
        use regnmed_core::voucher::{EntryDraft, VoucherDraft};
        let party_no: String = pair.get("party_no");
        let account: String = pair.get("account_number");
        let today: NaiveDate = sqlx::query_scalar("select current_date")
            .fetch_one(&mut *tx)
            .await?;
        let (income_account, result_amount, party_amount) = if agio > 0 {
            (gain_account, -agio, agio)
        } else {
            (loss_account, -agio, agio)
        };
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: today,
            description: format!(
                "Realisert {} valuta ({})",
                if agio > 0 { "agio" } else { "disagio" },
                valuta_a.as_deref().unwrap_or("")
            ),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: account,
                    amount: Ore(party_amount),
                    vat_code: None,
                    description: Some("Kursdifferanse ved oppgjør".into()),
                    party_no: Some(party_no),
                    department: None,
                    project: None,
                    currency: None,
                },
                EntryDraft {
                    account_number: income_account.to_string(),
                    amount: Ore(result_amount),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                },
            ],
        };
        draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
        let posted =
            crate::ledger::post_voucher_in(&mut tx, company_id, &draft, matched_by).await?;
        let agio_entry: Uuid = sqlx::query_scalar(
            "select id from entry where voucher_id = $1 and party_id is not null",
        )
        .bind(posted.id)
        .fetch_one(&mut *tx)
        .await?;
        // Close the leftover with the transfer entry: gevinst leaves a
        // credit rest on b (transfer is debit); tap leaves a debit rest
        // on a (transfer is credit).
        let (ma, mb) = if agio > 0 {
            (agio_entry, entry_b)
        } else {
            (entry_a, agio_entry)
        };
        sqlx::query(
            "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(ma)
        .bind(mb)
        .bind(agio.abs())
        .bind(matched_by)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(agio)
}

/// Urealisert kursregulering (docs/valuta.md): every open valuta item
/// revalued at the newest kurs on `per_dato`; the net difference posts
/// as one voucher on `per_dato` (balansekonto mot gevinst-/tapskonto)
/// and its REVERSAL the day after — generated and self-cleaning, so
/// the realized agio at oppgjør stays untouched. Returns None when
/// there is nothing to regulate.
#[allow(clippy::too_many_arguments)]
pub async fn kursregulering(
    pool: &PgPool,
    company_id: Uuid,
    as_of_date: NaiveDate,
    balance_account: &str,
    gain_account: &str,
    loss_account: &str,
    created_by: &str,
) -> Result<Option<(i64, (i32, i64), (i32, i64))>> {
    let rows = sqlx::query(
        "select e.id, e.amount_ore, e.valuta, e.valutabelop_cent,
                coalesce((select sum(m.amount_ore) from reskontro_match m
                          where m.entry_a = e.id), 0)::bigint
              + coalesce((select sum(-m.amount_ore) from reskontro_match m
                          where m.entry_b = e.id), 0)::bigint as matched_nok,
                coalesce((select sum(m.valuta_cent) from reskontro_match m
                          where m.entry_a = e.id), 0)::bigint
              + coalesce((select sum(m.valuta_cent) from reskontro_match m
                          where m.entry_b = e.id), 0)::bigint as matched_cent
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.valuta is not null and e.party_id is not null
           and v.voucher_date <= $2",
    )
    .bind(company_id)
    .bind(as_of_date)
    .fetch_all(pool)
    .await?;

    let mut diff_total = 0i64;
    for row in &rows {
        let amount: i64 = row.get("amount_ore");
        let matched_nok: i64 = row.get("matched_nok");
        let rest_nok = amount - matched_nok;
        if rest_nok == 0 {
            continue;
        }
        let amount: i64 = row.get::<Option<i64>, _>("valutabelop_cent").unwrap_or(0);
        let matched_cent: i64 = row.get("matched_cent");
        let rest_cent = if amount >= 0 {
            amount - matched_cent
        } else {
            amount + matched_cent
        };
        let currency: String = row.get("valuta");
        let (_, exchange_rate) =
            crate::currency::require_exchange_rate(pool, &currency, as_of_date).await?;
        diff_total += regnmed_core::currency::nok_ore(rest_cent, exchange_rate) - rest_nok;
    }
    if diff_total == 0 {
        return Ok(None);
    }

    use regnmed_core::Ore;
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};
    let entry = |account: &str, amount: i64, desc: &str| EntryDraft {
        account_number: account.to_string(),
        amount: Ore(amount),
        vat_code: None,
        description: Some(desc.to_string()),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    };
    let income_account = if diff_total > 0 {
        gain_account
    } else {
        loss_account
    };
    let mut tx = pool.begin().await?;
    let revaluation = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: as_of_date,
        description: format!("Urealisert kursregulering per {as_of_date}"),
        reverses: None,
        entries: vec![
            entry(
                balance_account,
                diff_total,
                "Kursregulering åpne valutaposter",
            ),
            entry(income_account, -diff_total, "Urealisert kursdifferanse"),
        ],
    };
    revaluation.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted =
        crate::ledger::post_voucher_in(&mut tx, company_id, &revaluation, created_by).await?;
    let next_day = as_of_date
        .succ_opt()
        .context("per_dato has no following day")?;
    let reversal = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: next_day,
        description: format!("Reversering kursregulering per {as_of_date}"),
        reverses: Some(posted.id),
        entries: vec![
            entry(balance_account, -diff_total, "Reversering kursregulering"),
            entry(income_account, diff_total, "Reversering kursregulering"),
        ],
    };
    reversal.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let reversed =
        crate::ledger::post_voucher_in(&mut tx, company_id, &reversal, created_by).await?;
    tx.commit().await?;
    Ok(Some((
        diff_total,
        (posted.fiscal_year, posted.voucher_number),
        (reversed.fiscal_year, reversed.voucher_number),
    )))
}

/// Per account, the hovedbok saldo against the sum of the parties' own
/// postings: every flagged account PLUS every unflagged account that
/// carries party postings (an amount no reskontro account holds is
/// exactly the divergence a check limited to flagged accounts cannot
/// see). The revisjonsrapport's check 4 and the reskontro overview read
/// the same numbers.
pub async fn subledger_accounts(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<Vec<regnmed_core::audit::SubledgerAccount>> {
    let rows = sqlx::query(
        "select a.number, a.reskontro_kind,
                coalesce(sum(e.amount_ore), 0)::bigint as hovedbok_ore,
                coalesce(sum(e.amount_ore) filter (where e.party_id is not null), 0)::bigint
                    as reskontro_ore,
                count(e.id) filter (where e.party_id is null)::bigint as uten_part_antall,
                count(e.id) filter (where e.party_id is not null)::bigint as med_part_antall,
                count(e.id) filter (where p.kind is not null
                                      and p.kind is distinct from a.reskontro_kind)::bigint
                    as feil_kind_antall,
                coalesce(sum(e.amount_ore) filter (where p.kind is not null
                                      and p.kind is distinct from a.reskontro_kind), 0)::bigint
                    as feil_kind_ore,
                coalesce(array_agg(distinct p.kind) filter (where p.kind is not null),
                         array[]::text[]) as part_kinds
         from account a
         left join entry e on e.account_id = a.id
         left join party p on p.id = e.party_id
         where a.company_id = $1
           and (a.reskontro_kind is not null
                or exists (select 1 from entry x
                           where x.account_id = a.id and x.party_id is not null))
         group by a.number, a.reskontro_kind
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let mut part_kinds: Vec<String> = r.get("part_kinds");
            part_kinds.sort(); // array_agg has no order; the report is deterministic
            regnmed_core::audit::SubledgerAccount {
                account: r.get("number"),
                flagg: r.get("reskontro_kind"),
                general_ledger_ore: r.get("hovedbok_ore"),
                subledger_ore: r.get("reskontro_ore"),
                without_party_count: r.get("uten_part_antall"),
                with_party_count: r.get("med_part_antall"),
                wrong_kind_count: r.get("feil_kind_antall"),
                feil_kind_ore: r.get("feil_kind_ore"),
                part_kinds,
            }
        })
        .collect())
}

/// A party with open items on BOTH sides of its reskontro — an invoice
/// and a payment that can be matched against each other.
#[derive(Debug)]
pub struct PartyToMatch {
    pub party_id: Uuid,
    pub party_no: String,
    pub name: String,
    pub kind: String,
    /// Open on the invoice side (customer: positive; supplier: the
    /// payments, positive too) and the settlement side (negative).
    pub open_positive_ore: i64,
    pub open_negative_ore: i64,
}

pub async fn parties_to_match(
    pool: &PgPool,
    company_id: Uuid,
    limit: i64,
) -> Result<(Vec<PartyToMatch>, i64)> {
    let rows = sqlx::query(
        "select p.id, p.party_no, p.name, p.kind,
                sum(o.remaining_ore) filter (where o.remaining_ore > 0)::bigint as pos,
                sum(o.remaining_ore) filter (where o.remaining_ore < 0)::bigint as neg,
                count(*) over () as total
         from subledger_entry_open o
         join party p on p.id = o.party_id
         where p.company_id = $1 and o.remaining_ore <> 0
         group by p.id, p.party_no, p.name, p.kind
         having bool_or(o.remaining_ore > 0) and bool_or(o.remaining_ore < 0)
         order by p.name, p.party_no
         limit $2",
    )
    .bind(company_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let total = rows.first().map(|r| r.get::<i64, _>("total")).unwrap_or(0);
    Ok((
        rows.iter()
            .map(|r| PartyToMatch {
                party_id: r.get("id"),
                party_no: r.get("party_no"),
                name: r.get("name"),
                kind: r.get("kind"),
                open_positive_ore: r.get("pos"),
                open_negative_ore: r.get("neg"),
            })
            .collect(),
        total,
    ))
}

/// One reskontro match on a party, both sides named — the history the
/// party page shows, and where a wrong match is undone.
#[derive(Debug)]
pub struct MatchRow {
    pub id: Uuid,
    pub entry_a: Uuid,
    pub entry_b: Uuid,
    pub voucher_a: String,
    pub date_a: NaiveDate,
    pub voucher_b: String,
    pub date_b: NaiveDate,
    pub amount_ore: i64,
    pub matched_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The match closes a currency-difference transfer posted by
    /// `match_valuta`: undoing it alone would leave that voucher behind.
    pub currency_settlement: bool,
}

pub async fn party_matches(
    pool: &PgPool,
    company_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<MatchRow>> {
    let rows = sqlx::query(
        "select m.id, m.entry_a, m.entry_b, m.amount_ore, m.matched_by, m.created_at,
                va.fiscal_year as ya, va.voucher_number as na, va.voucher_date as da,
                vb.fiscal_year as yb, vb.voucher_number as nb, vb.voucher_date as db,
                (m.valuta_cent is not null or exists (
                    select 1 from reskontro_match v
                    where v.valuta_cent is not null and v.created_at = m.created_at
                      and (v.entry_a in (m.entry_a, m.entry_b) or v.entry_b in (m.entry_a, m.entry_b))
                 )) as currency
         from reskontro_match m
         join entry ea on ea.id = m.entry_a
         join voucher va on va.id = ea.voucher_id
         join entry eb on eb.id = m.entry_b
         join voucher vb on vb.id = eb.voucher_id
         where va.company_id = $1 and ea.party_id = $2
         order by m.created_at desc, m.id desc
         limit 200",
    )
    .bind(company_id)
    .bind(party_id)
    .fetch_all(pool)
    .await?;
    let label = |y: i32, n: i64| format!("{y}-{n}");
    Ok(rows
        .iter()
        .map(|r| MatchRow {
            id: r.get("id"),
            entry_a: r.get("entry_a"),
            entry_b: r.get("entry_b"),
            voucher_a: label(r.get("ya"), r.get("na")),
            date_a: r.get("da"),
            voucher_b: label(r.get("yb"), r.get("nb")),
            date_b: r.get("db"),
            amount_ore: r.get("amount_ore"),
            matched_by: r.get("matched_by"),
            created_at: r.get("created_at"),
            currency_settlement: r.get("currency"),
        })
        .collect())
}
