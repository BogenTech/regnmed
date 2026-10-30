//! Lønnskjøring (docs/lonn.md, #46 første del).
//!
//! A run is ONE bilag, posted in the same transaction that stores the run
//! and its lines. The lines exist for the payslip and for the a-melding
//! later; the numbers in the hovedbok are the bilag, not the lines.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::payroll::{
    self, PayrollBasis, PayrollCalculation, PayrollTotals, Withholding, Zone,
};
use regnmed_core::rates::RatePeriod;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

/// NS 4102 accounts for payroll. Overridable per run.
pub const ACCOUNT_SALARY: &str = "5000";
pub const ACCOUNT_HOLIDAY_PAY_COST: &str = "5090";
pub const ACCOUNT_EMPLOYER_CONTRIBUTION_COST: &str = "5400";
pub const ACCOUNT_WITHHOLDING: &str = "2600";
pub const ACCOUNT_HOLIDAY_PAY_OWED: &str = "2940";
pub const ACCOUNT_EMPLOYER_CONTRIBUTION_PAYABLE: &str = "2770";
pub const ACCOUNT_SALARY_PAYABLE: &str = "2930";
/// Aga on feriepenger that are accrued but not paid out. The account
/// numbers are chosen so the SAF-T grouping lands correctly: the code
/// list names 5400 and 2770, and the nearest-neighbour lookup puts 5405
/// on 5400 (Arbeidsgiveravgift) and 2780 on 2770 (Skyldig
/// arbeidsgiveravgift). 2785 would have landed on 2790 "Andre offentlige
/// avgifter" instead.
pub const ACCOUNT_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY_COST: &str = "5405";
pub const ACCOUNT_ACCRUED_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY: &str = "2780";

#[derive(Debug, Clone)]
pub struct NewEmployee {
    pub nin: String,
    pub name: String,
    pub stilling: Option<String>,
    pub employed_from: NaiveDate,
    pub monthly_salary_ore: Option<i64>,
    pub hourly_wage_ore: Option<i64>,
    pub withholding_type: String,
    pub withholding_percent_bp: Option<i32>,
    pub withholding_table: Option<i32>,
    pub holiday_pay_bp: i32,
    pub bank_account: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Employee {
    pub id: Uuid,
    pub name: String,
    pub stilling: Option<String>,
    /// Ferieloven §4-5 has no parallel here, but the privacy concern is
    /// the same as in the aksjeeierbok: the list carries the birth date,
    /// not the fødselsnummer. The number is fetched only when the
    /// a-melding is to be built.
    pub birth_date: Option<NaiveDate>,
    pub employed_from: NaiveDate,
    pub employed_to: Option<NaiveDate>,
    pub monthly_salary_ore: Option<i64>,
    pub hourly_wage_ore: Option<i64>,
    pub withholding_type: String,
    pub withholding_percent_bp: Option<i32>,
    pub withholding_table: Option<i32>,
    pub holiday_pay_bp: i32,
    pub bank_account: Option<String>,
    pub note: Option<String>,
    /// The linked portal user (migration 0036/0050), if any — who may
    /// see this employee's payslip and whose hours become timelønn.
    pub person_id: Option<Uuid>,
    pub person_name: Option<String>,
    pub person_email: Option<String>,
}

impl Employee {
    fn withholding(&self) -> Withholding {
        match self.withholding_type.as_str() {
            "prosent" => Withholding::Percent(self.withholding_percent_bp.unwrap_or(0) as i64),
            "tabell" => Withholding::Table(self.withholding_table.unwrap_or(0)),
            _ => Withholding::None,
        }
    }

    fn employed_in(&self, month_end: NaiveDate, maned_start: NaiveDate) -> bool {
        self.employed_from <= month_end && self.employed_to.is_none_or(|t| t >= maned_start)
    }
}

pub async fn create_employee(
    pool: &PgPool,
    company_id: Uuid,
    new: &NewEmployee,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(
        regnmed_core::nin::is_valid(&new.nin),
        "ugyldig fødselsnummer (kontrollsifrene stemmer ikke)"
    );
    ensure!(
        matches!(
            new.withholding_type.as_str(),
            "prosent" | "tabell" | "ingen"
        ),
        "ukjent trekktype «{}»",
        new.withholding_type
    );
    let id = Uuid::new_v4();
    sqlx::query(
        "insert into employee
             (id, company_id, fodselsnummer, navn, stilling, ansatt_fra,
              manedslonn_ore, timelonn_ore, trekk_type, trekk_prosent_bp,
              trekk_tabell, feriepenger_bp, bank_account, note, created_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&new.nin)
    .bind(&new.name)
    .bind(&new.stilling)
    .bind(new.employed_from)
    .bind(new.monthly_salary_ore)
    .bind(new.hourly_wage_ore)
    .bind(&new.withholding_type)
    .bind(new.withholding_percent_bp)
    .bind(new.withholding_table)
    .bind(new.holiday_pay_bp)
    .bind(&new.bank_account)
    .bind(&new.note)
    .bind(created_by)
    .execute(pool)
    .await
    .context("den ansatte finnes allerede i dette registeret")?;
    Ok(id)
}

pub async fn list_ansatte(pool: &PgPool, company_id: Uuid) -> Result<Vec<Employee>> {
    let rows = sqlx::query(
        "select e.id, e.fodselsnummer, e.navn, e.stilling, e.ansatt_fra, e.ansatt_til,
                e.manedslonn_ore, e.timelonn_ore, e.trekk_type, e.trekk_prosent_bp,
                e.trekk_tabell, e.feriepenger_bp, e.bank_account, e.note,
                e.person_id,
                coalesce(p.name, p.oidc_sub) as person_navn,
                p.email as person_epost
         from employee e
         left join person p on p.id = e.person_id
         where e.company_id = $1 order by e.navn",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(employee_from_row).collect())
}

fn employee_from_row(r: &sqlx::postgres::PgRow) -> Employee {
    Employee {
        id: r.get("id"),
        name: r.get("navn"),
        stilling: r.get("stilling"),
        birth_date: r
            .get::<String, _>("fodselsnummer")
            .as_str()
            .pipe_birth_date(),
        employed_from: r.get("ansatt_fra"),
        employed_to: r.get("ansatt_til"),
        monthly_salary_ore: r.get("manedslonn_ore"),
        hourly_wage_ore: r.get("timelonn_ore"),
        withholding_type: r.get("trekk_type"),
        withholding_percent_bp: r.get("trekk_prosent_bp"),
        withholding_table: r.get("trekk_tabell"),
        holiday_pay_bp: r.get("feriepenger_bp"),
        bank_account: r.get("bank_account"),
        note: r.get("note"),
        person_id: r.get("person_id"),
        person_name: r.get("person_navn"),
        person_email: r.get("person_epost"),
    }
}

trait BirthDate {
    fn pipe_birth_date(&self) -> Option<NaiveDate>;
}
impl BirthDate for &str {
    fn pipe_birth_date(&self) -> Option<NaiveDate> {
        regnmed_core::nin::birth_date(self)
    }
}

/// What one employee gets this month, decided by the caller.
#[derive(Debug, Clone)]
pub struct PayrollEntry {
    pub employee_id: Uuid,
    /// Ordinary pay. None uses the employee's månedslønn, unless
    /// `from_timesheet` asks for the timesheet instead.
    pub gross_ore: Option<i64>,
    /// Feriepenger paid out this month (drawn from the liability).
    pub holiday_pay_ore: i64,
    /// Compute pay from hours logged this month × the employee's
    /// timelønn. Requires the timesheet month to be locked.
    pub from_timesheet: bool,
}

#[derive(Debug, Clone)]
pub struct Lonnskjoring {
    pub id: Uuid,
    pub year: i32,
    pub month: u32,
    pub paid_date: NaiveDate,
    pub zone: String,
    pub sum: PayrollTotals,
    pub voucher_id: Uuid,
    pub lines: Vec<Lonnslinje>,
    /// (employee_id, navn) for the run's lines — enough for the portal
    /// to offer a payslip per person without asking again.
    pub employees: Vec<(Uuid, String)>,
    /// Things worth saying out loud about the run. Never fatal — a
    /// payroll run that balances is still a valid payroll run.
    pub warnings: Vec<String>,
}

/// One employee's line in a run.
#[derive(Debug, Clone)]
pub struct Lonnslinje {
    pub employee_id: Uuid,
    pub name: String,
    pub calculation: PayrollCalculation,
    pub holiday_pay_accrual_ore: i64,
    /// This run's change to the accrued aga on the employee's unpaid
    /// feriepenger — positive when new feriepenger were earned, negative
    /// when they were paid out.
    pub employer_contribution_holiday_pay_ore: i64,
}

/// Feriepenger owed and aga already accrued for one employee, derived
/// from the payroll history. Neither is stored: both are folds over
/// insert-only lines, so they cannot drift from what was booked.
#[derive(Debug, Clone, Copy, Default)]
struct Feriepengehistorikk {
    skyldig_ore: i64,
    avsatt_aga_ore: i64,
}

async fn feriepengehistorikk(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, Feriepengehistorikk>> {
    let rows = sqlx::query(
        "select l.employee_id,
                coalesce(sum(l.feriepengeavsetning_ore - l.feriepenger_ore), 0)::bigint as skyldig,
                coalesce(sum(l.aga_feriepenger_ore), 0)::bigint as avsatt
         from payroll_line l
         join payroll_run r on r.id = l.run_id
         where r.company_id = $1
         group by l.employee_id",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get("employee_id"),
                Feriepengehistorikk {
                    skyldig_ore: r.get("skyldig"),
                    avsatt_aga_ore: r.get("avsatt"),
                },
            )
        })
        .collect())
}

async fn account_balance(pool: &PgPool, company_id: Uuid, account: &str) -> Result<i64> {
    let balance: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company_id)
    .bind(account)
    .fetch_one(pool)
    .await?;
    Ok(balance)
}

async fn rates(pool: &PgPool) -> Result<Vec<RatePeriod>> {
    let rows = sqlx::query("select domene, valid_from, verdi from sats")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| RatePeriod {
            domain: r.get("domene"),
            valid_from: r.get("valid_from"),
            value: r.get("verdi"),
        })
        .collect())
}

/// Runs payroll for one month and posts it as one voucher, in one
/// transaction.
///
/// **Arbeidsgiveravgift is computed on what is actually paid out** —
/// ordinary pay plus any feriepenger paid this month — because that is
/// when the avgift falls due.
///
/// On top of that, the run trues up the **accrual** on feriepenger that
/// are owed but not yet paid: the obligation arises with the earning
/// even though the payment is next year. Per employee, `skyldige
/// feriepenger` and `already accrued` are folded out of the insert-only
/// payroll lines, and the run books the difference needed to reach
/// `sats × skyldig`. Because it targets a balance rather than adding
/// increments, a payout, a rate change or a first run on an existing
/// liability all settle themselves rather than leaving a residue.
#[allow(clippy::too_many_arguments)]
pub async fn run_payroll(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    month: u32,
    paid_date: NaiveDate,
    zone_slug: &str,
    poster: &[PayrollEntry],
    created_by: &str,
) -> Result<Lonnskjoring> {
    ensure!((1..=12).contains(&month), "måned må være 1-12");
    ensure!(!poster.is_empty(), "lønnskjøringen har ingen ansatte");
    let zone = Zone::from_slug(zone_slug)
        .with_context(|| format!("ukjent arbeidsgiveravgiftssone «{zone_slug}»"))?;

    // Sone Ia is refused BEFORE the rate lookup. Otherwise the error
    // would read "the rate is missing — add it", which is the exact
    // opposite of the right advice: entering 10,6 % as a flat rate is
    // precisely the mistake the fribeløp rule exists to prevent.
    payroll::employer_contribution(0, zone, 0).map_err(|e| anyhow::anyhow!("{e}"))?;

    let alle_satser = rates(pool).await?;
    let aga_domene = zone.rate_domain();
    let aga_bp =
        regnmed_core::rates::value_on(&alle_satser, &aga_domene, paid_date).ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                payroll::PayrollError::MissingRate(aga_domene.clone(), paid_date)
            )
        })?;

    let employees = list_ansatte(pool, company_id).await?;
    let maned_start = NaiveDate::from_ymd_opt(year, month, 1).context("ugyldig måned")?;
    let month_end = maned_start
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
        .context("ugyldig måned")?;

    let history = feriepengehistorikk(pool, company_id).await?;

    let mut sum = PayrollTotals::default();
    let mut lines = Vec::new();

    for post in poster {
        let employee = employees
            .iter()
            .find(|a| a.id == post.employee_id)
            .with_context(|| format!("ukjent ansatt {}", post.employee_id))?;
        ensure!(
            employee.employed_in(month_end, maned_start),
            "{} var ikke ansatt i {month}/{year}",
            employee.name
        );
        let gross = if post.from_timesheet {
            let basis = timegrunnlag(pool, company_id, employee.id, year, month).await?;
            // The hours must be LOCKED before they are paid. A payroll
            // run is insert-only; if the hours change afterwards the two
            // diverge forever with no way to reconcile them.
            ensure!(
                basis.laast,
                "timelisten for {month:02}/{year} er ikke låst — lås måneden før timelønn \
                 utbetales, ellers kan timene endres etter at lønnen er bokført ({})",
                employee.name
            );
            ensure!(
                basis.minutes > 0,
                "{} har ingen førte timer i {month:02}/{year}",
                employee.name
            );
            basis.amount_ore
        } else {
            post.gross_ore
                .or(employee.monthly_salary_ore)
                .with_context(|| {
                    format!(
                        "{} har verken månedslønn eller oppgitt beløp",
                        employee.name
                    )
                })?
        };
        ensure!(gross >= 0, "brutto kan ikke være negativ");

        let calculation = payroll::calculate(
            &PayrollBasis {
                gross_ore: gross,
                holiday_pay_ore: post.holiday_pay_ore,
                withholding: employee.withholding(),
            },
            month,
        )
        .map_err(|e| anyhow::anyhow!("{}: {e}", employee.name))?;

        // The feriepenger base is what is paid out as pay; feriepenger
        // do not earn new feriepenger.
        let avsetning = payroll::holiday_pay_accrual(gross, employee.holiday_pay_bp as i64);
        // The avgift falls on what is actually paid out.
        let aga = payroll::employer_contribution(gross + post.holiday_pay_ore, zone, aga_bp)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        sum.gross_ore += calculation.gross_ore;
        sum.holiday_pay_paid_ore += calculation.holiday_pay_ore;
        sum.withholding_ore += calculation.withholding_ore;
        sum.net_ore += calculation.net_ore;
        // The accrual on unpaid feriepenger is a TARGET, not an
        // increment: after the run, the employee's accrued aga should be
        // the rate times what they actually have coming. The difference
        // is what the run books — which is also why a feriepenger
        // liability earned before this function existed is caught up on
        // the first run instead of standing uncovered.
        let hist = history.get(&employee.id).copied().unwrap_or_default();
        let skyldig_etter = hist.skyldig_ore + avsetning - post.holiday_pay_ore;
        let employer_contribution_holiday_pay =
            payroll::employer_contribution_accrual_target(skyldig_etter, aga_bp)
                - hist.avsatt_aga_ore;

        sum.holiday_pay_accrual_ore += avsetning;
        sum.employer_contribution_ore += aga;
        sum.employer_contribution_holiday_pay_ore += employer_contribution_holiday_pay;

        lines.push(Lonnslinje {
            employee_id: employee.id,
            name: employee.name.clone(),
            calculation,
            holiday_pay_accrual_ore: avsetning,
            employer_contribution_holiday_pay_ore: employer_contribution_holiday_pay,
        });
    }

    // Each line is omitted when it is zero. A month with only holiday
    // taken — feriepenger paid out, no ordinary pay — has neither a pay
    // cost nor withholding, and a zero line is rejected by bilag
    // validation. Without this, that month could not be run at all.
    let mut entries = Vec::new();
    if sum.gross_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_SALARY.into(),
            amount: Ore(sum.gross_ore),
            vat_code: None,
            description: Some(format!("Lønn {month:02}/{year}")),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if sum.withholding_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_WITHHOLDING.into(),
            amount: Ore(-sum.withholding_ore),
            vat_code: None,
            description: Some("Forskuddstrekk".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if sum.net_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_SALARY_PAYABLE.into(),
            amount: Ore(-sum.net_ore),
            vat_code: None,
            description: Some("Netto til utbetaling".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }

    // Feriepenger paid out are NOT a new cost — they draw down the
    // liability accrued in the year they were earned.
    if sum.holiday_pay_paid_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_HOLIDAY_PAY_OWED.into(),
            amount: Ore(sum.holiday_pay_paid_ore),
            vat_code: None,
            description: Some("Utbetalte feriepenger".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if sum.holiday_pay_accrual_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_HOLIDAY_PAY_COST.into(),
            amount: Ore(sum.holiday_pay_accrual_ore),
            vat_code: None,
            description: Some("Feriepengeavsetning".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
        entries.push(EntryDraft {
            account_number: ACCOUNT_HOLIDAY_PAY_OWED.into(),
            amount: Ore(-sum.holiday_pay_accrual_ore),
            vat_code: None,
            description: Some("Avsatte feriepenger".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if sum.employer_contribution_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_EMPLOYER_CONTRIBUTION_COST.into(),
            amount: Ore(sum.employer_contribution_ore),
            vat_code: None,
            description: Some(format!("Arbeidsgiveravgift sone {zone_slug}")),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
        entries.push(EntryDraft {
            account_number: ACCOUNT_EMPLOYER_CONTRIBUTION_PAYABLE.into(),
            amount: Ore(-sum.employer_contribution_ore),
            vat_code: None,
            description: Some("Skyldig arbeidsgiveravgift".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }

    // The avgift on feriepenger not yet paid out. The amount is a
    // difference and can go either way: if feriepenger were paid out this
    // month, the accrual is reversed — the cost was taken in the year
    // they were earned, and the avgift on what was paid out is already in
    // the aga line above.
    if sum.employer_contribution_holiday_pay_ore != 0 {
        entries.push(EntryDraft {
            account_number: ACCOUNT_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY_COST.into(),
            amount: Ore(sum.employer_contribution_holiday_pay_ore),
            vat_code: None,
            description: Some("Arbeidsgiveravgift av påløpne feriepenger".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
        entries.push(EntryDraft {
            account_number: ACCOUNT_ACCRUED_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY.into(),
            amount: Ore(-sum.employer_contribution_holiday_pay_ore),
            vat_code: None,
            description: Some("Påløpt arbeidsgiveravgift av feriepenger".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: paid_date,
        description: format!("Lønnskjøring {month:02}/{year}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;

    // The accounts for the feriepenger avgift are new in this function,
    // so a company that has run payroll before does not have them. They
    // are created on demand — letting the run fail on "unknown account"
    // would have been a regression for every company already using
    // payroll.
    if sum.employer_contribution_holiday_pay_ore != 0 {
        crate::ledger::ensure_account(
            pool,
            company_id,
            ACCOUNT_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY_COST,
            "Arbeidsgiveravgift av påløpne feriepenger",
        )
        .await?;
        crate::ledger::ensure_account(
            pool,
            company_id,
            ACCOUNT_ACCRUED_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY,
            "Påløpt arbeidsgiveravgift av feriepenger",
        )
        .await?;
    }

    let mut tx = pool.begin().await?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;

    let run_id = Uuid::new_v4();
    sqlx::query(
        "insert into payroll_run
             (id, company_id, ar, maned, utbetalt_dato, sone, brutto_ore,
              feriepenger_utbetalt_ore, forskuddstrekk_ore, netto_ore,
              feriepengeavsetning_ore, aga_ore, aga_feriepenger_ore, voucher_id, created_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(year)
    .bind(month as i32)
    .bind(paid_date)
    .bind(zone_slug)
    .bind(sum.gross_ore)
    .bind(sum.holiday_pay_paid_ore)
    .bind(sum.withholding_ore)
    .bind(sum.net_ore)
    .bind(sum.holiday_pay_accrual_ore)
    .bind(sum.employer_contribution_ore)
    .bind(sum.employer_contribution_holiday_pay_ore)
    .bind(posted.id)
    .bind(created_by)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if e.to_string()
            .contains("payroll_run_company_id_ar_maned_key")
        {
            anyhow::anyhow!("lønn for {month:02}/{year} er allerede kjørt")
        } else {
            anyhow::anyhow!("{e}")
        }
    })?;

    for l in &lines {
        let b = &l.calculation;
        sqlx::query(
            "insert into payroll_line
                 (id, run_id, employee_id, brutto_ore, feriepenger_ore,
                  trekkgrunnlag_ore, forskuddstrekk_ore, netto_ore,
                  feriepengeavsetning_ore, aga_ore, aga_feriepenger_ore, halv_trekk)
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(l.employee_id)
        .bind(b.gross_ore)
        .bind(b.holiday_pay_ore)
        .bind(b.withholding_basis_ore)
        .bind(b.withholding_ore)
        .bind(b.net_ore)
        .bind(l.holiday_pay_accrual_ore)
        .bind(
            payroll::employer_contribution(b.gross_ore + b.holiday_pay_ore, zone, aga_bp)
                .unwrap_or(0),
        )
        .bind(l.employer_contribution_holiday_pay_ore)
        .bind(b.half_withholding)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Lonnskjoring {
        id: run_id,
        year,
        month,
        paid_date,
        zone: zone_slug.to_string(),
        sum,
        voucher_id: posted.id,
        employees: lines
            .iter()
            .map(|l| (l.employee_id, l.name.clone()))
            .collect(),
        lines,
        warnings: feriepengeavvik(pool, company_id)
            .await?
            .into_iter()
            .collect(),
    })
}

/// Says so when the ledger's feriepengegjeld is larger than what the
/// payroll history accounts for.
///
/// The accrual is derived per employee from payroll lines, so
/// feriepenger that reached 2940 some other way — an opening balance, a
/// SAF-T import, a manual voucher — carry no accrual and never will.
/// That is a real gap, and the honest handling is to name it rather than
/// either ignoring it or inventing an accrual for a liability we cannot
/// attribute to anyone.
async fn feriepengeavvik(pool: &PgPool, company_id: Uuid) -> Result<Option<String>> {
    let i_hovedboken = -account_balance(pool, company_id, ACCOUNT_HOLIDAY_PAY_OWED).await?;
    let fra_lonn: i64 = feriepengehistorikk(pool, company_id)
        .await?
        .values()
        .map(|h| h.skyldig_ore)
        .sum();
    if i_hovedboken == fra_lonn {
        return Ok(None);
    }
    Ok(Some(format!(
        "Konto {ACCOUNT_HOLIDAY_PAY_OWED} viser {} kr skyldige feriepenger, men \
         lønnshistorikken forklarer {} kr. Differansen på {} kr har ingen ansatt å \
         knyttes til, så det er ikke beregnet arbeidsgiveravgift på den — \
         avsetningen på konto {ACCOUNT_ACCRUED_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY} må i så fall føres manuelt.",
        kroner(i_hovedboken),
        kroner(fra_lonn),
        kroner(i_hovedboken - fra_lonn),
    )))
}

fn kroner(ore: i64) -> String {
    format!("{},{:02}", ore / 100, (ore % 100).abs())
}

pub async fn list_kjoringer(
    pool: &PgPool,
    company_id: Uuid,
    year: Option<i32>,
) -> Result<Vec<Lonnskjoring>> {
    let rows = sqlx::query(
        "select id, ar, maned, utbetalt_dato, sone, brutto_ore,
                feriepenger_utbetalt_ore, forskuddstrekk_ore, netto_ore,
                feriepengeavsetning_ore, aga_ore, aga_feriepenger_ore, voucher_id
         from payroll_run
         where company_id = $1 and ($2::int is null or ar = $2)
         order by ar desc, maned desc",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    let mut out = rows
        .iter()
        .map(|r| Lonnskjoring {
            id: r.get("id"),
            year: r.get("ar"),
            month: r.get::<i32, _>("maned") as u32,
            paid_date: r.get("utbetalt_dato"),
            zone: r.get("sone"),
            sum: PayrollTotals {
                gross_ore: r.get("brutto_ore"),
                holiday_pay_paid_ore: r.get("feriepenger_utbetalt_ore"),
                withholding_ore: r.get("forskuddstrekk_ore"),
                net_ore: r.get("netto_ore"),
                holiday_pay_accrual_ore: r.get("feriepengeavsetning_ore"),
                employer_contribution_ore: r.get("aga_ore"),
                employer_contribution_holiday_pay_ore: r.get("aga_feriepenger_ore"),
            },
            voucher_id: r.get("voucher_id"),
            lines: Vec::new(),
            employees: Vec::new(),
            warnings: Vec::new(),
        })
        .collect::<Vec<_>>();

    // Who took part in each run, so the portal can offer a payslip per
    // person without another call.
    for run in &mut out {
        let lines = sqlx::query(
            "select l.employee_id, e.navn from payroll_line l
             join employee e on e.id = l.employee_id
             where l.run_id = $1 order by e.navn",
        )
        .bind(run.id)
        .fetch_all(pool)
        .await?;
        run.employees = lines
            .iter()
            .map(|r| (r.get("employee_id"), r.get("navn")))
            .collect();
    }
    Ok(out)
}

/// Guard used by the API before offering a run: which months already ran.
pub async fn kjort_maned(pool: &PgPool, company_id: Uuid, year: i32, month: u32) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from payroll_run where company_id = $1 and ar = $2 and maned = $3",
    )
    .bind(company_id)
    .bind(year)
    .bind(month as i32)
    .fetch_one(pool)
    .await?;
    if n > 1 {
        bail!("flere kjøringer for samme måned — dette skal være umulig");
    }
    Ok(n == 1)
}

/// Builds the payslip for one employee in one run.
///
/// Rendered on demand rather than stored: the payroll line is
/// insert-only, so the same line yields the same slip forever — and not
/// storing it means one fewer copy of personal data to protect.
pub async fn payslip(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
) -> Result<regnmed_core::payslip::PayslipInput> {
    use regnmed_core::payslip::{PayslipInput, PayslipLine};

    let row = sqlx::query(
        "select r.ar, r.maned, r.utbetalt_dato,
                l.brutto_ore, l.feriepenger_ore, l.trekkgrunnlag_ore,
                l.forskuddstrekk_ore, l.netto_ore, l.feriepengeavsetning_ore,
                l.halv_trekk,
                e.navn, e.stilling, e.fodselsnummer, e.trekk_type,
                e.trekk_prosent_bp, e.feriepenger_bp,
                c.name as selskap, c.orgnr, c.address
         from payroll_line l
         join payroll_run r on r.id = l.run_id
         join employee e on e.id = l.employee_id
         join company c on c.id = r.company_id
         where r.id = $1 and l.employee_id = $2 and r.company_id = $3",
    )
    .bind(run_id)
    .bind(employee_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such lønnslinje")?;

    let year: i32 = row.get("ar");
    let gross: i64 = row.get("brutto_ore");
    let holiday_pay: i64 = row.get("feriepenger_ore");

    // Year to date, through this run.
    let year_to_date = sqlx::query(
        "select coalesce(sum(l.brutto_ore + l.feriepenger_ore), 0)::bigint as brutto,
                coalesce(sum(l.forskuddstrekk_ore), 0)::bigint as trekk,
                coalesce(sum(l.feriepengeavsetning_ore), 0)::bigint as feriepenger
         from payroll_line l
         join payroll_run r on r.id = l.run_id
         where r.company_id = $1 and r.ar = $2 and l.employee_id = $3
           and r.maned <= (select maned from payroll_run where id = $4)",
    )
    .bind(company_id)
    .bind(year)
    .bind(employee_id)
    .bind(run_id)
    .fetch_one(pool)
    .await?;

    let mut lines = vec![PayslipLine {
        text: "Fastlønn".into(),
        amount_ore: gross,
    }];
    if holiday_pay != 0 {
        lines.push(PayslipLine {
            text: "Feriepenger".into(),
            amount_ore: holiday_pay,
        });
    }

    let withholding_type: String = row.get("trekk_type");
    Ok(PayslipInput {
        employer_name: row.get("selskap"),
        employer_orgnumber: row.get("orgnr"),
        employer_address: row.get("address"),
        employee_name: row.get("navn"),
        employee_position: row.get("stilling"),
        // Birth date, not fødselsnummer — even on a document sent to the
        // employee themselves.
        employee_birth_date: row
            .get::<String, _>("fodselsnummer")
            .as_str()
            .pipe_birth_date(),
        year,
        month: row.get::<i32, _>("maned") as u32,
        paid_date: row.get("utbetalt_dato"),
        lines,
        gross_ore: gross + holiday_pay,
        withholding_basis_ore: row.get("trekkgrunnlag_ore"),
        withholding_ore: row.get("forskuddstrekk_ore"),
        withholding_percent_bp: (withholding_type == "prosent")
            .then(|| row.get::<Option<i32>, _>("trekk_prosent_bp").map(i64::from))
            .flatten(),
        half_withholding: row.get("halv_trekk"),
        net_ore: row.get("netto_ore"),
        holiday_pay_accrual_ore: row.get("feriepengeavsetning_ore"),
        holiday_pay_bp: row.get::<i32, _>("feriepenger_bp") as i64,
        year_to_date_gross_ore: year_to_date.get("brutto"),
        year_to_date_withholding_ore: year_to_date.get("trekk"),
        ytd_holiday_pay_ore: year_to_date.get("feriepenger"),
    })
}

/// The portal user an employee record is linked to, if any.
///
/// The link is optional (migration 0036): an employee who never logs in
/// has none, and then nobody can claim to be them.
pub async fn employee_person(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
) -> Result<Option<Uuid>> {
    let p: Option<Option<Uuid>> =
        sqlx::query_scalar("select person_id from employee where id = $1 and company_id = $2")
            .bind(employee_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    Ok(p.flatten())
}

/// Hours logged by an employee in one month, and what they come to.
#[derive(Debug, Clone, Copy)]
pub struct Timegrunnlag {
    pub minutes: i64,
    pub hourly_rate_ore: i64,
    pub amount_ore: i64,
    /// The timesheet is locked through the end of this month.
    pub laast: bool,
}

/// What an employee's logged hours amount to for one month.
///
/// **Every** entry counts, billable or not: the employer owes pay for
/// work done regardless of whether a client is invoiced for it.
///
/// `laast` reports whether the timesheet month is closed. Running
/// payroll from unlocked hours is refused by [`kjor_lonn`] — a payroll
/// run is insert-only, so if hours change afterwards the two disagree
/// forever with no way to reconcile. The månedslås exists for exactly
/// this order of operations (docs/timer.md: lock for lønn, then bill).
pub async fn timegrunnlag(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
    year: i32,
    month: u32,
) -> Result<Timegrunnlag> {
    let start = NaiveDate::from_ymd_opt(year, month, 1).context("ugyldig måned")?;
    let end = start
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
        .context("ugyldig måned")?;

    let employee = sqlx::query(
        "select person_id, timelonn_ore, navn from employee where id = $1 and company_id = $2",
    )
    .bind(employee_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such ansatt")?;
    let person_id: Option<Uuid> = employee.get("person_id");
    let name: String = employee.get("navn");
    let person_id = person_id.with_context(|| {
        format!("{name} er ikke koblet til en portalbruker — timeføringen vet ikke hvem det er")
    })?;
    let hourly_rate: i64 = employee
        .get::<Option<i64>, _>("timelonn_ore")
        .with_context(|| format!("{name} har ingen timelønn"))?;

    let minutes: Option<i64> = sqlx::query_scalar(
        "select sum(minutter)::bigint from time_entry
         where company_id = $1 and person_id = $2 and dato between $3 and $4",
    )
    .bind(company_id)
    .bind(person_id)
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await?;
    let minutes = minutes.unwrap_or(0);

    let laast = crate::timesheet::timesheet_lock(pool, company_id)
        .await?
        .is_some_and(|through| through >= end);

    Ok(Timegrunnlag {
        minutes,
        hourly_rate_ore: hourly_rate,
        amount_ore: regnmed_core::payroll::hourly_pay(minutes, hourly_rate),
        laast,
    })
}

/// Refuses everything that must never be on the other end of the link:
/// a person that does not exist, or a machine identity — an integration
/// with an employee's payslip access would be a leak by construction.
async fn require_linkable_person(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    person_id: Uuid,
) -> Result<()> {
    let kind: Option<String> = sqlx::query_scalar("select kind from person where id = $1")
        .bind(person_id)
        .fetch_optional(&mut **tx)
        .await?;
    match kind.as_deref() {
        None => bail!("ukjent portalbruker"),
        Some("menneske") => Ok(()),
        Some(_) => bail!("en integrasjon kan ikke være lønnsmottaker"),
    }
}

/// Links an employee to a portal user, the manual way (docs/lonn.md).
///
/// The guards are the point: the employee must be UNLINKED (re-linking
/// is a deliberate two-step — unlink first — never a dropdown slip),
/// the person must be a human, and the partial unique index refuses a
/// person who is already another employee in the same company. Every
/// outcome that changes the link leaves an `employee_link_change` row.
pub async fn link_employee_person(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
    person_id: Uuid,
    performed_by: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let eksisterende: Option<Option<Uuid>> = sqlx::query_scalar(
        "select person_id from employee where id = $1 and company_id = $2 for update",
    )
    .bind(employee_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(eksisterende) = eksisterende else {
        bail!("ukjent ansatt");
    };
    if let Some(p) = eksisterende {
        ensure!(
            p == person_id,
            "den ansatte er alt koblet til en annen portalbruker — koble fra først"
        );
        // Idempotent: linking to the person already linked is a no-op,
        // and writes no audit row pretending something changed.
        return Ok(());
    }
    require_linkable_person(&mut tx, person_id).await?;
    let res = sqlx::query("update employee set person_id = $3 where id = $1 and company_id = $2")
        .bind(employee_id)
        .bind(company_id)
        .bind(person_id)
        .execute(&mut *tx)
        .await;
    if let Err(e) = &res {
        if let Some(db) = e.as_database_error() {
            if db.code().as_deref() == Some("23505") {
                bail!("portalbrukeren er alt koblet til en annen ansatt i selskapet");
            }
        }
    }
    res?;
    log_link(
        &mut tx,
        company_id,
        employee_id,
        person_id,
        "koblet",
        "admin",
        Some(performed_by),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Removes the link. The employee record stands; only who-may-log-in-as
/// them is cleared. Audited like the link itself.
pub async fn unlink_employee_person(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
    performed_by: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let eksisterende: Option<Option<Uuid>> = sqlx::query_scalar(
        "select person_id from employee where id = $1 and company_id = $2 for update",
    )
    .bind(employee_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(Some(person_id)) = eksisterende else {
        bail!("den ansatte er ikke koblet til noen portalbruker");
    };
    sqlx::query("update employee set person_id = null where id = $1 and company_id = $2")
        .bind(employee_id)
        .bind(company_id)
        .execute(&mut *tx)
        .await?;
    log_link(
        &mut tx,
        company_id,
        employee_id,
        person_id,
        "frakoblet",
        "admin",
        Some(performed_by),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn log_link(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    employee_id: Uuid,
    person_id: Uuid,
    change: &str,
    source: &str,
    performed_by: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "insert into employee_link_change
             (company_id, employee_id, person_id, endring, kilde, utfort_av)
         values ($1,$2,$3,$4,$5,$6)",
    )
    .bind(company_id)
    .bind(employee_id)
    .bind(person_id)
    .bind(change)
    .bind(source)
    .bind(performed_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Koblingsendring {
    pub change: String,
    pub source: String,
    pub person_name: String,
    pub performed_by: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The trail of who linked whom — the question that matters when a
/// payslip was readable by the wrong person.
pub async fn employee_link_history(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
) -> Result<Vec<Koblingsendring>> {
    let rows = sqlx::query(
        "select c.endring, c.kilde, c.created_at,
                coalesce(p.name, p.oidc_sub) as person_navn,
                coalesce(u.name, u.oidc_sub) as utfort_av
         from employee_link_change c
         join person p on p.id = c.person_id
         left join person u on u.id = c.utfort_av
         where c.company_id = $1 and c.employee_id = $2
         order by c.created_at desc",
    )
    .bind(company_id)
    .bind(employee_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Koblingsendring {
            change: r.get("endring"),
            source: r.get("kilde"),
            person_name: r.get("person_navn"),
            performed_by: r.get("utfort_av"),
            created_at: r.get("created_at"),
        })
        .collect())
}
