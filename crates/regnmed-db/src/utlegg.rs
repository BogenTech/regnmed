//! Utlegg og kjøregodtgjørelse (docs/utlegg.md, #42): claims with
//! immutable content and one-way decisions (the innboks discipline),
//! kjøregodtgjørelse computed from the dated satsregister AT
//! SUBMISSION (the row is evidence), approval posting kostnad mot
//! mellomregning with the receipt attached to the voucher in ONE
//! transaction, and utbetaling posting mellomregning mot bank.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::hash::sha256;
use regnmed_core::mva::{rate_on, split_gross};
use regnmed_core::sats::sats_on;
use regnmed_core::utlegg::kjoregodtgjorelse;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::attachment::add_attachment_in;
use crate::ledger::post_voucher_in;
use crate::mva::load_vat_rates;
use crate::sats::load_satser;

/// Registers an utlegg with its receipt. The receipt is immutable from
/// this moment (SHA-256 stored, trigger + grants) and follows the
/// claim onto the voucher at approval — oppbevaringsplikten covers the
/// original documentation, not a copy of our making.
#[allow(clippy::too_many_arguments)]
pub async fn create_utlegg(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    dato: NaiveDate,
    beskrivelse: &str,
    belop_ore: i64,
    filename: &str,
    content_type: &str,
    content: &[u8],
    created_by: &str,
) -> Result<Uuid> {
    ensure!(belop_ore > 0, "beløpet må være positivt");
    ensure!(!content.is_empty(), "kvitteringen er tom");
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
    .bind(dato)
    .bind(beskrivelse)
    .bind(belop_ore)
    .bind(filename)
    .bind(content_type)
    .bind(content)
    .bind(sha256(content).to_vec())
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Registers one kjøring. The satser valid on `dato` are resolved from
/// the satsregister NOW and stored on the row — a claim outside the
/// register's coverage is refused loudly, never guessed.
pub async fn create_kjoring(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    dato: NaiveDate,
    beskrivelse: &str,
    km: i64,
    created_by: &str,
) -> Result<(Uuid, i64, i64)> {
    ensure!(km > 0, "km må være positivt");
    let satser = load_satser(pool).await?;
    let sats = sats_on(&satser, "km_godtgjorelse", dato)
        .with_context(|| format!("ingen km-sats for {dato} i satsregisteret"))?;
    let trekkfri_sats = sats_on(&satser, "km_godtgjorelse_trekkfri", dato)
        .with_context(|| format!("ingen trekkfri km-sats for {dato} i satsregisteret"))?;
    let beregnet = kjoregodtgjorelse(km, sats, trekkfri_sats);
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, kind, dato, beskrivelse, belop_ore,
                              km, sats_ore_per_km, trekkfri_ore, trekkpliktig_ore, created_by)
         values ($1, $2, $3, 'kjoring', $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(dato)
    .bind(beskrivelse)
    .bind(beregnet.belop_ore)
    .bind(km)
    .bind(sats)
    .bind(beregnet.trekkfri_ore)
    .bind(beregnet.trekkpliktig_ore)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok((id, beregnet.belop_ore, beregnet.trekkpliktig_ore))
}

#[derive(Debug)]
pub struct ExpenseRow {
    pub id: Uuid,
    pub person_name: String,
    pub own: bool,
    pub kind: String,
    pub dato: NaiveDate,
    pub beskrivelse: String,
    pub belop_ore: i64,
    pub km: Option<i64>,
    pub sats_ore_per_km: Option<i64>,
    pub trekkpliktig_ore: Option<i64>,
    pub receipt_filename: Option<String>,
    pub status: String,
    pub avvist_note: Option<String>,
    pub voucher: Option<String>,
    pub utbetalt_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_expenses(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
) -> Result<Vec<ExpenseRow>> {
    let rows = sqlx::query(
        "select e.id, coalesce(p.name, p.oidc_sub) as person_name, e.person_id = $2 as own,
                e.kind, e.dato, e.beskrivelse, e.belop_ore, e.km, e.sats_ore_per_km,
                e.trekkpliktig_ore, e.receipt_filename, e.status, e.avvist_note,
                e.utbetalt_at,
                v.fiscal_year, v.voucher_number
         from expense e
         join person p on p.id = e.person_id
         left join voucher v on v.id = e.voucher_id
         where e.company_id = $1
         order by e.created_at desc
         limit 200",
    )
    .bind(company_id)
    .bind(viewer)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ExpenseRow {
            id: r.get("id"),
            person_name: r.get("person_name"),
            own: r.get("own"),
            kind: r.get("kind"),
            dato: r.get("dato"),
            beskrivelse: r.get("beskrivelse"),
            belop_ore: r.get("belop_ore"),
            km: r.get("km"),
            sats_ore_per_km: r.get("sats_ore_per_km"),
            trekkpliktig_ore: r.get("trekkpliktig_ore"),
            receipt_filename: r.get("receipt_filename"),
            status: r.get("status"),
            avvist_note: r.get("avvist_note"),
            voucher: match (
                r.get::<Option<i32>, _>("fiscal_year"),
                r.get::<Option<i64>, _>("voucher_number"),
            ) {
                (Some(year), Some(no)) => Some(format!("{year}-{no}")),
                _ => None,
            },
            utbetalt_at: r.get("utbetalt_at"),
        })
        .collect())
}

/// The receipt, integrity-checked against the stored SHA-256 on every
/// read — a claim whose documentation cannot be verified is an error,
/// never silently served.
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
    let content: Vec<u8> = row.get("receipt_content");
    let stored: Vec<u8> = row.get("receipt_sha256");
    ensure!(
        sha256(&content).to_vec() == stored,
        "receipt content fails integrity check"
    );
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
    konto: &str,
    mva_kode: Option<&str>,
    mva_konto: &str,
    motkonto: &str,
    decided_by: &str,
) -> Result<ApprovedExpense> {
    let mut tx = pool.begin().await?;
    let expense = sqlx::query(
        "select kind, dato, beskrivelse, belop_ore, km, sats_ore_per_km, trekkpliktig_ore,
                receipt_filename, receipt_content_type, receipt_content, status,
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
    let kind: String = expense.get("kind");
    let dato: NaiveDate = expense.get("dato");
    let belop: i64 = expense.get("belop_ore");
    let beskrivelse: String = expense.get("beskrivelse");
    let navn: String = expense.get("navn");

    let mut entries = Vec::new();
    let mut warning = None;
    match kind.as_str() {
        "utlegg" => {
            let (netto, mva) = match mva_kode {
                Some(code) => {
                    let rate_class: String =
                        sqlx::query_scalar("select rate_class from vat_code where code = $1")
                            .bind(code)
                            .fetch_optional(pool)
                            .await?
                            .with_context(|| format!("unknown vat code {code}"))?;
                    let rates = load_vat_rates(pool).await?;
                    let rate_bp = rate_on(&rates, &rate_class, dato)
                        .with_context(|| format!("no rate for {dato}"))?;
                    split_gross(belop, rate_bp)
                }
                None => (belop, 0),
            };
            entries.push(EntryDraft {
                account_number: konto.to_string(),
                amount: Ore(netto),
                vat_code: mva_kode.map(str::to_string),
                description: Some(beskrivelse.clone()),
                party_no: None,
                avdeling: None,
                prosjekt: None,
            });
            if mva != 0 {
                entries.push(EntryDraft {
                    account_number: mva_konto.to_string(),
                    amount: Ore(mva),
                    vat_code: None,
                    description: Some("Inngående mva".into()),
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                });
            }
        }
        _ => {
            ensure!(mva_kode.is_none(), "kjøregodtgjørelse har ingen mva");
            let km: i64 = expense.get::<Option<i64>, _>("km").unwrap_or(0);
            let sats: i64 = expense.get::<Option<i64>, _>("sats_ore_per_km").unwrap_or(0);
            entries.push(EntryDraft {
                account_number: konto.to_string(),
                amount: Ore(belop),
                vat_code: None,
                description: Some(format!(
                    "{beskrivelse} — {km} km à {},{:02} kr",
                    sats / 100,
                    sats % 100
                )),
                party_no: None,
                avdeling: None,
                prosjekt: None,
            });
            let trekkpliktig: i64 = expense
                .get::<Option<i64>, _>("trekkpliktig_ore")
                .unwrap_or(0);
            if trekkpliktig > 0 {
                warning = Some(format!(
                    "trekkpliktig del {},{:02} kr skal lønnsinnberettes — a-melding er ikke støttet ennå (#46)",
                    trekkpliktig / 100,
                    trekkpliktig % 100
                ));
            }
        }
    }
    entries.push(EntryDraft {
        account_number: motkonto.to_string(),
        amount: Ore(-belop),
        vat_code: None,
        description: Some(format!("Skyldig {navn}")),
        party_no: None,
        avdeling: None,
        prosjekt: None,
    });

    let label = if kind == "utlegg" {
        "Utlegg"
    } else {
        "Kjøregodtgjørelse"
    };
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("{label} {navn}: {beskrivelse}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, decided_by).await?;

    if kind == "utlegg" {
        let content: Vec<u8> = expense.get("receipt_content");
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
    .bind(motkonto)
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
    dato: NaiveDate,
    bank_konto: &str,
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
    ensure!(status == "godkjent", "bare godkjente krav kan utbetales (kravet er {status})");
    let belop: i64 = expense.get("belop_ore");
    let motkonto: String = expense.get::<Option<String>, _>("motkonto").unwrap_or_default();
    let navn: String = expense.get("navn");

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("Utbetaling utlegg {navn}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: motkonto,
                amount: Ore(belop),
                vat_code: None,
                description: Some(expense.get("beskrivelse")),
                party_no: None,
                avdeling: None,
                prosjekt: None,
            },
            EntryDraft {
                account_number: bank_konto.to_string(),
                amount: Ore(-belop),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
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
