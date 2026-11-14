//! Payment list and remittering (docs/betaling.md, #33).
//!
//! The list is built from OPEN leverandør-poster (reskontro remainders
//! — computed, never stored), creditor data is snapshotted onto the
//! run items at creation, approval renders and STORES the pain.001
//! file (hash-checked on every download, reproducible forever), and
//! settlement posts the utbetalingsbilag and closes every item's
//! reskontro-rest in ONE transaction — the bank import then matches
//! the debit against that voucher like any other.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::hash::{sha256, truncate_to_micros};
use regnmed_core::pain001::{
    Pain001Input, Payment, is_valid_account_number, normalize_account_number,
};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

/// Something the company owes and can pay from a list: an open
/// leverandør post (`entry_id`) or an approved receipt claim
/// (`expense_id`), never both.
#[derive(Debug)]
pub struct PayableItem {
    pub entry_id: Option<Uuid>,
    pub expense_id: Option<Uuid>,
    pub voucher_label: String,
    pub date: NaiveDate,
    pub description: Option<String>,
    /// The supplier's number; None for a claim.
    pub party_no: Option<String>,
    pub party_name: String,
    pub bank_account: Option<String>,
    /// What we owe on this post (positive).
    pub amount_ore: i64,
    /// Already part of an utkast/godkjent run.
    pub in_run: bool,
    /// From the supplier's document (payable_terms, newest row).
    pub due_date: Option<NaiveDate>,
    pub kid: Option<String>,
}

/// Due date and KID read off a supplier's document.
#[derive(Debug, Clone, Default)]
pub struct PayableTermsDraft {
    pub due_date: Option<NaiveDate>,
    pub kid: Option<String>,
}

impl PayableTermsDraft {
    pub fn is_empty(&self) -> bool {
        self.due_date.is_none() && self.kid.as_deref().is_none_or(|k| k.trim().is_empty())
    }

    fn validated_kid(&self) -> Result<Option<String>> {
        match self.kid.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            Some(kid) => {
                ensure!(regnmed_core::kid::is_valid(kid), "ugyldig KID {kid}");
                Ok(Some(kid.to_string()))
            }
            None => Ok(None),
        }
    }
}

/// Per supplier: what remains on open posts whose due date has passed,
/// as a positive amount owed. A SELECT over $1 = company id, usable as a
/// CTE body. Posts without a recorded due date are never overdue -- the
/// system does not guess a payment term.
pub(crate) const SUPPLIER_OVERDUE_SQL: &str = "
    select party_no, sum(-remaining_ore)::bigint as overdue_ore
    from (
        select p.party_no, terms.due_date,
               e.amount_ore
               - coalesce((select sum(m.amount_ore) from reskontro_match m
                           where m.entry_a = e.id), 0)::bigint
               + coalesce((select sum(m.amount_ore) from reskontro_match m
                           where m.entry_b = e.id), 0)::bigint as remaining_ore
        from entry e
        join party p on p.id = e.party_id
        left join lateral (
            select t.due_date from payable_terms t
            where t.entry_id = e.id
            order by t.recorded_at desc, t.id desc limit 1
        ) terms on true
        where p.company_id = $1 and p.kind = 'leverandor'
    ) posts
    where remaining_ore < 0 and due_date < current_date
    group by party_no";

/// Records due date and KID on every supplier entry of a voucher, inside
/// the caller's transaction -- the inbox posting, so a supplier invoice
/// and its terms exist together or not at all. Refuses a voucher with no
/// supplier line: terms on anything else would be stored and never read.
pub async fn record_payable_terms_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    voucher_id: Uuid,
    terms: &PayableTermsDraft,
    recorded_by: &str,
) -> Result<()> {
    if terms.is_empty() {
        return Ok(());
    }
    let kid = terms.validated_kid()?;
    let entries: Vec<Uuid> = sqlx::query_scalar(
        "select e.id from entry e join party p on p.id = e.party_id
         where e.voucher_id = $1 and p.company_id = $2 and p.kind = 'leverandor'",
    )
    .bind(voucher_id)
    .bind(company_id)
    .fetch_all(&mut **tx)
    .await?;
    ensure!(
        !entries.is_empty(),
        "forfallsdato og KID gjelder en leverandørpost, men bilaget har ingen linje med leverandør"
    );
    for entry_id in entries {
        insert_terms(
            &mut **tx,
            company_id,
            entry_id,
            terms.due_date,
            kid.as_deref(),
            recorded_by,
        )
        .await?;
    }
    Ok(())
}

/// Sets or corrects due date and KID on one existing supplier entry (a
/// posting booked before terms were kept, or a typing error). A new row;
/// the newest applies and the earlier ones remain as history.
pub async fn record_payable_terms(
    pool: &PgPool,
    company_id: Uuid,
    entry_id: Uuid,
    terms: &PayableTermsDraft,
    recorded_by: &str,
) -> Result<()> {
    ensure!(!terms.is_empty(), "oppgi forfallsdato eller KID");
    let kid = terms.validated_kid()?;
    let supplier: Option<bool> = sqlx::query_scalar(
        "select p.kind = 'leverandor' from entry e
         join voucher v on v.id = e.voucher_id
         join party p on p.id = e.party_id
         where e.id = $1 and v.company_id = $2",
    )
    .bind(entry_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    ensure!(supplier.is_some(), "ingen slik post med part i selskapet");
    ensure!(
        supplier == Some(true),
        "forfallsdato og KID føres bare på leverandørposter"
    );
    insert_terms(
        pool,
        company_id,
        entry_id,
        terms.due_date,
        kid.as_deref(),
        recorded_by,
    )
    .await
}

pub(crate) async fn insert_terms<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    company_id: Uuid,
    entry_id: Uuid,
    due_date: Option<NaiveDate>,
    kid: Option<&str>,
    recorded_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into payable_terms (id, company_id, entry_id, due_date, kid, recorded_by)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(entry_id)
    .bind(due_date)
    .bind(kid)
    .bind(recorded_by)
    .execute(executor)
    .await?;
    Ok(())
}

/// Open leverandør-poster: credit remainders on leverandør-reskontro
/// accounts — what the company owes, post for post — followed by the
/// approved receipt claims owed to employees. Kjøregodtgjørelse is not
/// here: it is paid with pay (`payroll::run_payroll`).
pub async fn payable_items(pool: &PgPool, company_id: Uuid) -> Result<Vec<PayableItem>> {
    let rows = sqlx::query(
        "select e.id, v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description,
                p.party_no, p.name as party_name, p.bank_account,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore,
                exists (select 1 from payment_run_item i
                        join payment_run r on r.id = i.run_id
                        where i.entry_id = e.id
                          and r.status in ('utkast', 'godkjent')) as i_kjoring,
                terms.due_date, terms.kid
         from entry e
         join voucher v on v.id = e.voucher_id
         join party p on p.id = e.party_id
         left join lateral (
             select t.due_date, t.kid from payable_terms t
             where t.entry_id = e.id
             order by t.recorded_at desc, t.id desc limit 1
         ) terms on true
         where v.company_id = $1 and p.kind = 'leverandor'
         -- What falls due first comes first; undated posts after, oldest first.
         order by terms.due_date nulls last, v.voucher_date, v.voucher_number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter(|r| r.get::<i64, _>("remaining_ore") < 0)
        .map(|r| PayableItem {
            entry_id: Some(r.get("id")),
            expense_id: None,
            voucher_label: format!(
                "{}-{}",
                r.get::<i32, _>("fiscal_year"),
                r.get::<i64, _>("voucher_number")
            ),
            date: r.get("voucher_date"),
            description: r.get("description"),
            party_no: Some(r.get("party_no")),
            party_name: r.get("party_name"),
            bank_account: r.get("bank_account"),
            amount_ore: -r.get::<i64, _>("remaining_ore"),
            in_run: r.get("i_kjoring"),
            due_date: r.get("due_date"),
            kid: r.get("kid"),
        })
        .chain(payable_claims(pool, company_id).await?)
        .collect())
}

/// Approved receipt claims, paid to the claimant's account from the
/// employee register (the only place regnmed keeps a person's
/// kontonummer). A claim is paid whole: its status flips to utbetalt.
async fn payable_claims(pool: &PgPool, company_id: Uuid) -> Result<Vec<PayableItem>> {
    let rows = sqlx::query(&format!(
        "select x.id, x.dato, x.beskrivelse, x.belop_ore, v.fiscal_year, v.voucher_number,
                coalesce(p.name, '') as name, emp.bank_account, {in_run} as i_kjoring
         from expense x
         join voucher v on v.id = x.voucher_id
         left join person p on p.id = x.person_id
         left join employee emp on emp.company_id = x.company_id and emp.person_id = x.person_id
         where x.company_id = $1 and x.kind = 'utlegg' and x.status = 'godkjent'
         order by x.dato, x.id",
        in_run = crate::expenses::in_open_run("x"),
    ))
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PayableItem {
            entry_id: None,
            expense_id: Some(r.get("id")),
            voucher_label: format!(
                "{}-{}",
                r.get::<i32, _>("fiscal_year"),
                r.get::<i64, _>("voucher_number")
            ),
            date: r.get("dato"),
            description: Some(format!("Utlegg: {}", r.get::<String, _>("beskrivelse"))),
            party_no: None,
            party_name: r.get("name"),
            bank_account: r.get("bank_account"),
            amount_ore: r.get("belop_ore"),
            in_run: r.get("i_kjoring"),
            due_date: None,
            kid: None,
        })
        .collect())
}

/// One payment: give `entry_id` (a supplier post) or `expense_id` (a
/// receipt claim), not both.
#[derive(Debug, Clone)]
pub struct PaymentItemDraft {
    pub entry_id: Option<Uuid>,
    pub expense_id: Option<Uuid>,
    /// None = the whole open remainder.
    pub amount_ore: Option<i64>,
    pub kid: Option<String>,
    pub message: Option<String>,
}

/// Creates a betalingsliste (status utkast): every item validated
/// against its OPEN remainder, the creditor's name and kontonummer
/// snapshotted onto the row. Approving is a separate audited action.
pub async fn create_run(
    pool: &PgPool,
    company_id: Uuid,
    items: &[PaymentItemDraft],
    debtor_account: Option<&str>,
    execution_date: NaiveDate,
    created_by: &str,
    created_by_person: Uuid,
) -> Result<Uuid> {
    ensure!(!items.is_empty(), "betalingslisten er tom");
    let debitor = match debtor_account {
        Some(account) => normalize_account_number(account),
        None => {
            let account: Option<String> =
                sqlx::query_scalar("select bank_account from company where id = $1")
                    .bind(company_id)
                    .fetch_one(pool)
                    .await?;
            account
                .map(|k| normalize_account_number(&k))
                .context("selskapet mangler kontonummer — sett det under Firmaopplysninger")?
        }
    };
    ensure!(
        is_valid_account_number(&debitor),
        "debitorkonto {debitor} er ikke et gyldig kontonummer"
    );

    let payable = payable_items(pool, company_id).await?;
    let mut tx = pool.begin().await?;
    let run_id = Uuid::now_v7();
    sqlx::query(
        "insert into payment_run (id, company_id, debitor_konto, execution_date,
                                  created_by, created_by_person)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(&debitor)
    .bind(execution_date)
    .bind(created_by)
    .bind(created_by_person)
    .execute(&mut *tx)
    .await?;
    for (i, item) in items.iter().enumerate() {
        ensure!(
            item.entry_id.is_some() != item.expense_id.is_some(),
            "linje {}: oppgi enten en leverandørpost eller et utleggskrav",
            i + 1
        );
        let open = payable
            .iter()
            .find(|p| {
                (item.entry_id.is_some() && p.entry_id == item.entry_id)
                    || (item.expense_id.is_some() && p.expense_id == item.expense_id)
            })
            .with_context(|| {
                if item.entry_id.is_some() {
                    format!("linje {}: ingen åpen leverandørpost", i + 1)
                } else {
                    format!("linje {}: utleggskravet er ikke godkjent og ubetalt", i + 1)
                }
            })?;
        // A post in an utkast or godkjent list is already on its way to
        // the bank; a second list would pay it twice.
        ensure!(
            !open.in_run,
            "linje {}: posten ligger allerede i en betalingsliste som ikke er utbetalt",
            i + 1
        );
        let amount = item.amount_ore.unwrap_or(open.amount_ore);
        // A claim is paid whole: settling it flips it to utbetalt.
        ensure!(
            open.expense_id.is_none() || amount == open.amount_ore,
            "linje {}: et utleggskrav betales i sin helhet ({} øre)",
            i + 1,
            open.amount_ore
        );
        ensure!(
            amount > 0 && amount <= open.amount_ore,
            "linje {}: beløpet må være 1..{} øre",
            i + 1,
            open.amount_ore
        );
        let account = open
            .bank_account
            .as_deref()
            .map(normalize_account_number)
            .filter(|k| is_valid_account_number(k))
            .with_context(|| match &open.party_no {
                Some(no) => format!(
                    "linje {}: leverandør {no} mangler gyldig kontonummer",
                    i + 1
                ),
                None => format!(
                    "linje {}: {} mangler gyldig kontonummer i ansattregisteret",
                    i + 1,
                    open.party_name
                ),
            })?;
        // The KID the supplier asked for, unless the caller gave one: a
        // payment without it is one the supplier has to match by hand.
        // ... but a caller who wrote a message chose an unstructured
        // reference, and that choice stands.
        let kid = item
            .kid
            .clone()
            .or_else(|| open.kid.clone().filter(|_| item.message.is_none()));
        if let Some(kid) = &kid {
            ensure!(
                regnmed_core::kid::is_valid(kid),
                "linje {}: ugyldig KID {kid}",
                i + 1
            );
        }
        // The list above was read before this transaction. A claim paid by
        // hand since then must not reach the bank file as well: lock it
        // (pay_expense takes the same lock) and read its status again.
        if let Some(expense_id) = item.expense_id {
            let status: String = sqlx::query_scalar(
                "select status from expense where id = $1 and company_id = $2 for update",
            )
            .bind(expense_id)
            .bind(company_id)
            .fetch_one(&mut *tx)
            .await?;
            ensure!(
                status == "godkjent",
                "linje {}: utleggskravet er {status} i mellomtiden",
                i + 1
            );
        }
        sqlx::query(
            "insert into payment_run_item (id, run_id, entry_id, expense_id, belop_ore,
                                           kreditor_navn, kreditor_konto, kid, melding)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(Uuid::now_v7())
        .bind(run_id)
        .bind(item.entry_id)
        .bind(item.expense_id)
        .bind(amount)
        .bind(&open.party_name)
        .bind(&account)
        .bind(&kid)
        .bind(
            item.message
                .clone()
                .or_else(|| open.description.clone())
                .filter(|_| kid.is_none()),
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(run_id)
}

/// Approves the list for export: renders the pain.001 file, stores it
/// with its SHA-256 and flips utkast → godkjent, in one transaction.
/// The approver is recorded separately from the creator; when the
/// attestering policy is active (docs/attestering.md, #47) the
/// approver MUST be a different person than the creator — four eyes on
/// money out, enforced here inside the transaction.
pub async fn approve_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    approved_by: &str,
    approved_by_person: Uuid,
) -> Result<[u8; 32]> {
    let mut tx = pool.begin().await?;
    let run = sqlx::query(
        "select r.status, r.debitor_konto, r.execution_date, r.created_by_person,
                c.name as company_name
         from payment_run r join company c on c.id = r.company_id
         where r.id = $1 and r.company_id = $2 for update of r",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such payment run")?;
    let status: String = run.get("status");
    ensure!(status == "utkast", "kjøringen er {status}");
    // An employee's money is released by a person, never by an agent —
    // the same rule as approving, rejecting and paying the claim itself
    // (docs/utlegg.md «Agenten»).
    let has_claims: bool = sqlx::query_scalar(
        "select exists (select 1 from payment_run_item where run_id = $1 and expense_id is not null)",
    )
    .bind(run_id)
    .fetch_one(&mut *tx)
    .await?;
    ensure!(
        !has_claims || !crate::is_machine(pool, approved_by_person).await?,
        "betalingslisten har utleggskrav, og et ansatts penger frigis av et menneske — \
         en agent kan ikke godkjenne den"
    );

    if let Some(policy) = crate::approval::current_policy(&mut *tx, company_id).await?
        && policy.active
    {
        let creator: Option<Uuid> = run.get("created_by_person");
        match creator {
            None => bail!(
                "kjøringen er opprettet uten registrert person og kan ikke godkjennes \
                 under aktiv attestering — lag betalingslisten på nytt"
            ),
            Some(person) if person == approved_by_person => bail!(
                "betalingslisten må godkjennes av en annen enn den som opprettet den (fire øyne)"
            ),
            Some(_) => {}
        }
    }

    let items = run_items(&mut tx, run_id).await?;
    ensure!(!items.is_empty(), "kjøringen har ingen linjer");
    let approved_at = truncate_to_micros(chrono::Utc::now());
    let input = Pain001Input {
        msg_id: format!("regnmed-{run_id}"),
        created: approved_at,
        sender_name: run.get("company_name"),
        debtor_account: run.get("debitor_konto"),
        execution_date: run.get("execution_date"),
        payments: items,
    };
    let xml = regnmed_core::pain001::render(&input);
    let digest = sha256(xml.as_bytes());
    sqlx::query(
        "update payment_run set status = 'godkjent', approved_by = $3, approved_at = $4,
                                file = $5, file_sha256 = $6
         where id = $1 and company_id = $2",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(approved_by)
    .bind(approved_at)
    .bind(xml.as_bytes())
    .bind(digest.as_slice())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(digest)
}

async fn run_items(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
) -> Result<Vec<Payment>> {
    let rows = sqlx::query(
        "select id, belop_ore, kreditor_navn, kreditor_konto, kid, melding
         from payment_run_item where run_id = $1 order by created_at, id",
    )
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Payment {
            end_to_end_id: format!("regnmed-{}", r.get::<Uuid, _>("id")),
            amount_ore: r.get("belop_ore"),
            creditor_name: r.get("kreditor_navn"),
            creditor_account: r.get("kreditor_konto"),
            kid: r.get("kid"),
            message: r.get("melding"),
        })
        .collect())
}

/// The stored pain.001 file, integrity-checked on every download.
pub async fn run_file(pool: &PgPool, company_id: Uuid, run_id: Uuid) -> Result<(String, Vec<u8>)> {
    let row = sqlx::query(
        "select file, file_sha256 from payment_run
         where id = $1 and company_id = $2 and file is not null",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ingen fil — kjøringen er ikke godkjent")?;
    let file: Vec<u8> = row.get("file");
    let stored: Vec<u8> = row.get("file_sha256");
    ensure!(
        sha256(&file).to_vec() == stored,
        "payment file fails integrity check"
    );
    Ok((format!("pain001-{run_id}.xml"), file))
}

#[derive(Debug)]
pub struct SettledRun {
    pub voucher_number: i64,
    pub fiscal_year: i32,
}

/// Records the kjøring as executed: ONE utbetalingsbilag (debet each
/// leverandør-post's account with its party, kredit the ledger bank
/// account) and a reskontro match per item — everything in one
/// transaction, so the open posts close exactly when the payment is
/// booked. The bank import's debit then matches this voucher through
/// the ordinary engine, closing the circle.
pub async fn settle_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    date: NaiveDate,
    bank_account: &str,
    settled_by: &str,
) -> Result<SettledRun> {
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar(
        "select status from payment_run where id = $1 and company_id = $2 for update",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such payment run")?;
    ensure!(
        status == "godkjent",
        "bare godkjente kjøringer kan registreres utbetalt (kjøringen er {status})"
    );

    let items = sqlx::query(
        "select i.id, i.entry_id, i.expense_id, i.belop_ore,
                coalesce(a.number, x.motkonto) as account_number, p.party_no
         from payment_run_item i
         left join entry e on e.id = i.entry_id
         left join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join expense x on x.id = i.expense_id
         where i.run_id = $1
         order by i.created_at, i.id",
    )
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await?;
    ensure!(!items.is_empty(), "kjøringen har ingen linjer");

    let total: i64 = items.iter().map(|r| r.get::<i64, _>("belop_ore")).sum();
    let mut entries: Vec<EntryDraft> = items
        .iter()
        .map(|r| EntryDraft {
            account_number: r.get("account_number"),
            amount: Ore(r.get::<i64, _>("belop_ore")),
            vat_code: None,
            description: Some("Remittering".into()),
            party_no: r.get("party_no"),
            department: None,
            project: None,
            currency: None,
        })
        .collect();
    entries.push(EntryDraft {
        account_number: bank_account.to_string(),
        amount: Ore(-total),
        vat_code: None,
        description: Some(format!("Betalingskjøring {run_id}")),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    });
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("Utbetaling betalingsliste {run_id}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, settled_by).await?;

    // Match each supplier line against its original open post; mark each
    // claim paid by this voucher.
    for (line_no, item) in items.iter().enumerate() {
        if let Some(expense_id) = item.get::<Option<Uuid>, _>("expense_id") {
            // `status = 'godkjent'`: a claim paid by hand since the list was
            // made is not paid twice — the settlement fails instead.
            let paid = sqlx::query(
                "update expense set status = 'utbetalt', utbetalt_voucher_id = $2,
                                    utbetalt_at = now()
                 where id = $1 and status = 'godkjent'",
            )
            .bind(expense_id)
            .bind(posted.id)
            .execute(&mut *tx)
            .await?;
            ensure!(
                paid.rows_affected() == 1,
                "utleggskravet på linje {} er allerede utbetalt",
                line_no + 1
            );
            continue;
        }
        let debit_entry: Uuid =
            sqlx::query_scalar("select id from entry where voucher_id = $1 and line_no = $2")
                .bind(posted.id)
                .bind((line_no + 1) as i32)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(debit_entry)
        .bind(item.get::<Option<Uuid>, _>("entry_id"))
        .bind(item.get::<i64, _>("belop_ore"))
        .bind(settled_by)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "update payment_run set status = 'utbetalt', settled_voucher_id = $3, settled_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(posted.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(SettledRun {
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
    })
}

/// Cancels an utkast (one-way, audited). Approved runs are never
/// cancelled — the file exists; what happened in the bank is a
/// settlement question.
pub async fn cancel_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    cancelled_by: &str,
) -> Result<()> {
    let updated = sqlx::query(
        "update payment_run set status = 'annullert', annullert_by = $3, annullert_at = now()
         where id = $1 and company_id = $2 and status = 'utkast'",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(cancelled_by)
    .execute(pool)
    .await?;
    if updated.rows_affected() != 1 {
        bail!("kjøringen finnes ikke eller er ikke lenger utkast");
    }
    Ok(())
}

#[derive(Debug)]
pub struct PaymentRunRow {
    pub id: Uuid,
    pub status: String,
    pub execution_date: NaiveDate,
    pub sum_ore: i64,
    pub count: i64,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub approved_by: Option<String>,
    pub settled_voucher: Option<String>,
}

pub async fn list_payment_runs(pool: &PgPool, company_id: Uuid) -> Result<Vec<PaymentRunRow>> {
    let rows = sqlx::query(
        "select r.id, r.status, r.execution_date, r.created_by, r.created_at, r.approved_by,
                v.fiscal_year, v.voucher_number,
                coalesce((select sum(i.belop_ore) from payment_run_item i
                          where i.run_id = r.id), 0)::bigint as sum_ore,
                (select count(*) from payment_run_item i where i.run_id = r.id) as antall
         from payment_run r
         left join voucher v on v.id = r.settled_voucher_id
         where r.company_id = $1
         order by r.created_at desc
         limit 100",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PaymentRunRow {
            id: r.get("id"),
            status: r.get("status"),
            execution_date: r.get("execution_date"),
            sum_ore: r.get("sum_ore"),
            count: r.get("antall"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            approved_by: r.get("approved_by"),
            settled_voucher: match (
                r.get::<Option<i32>, _>("fiscal_year"),
                r.get::<Option<i64>, _>("voucher_number"),
            ) {
                (Some(year), Some(no)) => Some(format!("{year}-{no}")),
                _ => None,
            },
        })
        .collect())
}
