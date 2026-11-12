//! Lønnskjøring (docs/lonn.md, #46 første del).
//!
//! A run is ONE bilag, posted in the same transaction that stores the run
//! and its lines. The lines exist for the payslip and for the a-melding
//! later; the numbers in the hovedbok are the bilag, not the lines.

use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, NaiveDate};
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

/// The arbeidsforhold facts the a-melding requires (F290/F291,
/// migration 0073). Every field is optional in the register; the
/// a-melding refuses a missing one by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmploymentDetails {
    /// The underenhet (virksomhet) the employee works at.
    pub business_unit_orgnumber: Option<String>,
    pub occupation_code: Option<String>,
    pub position_percent_bp: Option<i32>,
    pub full_time_hours_per_week_hundredths: Option<i32>,
    pub working_hours_scheme: Option<String>,
    pub employment_form: Option<String>,
    pub last_salary_change: Option<NaiveDate>,
}

/// Checks what the DB constraints cannot say in words, and what they
/// cannot check at all: MOD11, the half-yearly code lists, and that the
/// virksomhet is an underenhet rather than the company itself.
fn validate_details(d: &EmploymentDetails, company_orgnumber: &str) -> Result<()> {
    use regnmed_core::amelding::{EMPLOYMENT_FORMS, WORKING_HOURS_SCHEMES};
    if let Some(n) = &d.business_unit_orgnumber {
        ensure!(
            regnmed_core::orgnumber::is_valid(n),
            "virksomhetens organisasjonsnummer «{n}» er ugyldig"
        );
        // shortcut: not checked against Enhetsregisteret that it is an
        // underenhet OF this company; do it when the BRREG lookup is wired
        // into the employee form.
        ensure!(
            n != company_orgnumber,
            "virksomheten skal være underenheten i Enhetsregisteret der den ansatte jobber,              ikke selskapets eget organisasjonsnummer (a-meldingen, F290)"
        );
    }
    if let Some(code) = &d.occupation_code {
        // shortcut: format only; the full STYRK-08 catalogue is not vendored.
        ensure!(
            code.len() == 7 && code.bytes().all(|b| b.is_ascii_digit()),
            "yrkeskoden skal være 7 siffer fra SSBs yrkeskatalog (STYRK-08)"
        );
    }
    if let Some(bp) = d.position_percent_bp {
        ensure!(
            (0..=10000).contains(&bp),
            "stillingsprosenten må være mellom 0 og 100"
        );
    }
    if let Some(h) = d.full_time_hours_per_week_hundredths {
        ensure!(
            (1..=16800).contains(&h),
            "timer per uke for full stilling må være mellom 0 og 168"
        );
    }
    if let Some(code) = &d.working_hours_scheme {
        ensure!(
            WORKING_HOURS_SCHEMES.contains(&code.as_str()),
            "ukjent arbeidstidsordning «{code}»"
        );
    }
    if let Some(code) = &d.employment_form {
        ensure!(
            EMPLOYMENT_FORMS.contains(&code.as_str()),
            "ukjent ansettelsesform «{code}»"
        );
    }
    Ok(())
}

async fn company_orgnumber(pool: &PgPool, company_id: Uuid) -> Result<String> {
    sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such company")
}

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
    pub details: EmploymentDetails,
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
    pub details: EmploymentDetails,
}

impl Employee {
    /// What the a-melding would refuse for this employee, by the same
    /// names its errors use. Empty means the arbeidsforhold is complete.
    pub fn amelding_missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.details.business_unit_orgnumber.is_none() {
            missing.push("virksomhet");
        }
        missing.extend(self.employment().missing());
        missing
    }

    /// The arbeidsforhold as the a-melding renderer takes it. The id is
    /// the employee id: stable across months (F108).
    fn employment(&self) -> regnmed_core::amelding::Employment {
        let d = &self.details;
        regnmed_core::amelding::Employment {
            id: self.id.to_string(),
            start_date: self.employed_from,
            end_date: self.employed_to,
            full_time_hours_per_week_hundredths: d
                .full_time_hours_per_week_hundredths
                .map(i64::from),
            occupation_code: d.occupation_code.clone(),
            working_hours_scheme: d.working_hours_scheme.clone(),
            position_percent_bp: d.position_percent_bp.map(i64::from),
            last_salary_change: d.last_salary_change,
            employment_form: d.employment_form.clone(),
        }
    }

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
    validate_details(&new.details, &company_orgnumber(pool, company_id).await?)?;
    let d = &new.details;
    let id = Uuid::new_v4();
    sqlx::query(
        "insert into employee
             (id, company_id, fodselsnummer, navn, stilling, ansatt_fra,
              manedslonn_ore, timelonn_ore, trekk_type, trekk_prosent_bp,
              trekk_tabell, feriepenger_bp, bank_account, note, created_by,
              business_unit_orgnumber, occupation_code, position_percent_bp,
              full_time_hours_per_week_hundredths, working_hours_scheme,
              employment_form, last_salary_change)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,
                 $16,$17,$18,$19,$20,$21,$22)",
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
    .bind(&d.business_unit_orgnumber)
    .bind(&d.occupation_code)
    .bind(d.position_percent_bp)
    .bind(d.full_time_hours_per_week_hundredths)
    .bind(&d.working_hours_scheme)
    .bind(&d.employment_form)
    .bind(d.last_salary_change)
    .execute(pool)
    .await
    .context("den ansatte finnes allerede i dette registeret")?;
    Ok(id)
}

/// Replaces the employee's arbeidsforhold details (all seven, so a field
/// can be cleared). Payroll lines already run are untouched: the
/// a-melding for a past month reads what is true when it is built.
pub async fn update_employment_details(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
    d: &EmploymentDetails,
) -> Result<()> {
    validate_details(d, &company_orgnumber(pool, company_id).await?)?;
    let updated = sqlx::query(
        "update employee set
             business_unit_orgnumber = $3, occupation_code = $4,
             position_percent_bp = $5, full_time_hours_per_week_hundredths = $6,
             working_hours_scheme = $7, employment_form = $8, last_salary_change = $9
         where id = $1 and company_id = $2",
    )
    .bind(employee_id)
    .bind(company_id)
    .bind(&d.business_unit_orgnumber)
    .bind(&d.occupation_code)
    .bind(d.position_percent_bp)
    .bind(d.full_time_hours_per_week_hundredths)
    .bind(&d.working_hours_scheme)
    .bind(&d.employment_form)
    .bind(d.last_salary_change)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "ukjent ansatt");
    Ok(())
}

pub async fn list_ansatte(pool: &PgPool, company_id: Uuid) -> Result<Vec<Employee>> {
    let rows = sqlx::query(
        "select e.id, e.fodselsnummer, e.navn, e.stilling, e.ansatt_fra, e.ansatt_til,
                e.manedslonn_ore, e.timelonn_ore, e.trekk_type, e.trekk_prosent_bp,
                e.trekk_tabell, e.feriepenger_bp, e.bank_account, e.note,
                e.business_unit_orgnumber, e.occupation_code, e.position_percent_bp,
                e.full_time_hours_per_week_hundredths, e.working_hours_scheme,
                e.employment_form, e.last_salary_change,
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
        details: EmploymentDetails {
            business_unit_orgnumber: r.get("business_unit_orgnumber"),
            occupation_code: r.get("occupation_code"),
            position_percent_bp: r.get("position_percent_bp"),
            full_time_hours_per_week_hundredths: r.get("full_time_hours_per_week_hundredths"),
            working_hours_scheme: r.get("working_hours_scheme"),
            employment_form: r.get("employment_form"),
            last_salary_change: r.get("last_salary_change"),
        },
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
pub struct PayrollRun {
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
    /// `fixed` or `hourly` (migration 0073) — the a-melding reports them
    /// under different descriptions. None when the run could not tell.
    pub pay_kind: Option<&'static str>,
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
) -> Result<PayrollRun> {
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
        // Decided here, where it is known, and stored: inferring it later
        // from which wage the employee happens to have would be a guess.
        let pay_kind = if post.from_timesheet {
            Some("hourly")
        } else if post.gross_ore.is_none() {
            Some("fixed") // the employee's månedslønn
        } else {
            match (employee.monthly_salary_ore, employee.hourly_wage_ore) {
                (Some(_), None) => Some("fixed"),
                (None, Some(_)) => Some("hourly"),
                _ => None,
            }
        };
        let gross = if post.from_timesheet {
            let basis = hour_basis(pool, company_id, employee.id, year, month).await?;
            // The hours must be LOCKED before they are paid. A payroll
            // run is insert-only; if the hours change afterwards the two
            // diverge forever with no way to reconcile them.
            ensure!(
                basis.locked,
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
            // Pay is taxed in the year it is paid, so December's pay paid
            // in January takes January's tables -- like the aga rate.
            paid_date.year(),
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
        let owed_after = hist.skyldig_ore + avsetning - post.holiday_pay_ore;
        let employer_contribution_holiday_pay =
            payroll::employer_contribution_accrual_target(owed_after, aga_bp) - hist.avsatt_aga_ore;

        sum.holiday_pay_accrual_ore += avsetning;
        sum.employer_contribution_ore += aga;
        sum.employer_contribution_holiday_pay_ore += employer_contribution_holiday_pay;

        lines.push(Lonnslinje {
            employee_id: employee.id,
            name: employee.name.clone(),
            calculation,
            holiday_pay_accrual_ore: avsetning,
            employer_contribution_holiday_pay_ore: employer_contribution_holiday_pay,
            pay_kind,
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

    // The ledger would refuse the first missing account on its own, in
    // English and one at a time; the user needs every missing one, in
    // Norwegian, with where to fix it.
    let mut wanted: Vec<&str> = draft
        .entries
        .iter()
        .map(|e| e.account_number.as_str())
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let active: Vec<String> = sqlx::query_scalar(
        "select number from account where company_id = $1 and active and number = any($2)",
    )
    .bind(company_id)
    .bind(&wanted)
    .fetch_all(pool)
    .await?;
    let missing: Vec<&str> = wanted
        .into_iter()
        .filter(|n| !active.iter().any(|a| a == n))
        .collect();
    ensure!(
        missing.is_empty(),
        "Lønnskjøringen bokfører på konto {}, som selskapet ikke har som aktiv konto. \
         Legg {} til under Hovedbok → Kontoplan og kjør lønnen på nytt.",
        missing.join(", "),
        if missing.len() == 1 { "den" } else { "dem" }
    );

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
                  feriepengeavsetning_ore, aga_ore, aga_feriepenger_ore, halv_trekk,
                  pay_kind)
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
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
        .bind(l.pay_kind)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(PayrollRun {
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
    let from_payroll: i64 = feriepengehistorikk(pool, company_id)
        .await?
        .values()
        .map(|h| h.skyldig_ore)
        .sum();
    if i_hovedboken == from_payroll {
        return Ok(None);
    }
    Ok(Some(format!(
        "Konto {ACCOUNT_HOLIDAY_PAY_OWED} viser {} kr skyldige feriepenger, men \
         lønnshistorikken forklarer {} kr. Differansen på {} kr har ingen ansatt å \
         knyttes til, så det er ikke beregnet arbeidsgiveravgift på den — \
         avsetningen på konto {ACCOUNT_ACCRUED_EMPLOYER_CONTRIBUTION_HOLIDAY_PAY} må i så fall føres manuelt.",
        kroner(i_hovedboken),
        kroner(from_payroll),
        kroner(i_hovedboken - from_payroll),
    )))
}

fn kroner(ore: i64) -> String {
    format!("{},{:02}", ore / 100, (ore % 100).abs())
}

pub async fn list_kjoringer(
    pool: &PgPool,
    company_id: Uuid,
    year: Option<i32>,
) -> Result<Vec<PayrollRun>> {
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
        .map(|r| PayrollRun {
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
pub struct HourBasis {
    pub minutes: i64,
    pub hourly_rate_ore: i64,
    pub amount_ore: i64,
    /// The timesheet is locked through the end of this month.
    pub locked: bool,
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
pub async fn hour_basis(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
    year: i32,
    month: u32,
) -> Result<HourBasis> {
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

    let locked = crate::timesheet::timesheet_lock(pool, company_id)
        .await?
        .is_some_and(|through| through >= end);

    Ok(HourBasis {
        minutes,
        hourly_rate_ore: hourly_rate,
        amount_ore: regnmed_core::payroll::hourly_pay(minutes, hourly_rate),
        locked,
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

/// One stored payroll run made ready for the a-melding (docs/lonn.md).
///
/// `hindrances` are what stops the melding from being delivered, in
/// words a payroll person can act on — ALL of them at once, not the
/// first one the renderer trips over. `melding` is built regardless, so
/// the caller can still render and show the renderer's own verdict once
/// the list is empty.
pub struct AmeldingDraft {
    pub melding: regnmed_core::amelding::Melding,
    pub hindrances: Vec<String>,
}

/// Builds the a-melding for a stored run. The fødselsnummer is read HERE
/// and nowhere else in the payroll module, as for RF-1086.
pub async fn amelding_for_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    delivered_at: chrono::DateTime<chrono::Utc>,
) -> Result<AmeldingDraft> {
    use regnmed_core::amelding::{Income, IncomeKind, Melding, Recipient};

    let run = sqlx::query(
        "select ar, maned, utbetalt_dato, sone, aga_ore from payroll_run
         where id = $1 and company_id = $2",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent lønnskjøring")?;
    let (year, month): (i32, i32) = (run.get("ar"), run.get("maned"));
    let paid_date: NaiveDate = run.get("utbetalt_dato");
    let zone_slug: String = run.get("sone");
    let zone = Zone::from_slug(&zone_slug)
        .with_context(|| format!("ukjent arbeidsgiveravgiftssone «{zone_slug}»"))?;

    let mut hindrances = Vec::new();
    let rate_bp =
        regnmed_core::rates::value_on(&rates(pool).await?, &zone.rate_domain(), paid_date)
            .unwrap_or_else(|| {
                hindrances.push(format!(
                    "arbeidsgiveravgiftssatsen for sone {zone_slug} på {paid_date} finnes ikke i \
                 satsregisteret"
                ));
                0
            });

    let employees = list_ansatte(pool, company_id).await?;
    let nins: std::collections::HashMap<Uuid, String> =
        sqlx::query("select id, fodselsnummer from employee where company_id = $1")
            .bind(company_id)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| (r.get("id"), r.get("fodselsnummer")))
            .collect();
    let lines = sqlx::query(
        "select employee_id, brutto_ore, feriepenger_ore, forskuddstrekk_ore, pay_kind
         from payroll_line where run_id = $1",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    let mut recipients = Vec::new();
    let mut business_units = std::collections::BTreeSet::new();
    for line in &lines {
        let employee_id: Uuid = line.get("employee_id");
        let employee = employees
            .iter()
            .find(|e| e.id == employee_id)
            .context("lønnslinjen peker på en ansatt som ikke finnes")?;
        let missing = employee.amelding_missing();
        if !missing.is_empty() {
            hindrances.push(format!(
                "{}: arbeidsforholdet mangler {}",
                employee.name,
                missing.join(", ")
            ));
        }
        business_units.extend(employee.details.business_unit_orgnumber.clone());

        let gross: i64 = line.get("brutto_ore");
        let mut income = Vec::new();
        if gross != 0 {
            match line.get::<Option<String>, _>("pay_kind").as_deref() {
                Some("fixed") => income.push(Income {
                    kind: IncomeKind::FixedSalary,
                    amount_ore: gross,
                }),
                Some(_) => income.push(Income {
                    kind: IncomeKind::HourlyWage,
                    amount_ore: gross,
                }),
                // Not stored (a run from before migration 0073, or one
                // that could not tell): never guessed now.
                None => hindrances.push(format!(
                    "{}: det ble ikke lagret om lønnen i denne kjøringen var fastlønn eller \
                     timelønn, og regnmed gjetter det ikke i ettertid",
                    employee.name
                )),
            }
        }
        let holiday_pay: i64 = line.get("feriepenger_ore");
        if holiday_pay != 0 {
            income.push(Income {
                kind: IncomeKind::HolidayPay,
                amount_ore: holiday_pay,
            });
        }
        recipients.push(Recipient {
            name: employee.name.clone(),
            nin: nins.get(&employee_id).cloned().unwrap_or_default(),
            employment: employee.employment(),
            income,
            withholding_ore: line.get("forskuddstrekk_ore"),
        });
    }

    // shortcut: one virksomhet per melding, as the renderer takes it;
    // group per virksomhet (and split the avgift) when a customer has
    // employees on two underenheter.
    if business_units.len() > 1 {
        hindrances.push(format!(
            "de ansatte i kjøringen står på ulike virksomheter ({}) — regnmed lager foreløpig \
             bare a-melding for én virksomhet om gangen",
            business_units
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // F108: an active arbeidsforhold is reported every month until its
    // end date, paid or not. A run only has lines for those who were
    // paid, and a melding without the others is wrong at NAV.
    let month_start = NaiveDate::from_ymd_opt(year, month as u32, 1).context("ugyldig måned")?;
    let month_end = month_start
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
        .context("ugyldig måned")?;
    for e in &employees {
        let paid = lines
            .iter()
            .any(|l| l.get::<Uuid, _>("employee_id") == e.id);
        if !paid && e.employed_in(month_end, month_start) {
            hindrances.push(format!(
                "{} var ansatt i {month:02}/{year}, men har ingen linje i kjøringen — \
                 a-meldingen skal rapportere arbeidsforholdet hver måned også uten lønn (F108), \
                 og det er ikke bygget ennå",
                e.name
            ));
        }
    }

    Ok(AmeldingDraft {
        melding: Melding {
            message_id: run_id.to_string(),
            replaces_message_id: None,
            delivered_at,
            source_system: "regnmed".into(),
            orgnumber: company_orgnumber(pool, company_id).await?,
            business_orgnumber: business_units.into_iter().next(),
            payment_date: paid_date,
            zone,
            employer_contribution_rate_bp: rate_bp,
            employer_contribution_ore: run.get("aga_ore"),
            recipients,
        },
        hindrances,
    })
}
