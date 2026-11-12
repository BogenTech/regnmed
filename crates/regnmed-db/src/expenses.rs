//! Utlegg og kjøregodtgjørelse (docs/utlegg.md, #42): claims with
//! immutable content and one-way decisions (the innboks discipline),
//! kjøregodtgjørelse computed from the dated satsregister AT
//! SUBMISSION (the row is evidence), approval posting kostnad mot
//! mellomregning with the receipt attached to the voucher in ONE
//! transaction, and utbetaling posting mellomregning mot bank.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::expenses::mileage_allowance;
use regnmed_core::hash::sha256;
use regnmed_core::rates::value_on;
use regnmed_core::vat::{rate_on, split_gross};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::attachment::add_attachment_in;
use crate::ledger::post_voucher_in;
use crate::rates::load_rates;
use crate::vat::load_vat_rates;

/// Registers an utlegg with its receipt. The receipt is immutable from
/// this moment (SHA-256 stored, trigger + grants) and follows the
/// claim onto the voucher at approval — oppbevaringsplikten covers the
/// original documentation, not a copy of our making.
#[allow(clippy::too_many_arguments)]
pub async fn create_expense(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    date: NaiveDate,
    description: &str,
    amount_ore: i64,
    filename: &str,
    content_type: &str,
    content: &[u8],
    created_by: &str,
) -> Result<Uuid> {
    ensure!(amount_ore > 0, "beløpet må være positivt");
    ensure!(!content.is_empty(), "kvitteringen er tom");
    ensure!(!description.trim().is_empty(), "skriv hva utlegget gjaldt");
    let digest = sha256(content).to_vec();
    // Outside the transaction: with an object store this is a network
    // PUT, and content addressing makes a refused duplicate harmless.
    let in_row = crate::blob::blobs().store(content).await?;
    let mut tx = pool.begin().await?;
    // The same receipt twice is a double-click or a resubmission, never
    // two expenses — refused while the first claim stands. A REJECTED
    // claim does not block: resubmitting with a better description is
    // the normal way back. The advisory lock makes two simultaneous
    // submissions of the same bytes take turns.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text || encode($2, 'hex'), 0))")
        .bind(company_id)
        .bind(&digest)
        .execute(&mut *tx)
        .await?;
    let earlier: Option<(NaiveDate, String)> = sqlx::query_as(
        "select dato, beskrivelse from expense
         where company_id = $1 and receipt_sha256 = $2 and status <> 'avvist'
         limit 1",
    )
    .bind(company_id)
    .bind(&digest)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((date, what)) = earlier {
        bail!(
            "denne kvitteringen er allerede sendt inn («{what}», {})",
            date.format("%d.%m.%Y")
        );
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, kind, dato, beskrivelse, belop_ore,
                              receipt_filename, receipt_content_type, receipt_content,
                              receipt_sha256, created_by)
         values ($1, $2, $3, 'utlegg', $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(date)
    .bind(description)
    .bind(amount_ore)
    .bind(filename)
    .bind(content_type)
    .bind(in_row)
    .bind(digest)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Statens km-sats and its trekkfri part on `date`, from the
/// satsregister — what a kjøring that day is paid. Outside the
/// register's coverage: an error, never a guess.
pub async fn mileage_rates(pool: &PgPool, date: NaiveDate) -> Result<(i64, i64)> {
    let rates = load_rates(pool).await?;
    let rate = value_on(&rates, "km_godtgjorelse", date)
        .with_context(|| format!("ingen km-sats for {date} i satsregisteret"))?;
    let trekkfri = value_on(&rates, "km_godtgjorelse_trekkfri", date)
        .with_context(|| format!("ingen trekkfri km-sats for {date} i satsregisteret"))?;
    Ok((rate, trekkfri))
}

/// Registers one kjøring. The satser valid on `dato` are resolved from
/// the satsregister NOW and stored on the row — a claim outside the
/// register's coverage is refused loudly, never guessed.
pub async fn create_expense_run(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    date: NaiveDate,
    description: &str,
    km: i64,
    created_by: &str,
) -> Result<(Uuid, i64, i64)> {
    ensure!(km > 0, "oppgi antall kilometer");
    ensure!(!description.trim().is_empty(), "skriv strekning og formål");
    let (rate, trekkfri_sats) = mileage_rates(pool, date).await?;
    let beregnet = mileage_allowance(km, rate, trekkfri_sats);
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, kind, dato, beskrivelse, belop_ore,
                              km, sats_ore_per_km, trekkfri_ore, trekkpliktig_ore, created_by)
         values ($1, $2, $3, 'kjoring', $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(date)
    .bind(description)
    .bind(beregnet.amount_ore)
    .bind(km)
    .bind(rate)
    .bind(beregnet.tax_free_ore)
    .bind(beregnet.taxable_ore)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok((id, beregnet.amount_ore, beregnet.taxable_ore))
}

#[derive(Debug)]
pub struct ExpenseRow {
    pub id: Uuid,
    pub person_name: String,
    pub own: bool,
    pub kind: String,
    pub date: NaiveDate,
    pub description: String,
    pub amount_ore: i64,
    pub km: Option<i64>,
    pub sats_ore_per_km: Option<i64>,
    pub trekkfri_ore: Option<i64>,
    pub trekkpliktig_ore: Option<i64>,
    pub receipt_filename: Option<String>,
    pub status: String,
    pub avvist_note: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The kostnad voucher posted at approval.
    pub voucher: Option<String>,
    /// The utbetaling voucher.
    pub paid_voucher: Option<String>,
    pub utbetalt_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Which claims a list shows. `status` is a TAB, not a stored value:
/// `apne` = innsendt + godkjent (money not yet out), or one of the
/// stored statuses. `expense_id` picks one claim (the detail page).
#[derive(Debug, Default, Clone)]
pub struct ExpenseFilter {
    pub status: Option<String>,
    pub expense_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Default)]
pub struct ExpenseCounts {
    pub innsendt: i64,
    pub godkjent: i64,
    pub utbetalt: i64,
    pub avvist: i64,
}

#[derive(Debug)]
pub struct ExpensePage {
    pub rows: Vec<ExpenseRow>,
    /// Rows matching the filter, before paging.
    pub total: i64,
    /// Per status over the viewer's whole scope — the tab counters.
    pub counts: ExpenseCounts,
}

/// `only_own` scopes the list to the viewer's own claims — what an
/// ansatt gets. For everyone else the flag is false and the list is the
/// whole company's, with `own` marking the viewer's own rows.
pub async fn list_expenses(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
    only_own: bool,
    filter: &ExpenseFilter,
) -> Result<ExpensePage> {
    let statuses: Option<Vec<&str>> = match filter.status.as_deref() {
        None | Some("") => None,
        Some("apne") => Some(vec!["innsendt", "godkjent"]),
        Some(s @ ("innsendt" | "godkjent" | "utbetalt" | "avvist")) => Some(vec![s]),
        Some(other) => bail!("ukjent status «{other}»"),
    };
    let rows_query = sqlx::query(
        "select e.id, coalesce(p.name, p.oidc_sub) as person_name, e.person_id = $2 as own,
                e.kind, e.dato, e.beskrivelse, e.belop_ore, e.km, e.sats_ore_per_km,
                e.trekkfri_ore, e.trekkpliktig_ore, e.receipt_filename, e.status,
                e.avvist_note, e.decided_by, e.decided_at, e.utbetalt_at, e.created_at,
                v.fiscal_year, v.voucher_number,
                pv.fiscal_year as paid_year, pv.voucher_number as paid_number
         from expense e
         join person p on p.id = e.person_id
         left join voucher v on v.id = e.voucher_id
         left join voucher pv on pv.id = e.utbetalt_voucher_id
         where e.company_id = $1 and ($3 = false or e.person_id = $2)
           and ($4::text[] is null or e.status = any($4))
           and ($5::uuid is null or e.id = $5)
         order by e.created_at desc
         limit $6 offset $7",
    )
    .bind(company_id)
    .bind(viewer)
    .bind(only_own)
    .bind(statuses.clone())
    .bind(filter.expense_id)
    .bind(filter.limit.clamp(1, 500))
    .bind(filter.offset.max(0))
    .fetch_all(pool);
    let counts_query = sqlx::query(
        "select status, count(*) as n from expense
         where company_id = $1 and ($3 = false or person_id = $2)
         group by status",
    )
    .bind(company_id)
    .bind(viewer)
    .bind(only_own)
    .fetch_all(pool);
    let (rows, count_rows) = tokio::try_join!(rows_query, counts_query)?;
    let voucher = |r: &sqlx::postgres::PgRow, year: &str, no: &str| match (
        r.get::<Option<i32>, _>(year),
        r.get::<Option<i64>, _>(no),
    ) {
        (Some(year), Some(no)) => Some(format!("{year}-{no}")),
        _ => None,
    };
    let rows: Vec<ExpenseRow> = rows
        .iter()
        .map(|r| ExpenseRow {
            id: r.get("id"),
            person_name: r.get("person_name"),
            own: r.get("own"),
            kind: r.get("kind"),
            date: r.get("dato"),
            description: r.get("beskrivelse"),
            amount_ore: r.get("belop_ore"),
            km: r.get("km"),
            sats_ore_per_km: r.get("sats_ore_per_km"),
            trekkfri_ore: r.get("trekkfri_ore"),
            trekkpliktig_ore: r.get("trekkpliktig_ore"),
            receipt_filename: r.get("receipt_filename"),
            status: r.get("status"),
            avvist_note: r.get("avvist_note"),
            decided_by: r.get("decided_by"),
            decided_at: r.get("decided_at"),
            voucher: voucher(r, "fiscal_year", "voucher_number"),
            paid_voucher: voucher(r, "paid_year", "paid_number"),
            utbetalt_at: r.get("utbetalt_at"),
            created_at: r.get("created_at"),
        })
        .collect();

    let mut counts = ExpenseCounts::default();
    for r in &count_rows {
        let n: i64 = r.get("n");
        match r.get::<String, _>("status").as_str() {
            "innsendt" => counts.innsendt = n,
            "godkjent" => counts.godkjent = n,
            "utbetalt" => counts.utbetalt = n,
            "avvist" => counts.avvist = n,
            _ => {}
        }
    }
    // The total comes from the counts, not from the page: a page past
    // the end still knows how many rows there are to page back to.
    let total = if filter.expense_id.is_some() {
        rows.len() as i64
    } else {
        let all = ["innsendt", "godkjent", "utbetalt", "avvist"];
        let n = |st: &str| match st {
            "innsendt" => counts.innsendt,
            "godkjent" => counts.godkjent,
            "utbetalt" => counts.utbetalt,
            _ => counts.avvist,
        };
        statuses
            .as_deref()
            .unwrap_or(&all[..])
            .iter()
            .map(|st| n(st))
            .sum()
    };
    Ok(ExpensePage {
        rows,
        total,
        counts,
    })
}

/// The receipt, integrity-checked against the stored SHA-256 on every
/// read — a claim whose documentation cannot be verified is an error,
/// never silently served.
/// Who submitted a claim — the caller needs it to decide whether a
/// viewer with only `UTLEGG_LES_EGNE` may see it.
pub async fn expense_owner(pool: &PgPool, company_id: Uuid, expense_id: Uuid) -> Result<Uuid> {
    sqlx::query_scalar("select person_id from expense where id = $1 and company_id = $2")
        .bind(expense_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such utlegg")
}

pub async fn expense_receipt(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<(String, String, Vec<u8>)> {
    let row = sqlx::query(
        "select receipt_filename, receipt_content_type, receipt_content, receipt_sha256
         from expense where id = $1 and company_id = $2 and kind = 'utlegg'",
    )
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such utlegg")?;
    let stored: Vec<u8> = row.get("receipt_sha256");
    let content = crate::blob::blobs()
        .load(row.get("receipt_content"), &stored)
        .await
        .with_context(|| format!("receipt of expense {expense_id}"))?;
    Ok((
        row.get("receipt_filename"),
        row.get("receipt_content_type"),
        content,
    ))
}

#[derive(Debug)]
pub struct ApprovedExpense {
    pub voucher_number: i64,
    pub fiscal_year: i32,
    /// Set for kjøring with a trekkpliktig del: the honest warning the
    /// caller must surface (lønnsinnberetning is not built yet).
    pub warning: Option<String>,
}

/// Approves a claim in ONE transaction: the kostnad voucher (utlegg:
/// netto + eventuell inngående mva mot brutto; kjøring: hele beløpet),
/// kredit mellomregning, the receipt copied onto the voucher as an
/// attachment, and the one-way status flip. Rows lock FOR UPDATE so a
/// claim can never be approved twice.
#[allow(clippy::too_many_arguments)]
pub async fn approve_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    account: &str,
    vat_code: Option<&str>,
    vat_account: &str,
    contra_account: &str,
    decided_by: &str,
    decided_by_person: Uuid,
) -> Result<ApprovedExpense> {
    let mut tx = pool.begin().await?;
    let expense = sqlx::query(
        "select kind, dato, beskrivelse, belop_ore, km, sats_ore_per_km, trekkpliktig_ore,
                receipt_filename, receipt_content_type, receipt_content, receipt_sha256,
                status, person_id,
                coalesce((select p.name from person p where p.id = expense.person_id), '') as navn
         from expense where id = $1 and company_id = $2 for update",
    )
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such expense")?;
    let status: String = expense.get("status");
    ensure!(status == "innsendt", "kravet er allerede {status}");

    // Self-approval was allowed in v1 (#42); with attestering active
    // (docs/attestering.md, #47) the approver must be someone other than
    // the person who submitted the claim.
    if let Some(policy) = crate::approval::current_policy(&mut *tx, company_id).await?
        && policy.active
    {
        let innsender: Uuid = expense.get("person_id");
        ensure!(
            innsender != decided_by_person,
            "utleggskrav kan ikke godkjennes av den som sendte det inn (fire øyne)"
        );
    }
    let kind: String = expense.get("kind");
    let date: NaiveDate = expense.get("dato");
    let amount: i64 = expense.get("belop_ore");
    let description: String = expense.get("beskrivelse");
    let name: String = expense.get("navn");

    let mut entries = Vec::new();
    let mut warning = None;
    match kind.as_str() {
        "utlegg" => {
            // Deducting input VAT needs the registration too (#99) —
            // otherwise approval quietly books a deduction the company
            // has no right to, and the cost comes out too low.
            if vat_code.is_some_and(crate::settings::requires_vat_registration) {
                crate::settings::require_vat_registered(pool, company_id, date).await?;
            }
            let (net, vat) = match vat_code {
                Some(code) => {
                    let rate_class: String =
                        sqlx::query_scalar("select rate_class from vat_code where code = $1")
                            .bind(code)
                            .fetch_optional(pool)
                            .await?
                            .with_context(|| format!("unknown vat code {code}"))?;
                    let rates = load_vat_rates(pool).await?;
                    let rate_bp = rate_on(&rates, &rate_class, date)
                        .with_context(|| format!("no rate for {date}"))?;
                    split_gross(amount, rate_bp)
                }
                None => (amount, 0),
            };
            entries.push(EntryDraft {
                account_number: account.to_string(),
                amount: Ore(net),
                vat_code: vat_code.map(str::to_string),
                description: Some(description.clone()),
                party_no: None,
                department: None,
                project: None,
                currency: None,
            });
            if vat != 0 {
                entries.push(EntryDraft {
                    account_number: vat_account.to_string(),
                    amount: Ore(vat),
                    vat_code: None,
                    description: Some("Inngående mva".into()),
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                });
            }
        }
        _ => {
            ensure!(vat_code.is_none(), "kjøregodtgjørelse har ingen mva");
            let km: i64 = expense.get::<Option<i64>, _>("km").unwrap_or(0);
            let rate: i64 = expense
                .get::<Option<i64>, _>("sats_ore_per_km")
                .unwrap_or(0);
            entries.push(EntryDraft {
                account_number: account.to_string(),
                amount: Ore(amount),
                vat_code: None,
                description: Some(format!(
                    "{description} — {km} km à {},{:02} kr",
                    rate / 100,
                    rate % 100
                )),
                party_no: None,
                department: None,
                project: None,
                currency: None,
            });
            let taxable: i64 = expense
                .get::<Option<i64>, _>("trekkpliktig_ore")
                .unwrap_or(0);
            if taxable > 0 {
                warning = Some(format!(
                    "trekkpliktig del {},{:02} kr skal lønnsinnberettes — regnmed tar den ikke med i a-meldingen ennå, så den må meldes særskilt",
                    taxable / 100,
                    taxable % 100
                ));
            }
        }
    }
    entries.push(EntryDraft {
        account_number: contra_account.to_string(),
        amount: Ore(-amount),
        vat_code: None,
        description: Some(format!("Skyldig {name}")),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    });

    let label = if kind == "utlegg" {
        "Utlegg"
    } else {
        "Kjøregodtgjørelse"
    };
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("{label} {name}: {description}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, decided_by).await?;

    if kind == "utlegg" {
        let stored: Vec<u8> = expense.get("receipt_sha256");
        let content = crate::blob::blobs()
            .load(expense.get("receipt_content"), &stored)
            .await?;
        add_attachment_in(
            &mut tx,
            company_id,
            posted.id,
            expense.get("receipt_filename"),
            expense.get("receipt_content_type"),
            &content,
            decided_by,
        )
        .await?;
    }

    sqlx::query(
        "update expense set status = 'godkjent', decided_by = $3, decided_at = now(),
                            voucher_id = $4, motkonto = $5
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(decided_by)
    .bind(posted.id)
    .bind(contra_account)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApprovedExpense {
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
        warning,
    })
}

/// Avvisning is one-way and requires a begrunnelse — a rejected claim
/// with its note is part of the story, never deleted.
pub async fn reject_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    note: &str,
    decided_by: &str,
) -> Result<()> {
    ensure!(!note.trim().is_empty(), "avvisning krever begrunnelse");
    let updated = sqlx::query(
        "update expense set status = 'avvist', decided_by = $3, decided_at = now(),
                            avvist_note = $4
         where id = $1 and company_id = $2 and status = 'innsendt'",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(decided_by)
    .bind(note)
    .execute(pool)
    .await?;
    if updated.rows_affected() != 1 {
        bail!("kravet finnes ikke eller er allerede avgjort");
    }
    Ok(())
}

#[derive(Debug)]
pub struct PaidExpense {
    pub voucher_number: i64,
    pub fiscal_year: i32,
}

/// Marks the reimbursement paid: debet mellomregning (the konto the
/// approval credited), kredit bank — one transaction with the one-way
/// status flip. When remittering (#33) lands, the betalingsliste will
/// drive this step instead of a manual click.
pub async fn pay_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    date: NaiveDate,
    bank_account: &str,
    paid_by: &str,
) -> Result<PaidExpense> {
    let mut tx = pool.begin().await?;
    let expense = sqlx::query(
        "select status, belop_ore, motkonto, beskrivelse,
                coalesce((select p.name from person p where p.id = expense.person_id), '') as navn
         from expense where id = $1 and company_id = $2 for update",
    )
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such expense")?;
    let status: String = expense.get("status");
    ensure!(
        status == "godkjent",
        "bare godkjente krav kan utbetales (kravet er {status})"
    );
    let amount: i64 = expense.get("belop_ore");
    let contra_account: String = expense
        .get::<Option<String>, _>("motkonto")
        .unwrap_or_default();
    let name: String = expense.get("navn");

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("Utbetaling utlegg {name}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: contra_account,
                amount: Ore(amount),
                vat_code: None,
                description: Some(expense.get("beskrivelse")),
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: bank_account.to_string(),
                amount: Ore(-amount),
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
    let posted = post_voucher_in(&mut tx, company_id, &draft, paid_by).await?;
    sqlx::query(
        "update expense set status = 'utbetalt', utbetalt_voucher_id = $3, utbetalt_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(posted.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PaidExpense {
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
    })
}
