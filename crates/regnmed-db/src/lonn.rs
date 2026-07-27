//! Lønnskjøring (docs/lonn.md, #46 første del).
//!
//! En kjøring er ETT bilag, postert i samme transaksjon som kjøringen og
//! linjene lagres. Linjene finnes for lønnsslippen og for a-meldingen
//! senere; tallene i hovedboken er bilaget, ikke linjene.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::lonn::{self, Lonnsberegning, Lonnsgrunnlag, Lonnssum, Sone, Trekk};
use regnmed_core::sats::SatsPeriode;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

/// NS 4102-kontoer for lønn. Overstyrbare per kjøring.
pub const KONTO_LONN: &str = "5000";
pub const KONTO_FERIEPENGER_KOSTNAD: &str = "5090";
pub const KONTO_AGA_KOSTNAD: &str = "5400";
pub const KONTO_FORSKUDDSTREKK: &str = "2600";
pub const KONTO_SKYLDIG_FERIEPENGER: &str = "2940";
pub const KONTO_SKYLDIG_AGA: &str = "2770";
pub const KONTO_SKYLDIG_LONN: &str = "2930";

#[derive(Debug, Clone)]
pub struct NyAnsatt {
    pub fodselsnummer: String,
    pub navn: String,
    pub stilling: Option<String>,
    pub ansatt_fra: NaiveDate,
    pub manedslonn_ore: Option<i64>,
    pub timelonn_ore: Option<i64>,
    pub trekk_type: String,
    pub trekk_prosent_bp: Option<i32>,
    pub trekk_tabell: Option<i32>,
    pub feriepenger_bp: i32,
    pub bank_account: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Ansatt {
    pub id: Uuid,
    pub navn: String,
    pub stilling: Option<String>,
    /// Ferieloven §4-5 har ingen parallell her, men personvernhensynet er
    /// det samme som i aksjeeierboken: listen bærer fødselsdato, ikke
    /// fødselsnummer. Nummeret hentes bare når a-meldingen skal bygges.
    pub fodselsdato: Option<NaiveDate>,
    pub ansatt_fra: NaiveDate,
    pub ansatt_til: Option<NaiveDate>,
    pub manedslonn_ore: Option<i64>,
    pub timelonn_ore: Option<i64>,
    pub trekk_type: String,
    pub trekk_prosent_bp: Option<i32>,
    pub trekk_tabell: Option<i32>,
    pub feriepenger_bp: i32,
    pub bank_account: Option<String>,
    pub note: Option<String>,
}

impl Ansatt {
    fn trekk(&self) -> Trekk {
        match self.trekk_type.as_str() {
            "prosent" => Trekk::Prosent(self.trekk_prosent_bp.unwrap_or(0) as i64),
            "tabell" => Trekk::Tabell(self.trekk_tabell.unwrap_or(0)),
            _ => Trekk::Ingen,
        }
    }

    fn ansatt_i(&self, maned_slutt: NaiveDate, maned_start: NaiveDate) -> bool {
        self.ansatt_fra <= maned_slutt && self.ansatt_til.is_none_or(|t| t >= maned_start)
    }
}

pub async fn create_ansatt(
    pool: &PgPool,
    company_id: Uuid,
    ny: &NyAnsatt,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(
        regnmed_core::fnr::is_valid(&ny.fodselsnummer),
        "ugyldig fødselsnummer (kontrollsifrene stemmer ikke)"
    );
    ensure!(
        matches!(ny.trekk_type.as_str(), "prosent" | "tabell" | "ingen"),
        "ukjent trekktype «{}»",
        ny.trekk_type
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
    .bind(&ny.fodselsnummer)
    .bind(&ny.navn)
    .bind(&ny.stilling)
    .bind(ny.ansatt_fra)
    .bind(ny.manedslonn_ore)
    .bind(ny.timelonn_ore)
    .bind(&ny.trekk_type)
    .bind(ny.trekk_prosent_bp)
    .bind(ny.trekk_tabell)
    .bind(ny.feriepenger_bp)
    .bind(&ny.bank_account)
    .bind(&ny.note)
    .bind(created_by)
    .execute(pool)
    .await
    .context("den ansatte finnes allerede i dette registeret")?;
    Ok(id)
}

pub async fn list_ansatte(pool: &PgPool, company_id: Uuid) -> Result<Vec<Ansatt>> {
    let rows = sqlx::query(
        "select id, fodselsnummer, navn, stilling, ansatt_fra, ansatt_til,
                manedslonn_ore, timelonn_ore, trekk_type, trekk_prosent_bp,
                trekk_tabell, feriepenger_bp, bank_account, note
         from employee where company_id = $1 order by navn",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(ansatt_fra_rad).collect())
}

fn ansatt_fra_rad(r: &sqlx::postgres::PgRow) -> Ansatt {
    Ansatt {
        id: r.get("id"),
        navn: r.get("navn"),
        stilling: r.get("stilling"),
        fodselsdato: r
            .get::<String, _>("fodselsnummer")
            .as_str()
            .pipe_fodselsdato(),
        ansatt_fra: r.get("ansatt_fra"),
        ansatt_til: r.get("ansatt_til"),
        manedslonn_ore: r.get("manedslonn_ore"),
        timelonn_ore: r.get("timelonn_ore"),
        trekk_type: r.get("trekk_type"),
        trekk_prosent_bp: r.get("trekk_prosent_bp"),
        trekk_tabell: r.get("trekk_tabell"),
        feriepenger_bp: r.get("feriepenger_bp"),
        bank_account: r.get("bank_account"),
        note: r.get("note"),
    }
}

trait Fodselsdato {
    fn pipe_fodselsdato(&self) -> Option<NaiveDate>;
}
impl Fodselsdato for &str {
    fn pipe_fodselsdato(&self) -> Option<NaiveDate> {
        regnmed_core::fnr::fodselsdato(self)
    }
}

/// What one employee gets this month, decided by the caller.
#[derive(Debug, Clone)]
pub struct Lonnspost {
    pub employee_id: Uuid,
    /// Ordinary pay. None uses the employee's månedslønn, unless
    /// `fra_timer` asks for the timesheet instead.
    pub brutto_ore: Option<i64>,
    /// Feriepenger paid out this month (drawn from the liability).
    pub feriepenger_ore: i64,
    /// Compute pay from hours logged this month × the employee's
    /// timelønn. Requires the timesheet month to be locked.
    pub fra_timer: bool,
}

#[derive(Debug, Clone)]
pub struct Lonnskjoring {
    pub id: Uuid,
    pub ar: i32,
    pub maned: u32,
    pub utbetalt_dato: NaiveDate,
    pub sone: String,
    pub sum: Lonnssum,
    pub voucher_id: Uuid,
    pub linjer: Vec<(Uuid, String, Lonnsberegning, i64)>,
    /// (employee_id, navn) for the run's lines — enough for the portal
    /// to offer a payslip per person without asking again.
    pub ansatte: Vec<(Uuid, String)>,
}

async fn satser(pool: &PgPool) -> Result<Vec<SatsPeriode>> {
    let rows = sqlx::query("select domene, valid_from, verdi from sats")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| SatsPeriode {
            domene: r.get("domene"),
            valid_from: r.get("valid_from"),
            verdi: r.get("verdi"),
        })
        .collect())
}

/// Runs payroll for one month and posts it as one voucher, in one
/// transaction.
///
/// **Arbeidsgiveravgift is computed on what is actually paid out** —
/// ordinary pay plus any feriepenger paid this month — because that is
/// when the avgift falls due. Accruing aga on feriepenger not yet paid
/// is ordinary practice but needs a matching drawdown when they are;
/// a half-built accrual is worse than none, so v1 leaves it out and
/// says so (docs/lonn.md).
#[allow(clippy::too_many_arguments)]
pub async fn kjor_lonn(
    pool: &PgPool,
    company_id: Uuid,
    ar: i32,
    maned: u32,
    utbetalt_dato: NaiveDate,
    sone_slug: &str,
    poster: &[Lonnspost],
    created_by: &str,
) -> Result<Lonnskjoring> {
    ensure!((1..=12).contains(&maned), "måned må være 1-12");
    ensure!(!poster.is_empty(), "lønnskjøringen har ingen ansatte");
    let sone = Sone::fra_slug(sone_slug)
        .with_context(|| format!("ukjent arbeidsgiveravgiftssone «{sone_slug}»"))?;

    // Sone Ia avvises FØR satsoppslaget. Ellers ville feilmeldingen bli
    // «satsen mangler — legg den inn», som er stikk motsatt av riktig
    // råd: å legge inn 10,6 % som en flat sats er nettopp feilen
    // fribeløpsregelen finnes for å hindre.
    lonn::arbeidsgiveravgift(0, sone, 0).map_err(|e| anyhow::anyhow!("{e}"))?;

    let alle_satser = satser(pool).await?;
    let aga_domene = sone.sats_domene();
    let aga_bp =
        regnmed_core::sats::sats_on(&alle_satser, &aga_domene, utbetalt_dato).ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                lonn::LonnError::ManglerSats(aga_domene.clone(), utbetalt_dato)
            )
        })?;

    let ansatte = list_ansatte(pool, company_id).await?;
    let maned_start = NaiveDate::from_ymd_opt(ar, maned, 1).context("ugyldig måned")?;
    let maned_slutt = maned_start
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
        .context("ugyldig måned")?;

    let mut sum = Lonnssum::default();
    let mut linjer = Vec::new();

    for post in poster {
        let ansatt = ansatte
            .iter()
            .find(|a| a.id == post.employee_id)
            .with_context(|| format!("ukjent ansatt {}", post.employee_id))?;
        ensure!(
            ansatt.ansatt_i(maned_slutt, maned_start),
            "{} var ikke ansatt i {maned}/{ar}",
            ansatt.navn
        );
        let brutto = if post.fra_timer {
            let grunnlag = timegrunnlag(pool, company_id, ansatt.id, ar, maned).await?;
            // Timene må være LÅST før de betales. En lønnskjøring er
            // innsettings-bar; endres timene etterpå, spriker de to for
            // alltid uten noen måte å avstemme dem på.
            ensure!(
                grunnlag.laast,
                "timelisten for {maned:02}/{ar} er ikke låst — lås måneden før timelønn \
                 utbetales, ellers kan timene endres etter at lønnen er bokført ({})",
                ansatt.navn
            );
            ensure!(
                grunnlag.minutter > 0,
                "{} har ingen førte timer i {maned:02}/{ar}",
                ansatt.navn
            );
            grunnlag.belop_ore
        } else {
            post.brutto_ore.or(ansatt.manedslonn_ore).with_context(|| {
                format!("{} har verken månedslønn eller oppgitt beløp", ansatt.navn)
            })?
        };
        ensure!(brutto >= 0, "brutto kan ikke være negativ");

        let beregning = lonn::beregn(
            &Lonnsgrunnlag {
                brutto_ore: brutto,
                feriepenger_ore: post.feriepenger_ore,
                trekk: ansatt.trekk(),
            },
            maned,
        )
        .map_err(|e| anyhow::anyhow!("{}: {e}", ansatt.navn))?;

        // Feriepengegrunnlaget er det som utbetales som lønn; feriepenger
        // opptjener ikke nye feriepenger.
        let avsetning = lonn::feriepengeavsetning(brutto, ansatt.feriepenger_bp as i64);
        // Avgiften faller på det som faktisk utbetales.
        let aga = lonn::arbeidsgiveravgift(brutto + post.feriepenger_ore, sone, aga_bp)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        sum.brutto_ore += beregning.brutto_ore;
        sum.feriepenger_utbetalt_ore += beregning.feriepenger_ore;
        sum.forskuddstrekk_ore += beregning.forskuddstrekk_ore;
        sum.netto_ore += beregning.netto_ore;
        sum.feriepengeavsetning_ore += avsetning;
        sum.aga_ore += aga;

        linjer.push((ansatt.id, ansatt.navn.clone(), beregning, avsetning));
    }

    let mut entries = vec![
        EntryDraft {
            account_number: KONTO_LONN.into(),
            amount: Ore(sum.brutto_ore),
            vat_code: None,
            description: Some(format!("Lønn {maned:02}/{ar}")),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        },
        EntryDraft {
            account_number: KONTO_FORSKUDDSTREKK.into(),
            amount: Ore(-sum.forskuddstrekk_ore),
            vat_code: None,
            description: Some("Forskuddstrekk".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        },
        EntryDraft {
            account_number: KONTO_SKYLDIG_LONN.into(),
            amount: Ore(-sum.netto_ore),
            vat_code: None,
            description: Some("Netto til utbetaling".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        },
    ];

    // Utbetalte feriepenger er IKKE en ny kostnad — de trekker ned
    // gjelden som ble avsatt i opptjeningsåret.
    if sum.feriepenger_utbetalt_ore != 0 {
        entries.push(EntryDraft {
            account_number: KONTO_SKYLDIG_FERIEPENGER.into(),
            amount: Ore(sum.feriepenger_utbetalt_ore),
            vat_code: None,
            description: Some("Utbetalte feriepenger".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    if sum.feriepengeavsetning_ore != 0 {
        entries.push(EntryDraft {
            account_number: KONTO_FERIEPENGER_KOSTNAD.into(),
            amount: Ore(sum.feriepengeavsetning_ore),
            vat_code: None,
            description: Some("Feriepengeavsetning".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
        entries.push(EntryDraft {
            account_number: KONTO_SKYLDIG_FERIEPENGER.into(),
            amount: Ore(-sum.feriepengeavsetning_ore),
            vat_code: None,
            description: Some("Avsatte feriepenger".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    if sum.aga_ore != 0 {
        entries.push(EntryDraft {
            account_number: KONTO_AGA_KOSTNAD.into(),
            amount: Ore(sum.aga_ore),
            vat_code: None,
            description: Some(format!("Arbeidsgiveravgift sone {sone_slug}")),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
        entries.push(EntryDraft {
            account_number: KONTO_SKYLDIG_AGA.into(),
            amount: Ore(-sum.aga_ore),
            vat_code: None,
            description: Some("Skyldig arbeidsgiveravgift".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: utbetalt_dato,
        description: format!("Lønnskjøring {maned:02}/{ar}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;

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
    .bind(ar)
    .bind(maned as i32)
    .bind(utbetalt_dato)
    .bind(sone_slug)
    .bind(sum.brutto_ore)
    .bind(sum.feriepenger_utbetalt_ore)
    .bind(sum.forskuddstrekk_ore)
    .bind(sum.netto_ore)
    .bind(sum.feriepengeavsetning_ore)
    .bind(sum.aga_ore)
    .bind(sum.aga_feriepenger_ore)
    .bind(posted.id)
    .bind(created_by)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if e.to_string()
            .contains("payroll_run_company_id_ar_maned_key")
        {
            anyhow::anyhow!("lønn for {maned:02}/{ar} er allerede kjørt")
        } else {
            anyhow::anyhow!("{e}")
        }
    })?;

    for (employee_id, _, b, avsetning) in &linjer {
        sqlx::query(
            "insert into payroll_line
                 (id, run_id, employee_id, brutto_ore, feriepenger_ore,
                  trekkgrunnlag_ore, forskuddstrekk_ore, netto_ore,
                  feriepengeavsetning_ore, aga_ore, aga_feriepenger_ore, halv_trekk)
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(employee_id)
        .bind(b.brutto_ore)
        .bind(b.feriepenger_ore)
        .bind(b.trekkgrunnlag_ore)
        .bind(b.forskuddstrekk_ore)
        .bind(b.netto_ore)
        .bind(avsetning)
        .bind(lonn::arbeidsgiveravgift(b.brutto_ore + b.feriepenger_ore, sone, aga_bp).unwrap_or(0))
        .bind(0i64)
        .bind(b.halv_trekk)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Lonnskjoring {
        id: run_id,
        ar,
        maned,
        utbetalt_dato,
        sone: sone_slug.to_string(),
        sum,
        voucher_id: posted.id,
        ansatte: linjer
            .iter()
            .map(|(id, navn, _, _)| (*id, navn.clone()))
            .collect(),
        linjer,
    })
}

pub async fn list_kjoringer(
    pool: &PgPool,
    company_id: Uuid,
    ar: Option<i32>,
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
    .bind(ar)
    .fetch_all(pool)
    .await?;
    let mut out = rows
        .iter()
        .map(|r| Lonnskjoring {
            id: r.get("id"),
            ar: r.get("ar"),
            maned: r.get::<i32, _>("maned") as u32,
            utbetalt_dato: r.get("utbetalt_dato"),
            sone: r.get("sone"),
            sum: Lonnssum {
                brutto_ore: r.get("brutto_ore"),
                feriepenger_utbetalt_ore: r.get("feriepenger_utbetalt_ore"),
                forskuddstrekk_ore: r.get("forskuddstrekk_ore"),
                netto_ore: r.get("netto_ore"),
                feriepengeavsetning_ore: r.get("feriepengeavsetning_ore"),
                aga_ore: r.get("aga_ore"),
                aga_feriepenger_ore: r.get("aga_feriepenger_ore"),
            },
            voucher_id: r.get("voucher_id"),
            linjer: Vec::new(),
            ansatte: Vec::new(),
        })
        .collect::<Vec<_>>();

    // Hvem som var med i hver kjøring, så portalen kan tilby lønnsslipp
    // per person uten et nytt kall.
    for kjoring in &mut out {
        let linjer = sqlx::query(
            "select l.employee_id, e.navn from payroll_line l
             join employee e on e.id = l.employee_id
             where l.run_id = $1 order by e.navn",
        )
        .bind(kjoring.id)
        .fetch_all(pool)
        .await?;
        kjoring.ansatte = linjer
            .iter()
            .map(|r| (r.get("employee_id"), r.get("navn")))
            .collect();
    }
    Ok(out)
}

/// Guard used by the API before offering a run: which months already ran.
pub async fn kjort_maned(pool: &PgPool, company_id: Uuid, ar: i32, maned: u32) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from payroll_run where company_id = $1 and ar = $2 and maned = $3",
    )
    .bind(company_id)
    .bind(ar)
    .bind(maned as i32)
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
pub async fn lonnsslipp(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
) -> Result<regnmed_core::lonnsslipp::LonnsslippInput> {
    use regnmed_core::lonnsslipp::{LonnsslippInput, Slipplinje};

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

    let ar: i32 = row.get("ar");
    let brutto: i64 = row.get("brutto_ore");
    let feriepenger: i64 = row.get("feriepenger_ore");

    // Hittil i år, til og med denne kjøringen.
    let hittil = sqlx::query(
        "select coalesce(sum(l.brutto_ore + l.feriepenger_ore), 0)::bigint as brutto,
                coalesce(sum(l.forskuddstrekk_ore), 0)::bigint as trekk,
                coalesce(sum(l.feriepengeavsetning_ore), 0)::bigint as feriepenger
         from payroll_line l
         join payroll_run r on r.id = l.run_id
         where r.company_id = $1 and r.ar = $2 and l.employee_id = $3
           and r.maned <= (select maned from payroll_run where id = $4)",
    )
    .bind(company_id)
    .bind(ar)
    .bind(employee_id)
    .bind(run_id)
    .fetch_one(pool)
    .await?;

    let mut linjer = vec![Slipplinje {
        tekst: "Fastlønn".into(),
        belop_ore: brutto,
    }];
    if feriepenger != 0 {
        linjer.push(Slipplinje {
            tekst: "Feriepenger".into(),
            belop_ore: feriepenger,
        });
    }

    let trekk_type: String = row.get("trekk_type");
    Ok(LonnsslippInput {
        arbeidsgiver_navn: row.get("selskap"),
        arbeidsgiver_orgnr: row.get("orgnr"),
        arbeidsgiver_adresse: row.get("address"),
        ansatt_navn: row.get("navn"),
        ansatt_stilling: row.get("stilling"),
        // Fødselsdato, ikke fødselsnummer — også på et dokument som
        // sendes til den ansatte selv.
        ansatt_fodselsdato: row
            .get::<String, _>("fodselsnummer")
            .as_str()
            .pipe_fodselsdato(),
        ar,
        maned: row.get::<i32, _>("maned") as u32,
        utbetalt_dato: row.get("utbetalt_dato"),
        linjer,
        brutto_ore: brutto + feriepenger,
        trekkgrunnlag_ore: row.get("trekkgrunnlag_ore"),
        forskuddstrekk_ore: row.get("forskuddstrekk_ore"),
        trekk_prosent_bp: (trekk_type == "prosent")
            .then(|| row.get::<Option<i32>, _>("trekk_prosent_bp").map(i64::from))
            .flatten(),
        halv_trekk: row.get("halv_trekk"),
        netto_ore: row.get("netto_ore"),
        feriepengeavsetning_ore: row.get("feriepengeavsetning_ore"),
        feriepenger_bp: row.get::<i32, _>("feriepenger_bp") as i64,
        hittil_brutto_ore: hittil.get("brutto"),
        hittil_trekk_ore: hittil.get("trekk"),
        hittil_feriepenger_ore: hittil.get("feriepenger"),
    })
}

/// Hours logged by an employee in one month, and what they come to.
#[derive(Debug, Clone, Copy)]
pub struct Timegrunnlag {
    pub minutter: i64,
    pub timesats_ore: i64,
    pub belop_ore: i64,
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
    ar: i32,
    maned: u32,
) -> Result<Timegrunnlag> {
    let start = NaiveDate::from_ymd_opt(ar, maned, 1).context("ugyldig måned")?;
    let slutt = start
        .checked_add_months(chrono::Months::new(1))
        .and_then(|d| d.pred_opt())
        .context("ugyldig måned")?;

    let ansatt = sqlx::query(
        "select person_id, timelonn_ore, navn from employee where id = $1 and company_id = $2",
    )
    .bind(employee_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such ansatt")?;
    let person_id: Option<Uuid> = ansatt.get("person_id");
    let navn: String = ansatt.get("navn");
    let person_id = person_id.with_context(|| {
        format!("{navn} er ikke koblet til en portalbruker — timeføringen vet ikke hvem det er")
    })?;
    let timesats: i64 = ansatt
        .get::<Option<i64>, _>("timelonn_ore")
        .with_context(|| format!("{navn} har ingen timelønn"))?;

    let minutter: Option<i64> = sqlx::query_scalar(
        "select sum(minutter)::bigint from time_entry
         where company_id = $1 and person_id = $2 and dato between $3 and $4",
    )
    .bind(company_id)
    .bind(person_id)
    .bind(start)
    .bind(slutt)
    .fetch_one(pool)
    .await?;
    let minutter = minutter.unwrap_or(0);

    let laast = crate::timesheet::timesheet_lock(pool, company_id)
        .await?
        .is_some_and(|through| through >= slutt);

    Ok(Timegrunnlag {
        minutter,
        timesats_ore: timesats,
        belop_ore: regnmed_core::lonn::timelonn(minutter, timesats),
        laast,
    })
}
