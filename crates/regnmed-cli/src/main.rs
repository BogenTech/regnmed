use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "regnmed", about = "regnmed ledger administration", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run pending database migrations
    Migrate,
    /// Re-walk and verify voucher hash chains against the stored hashes
    VerifyLedger {
        /// Company id; verifies every company when omitted
        #[arg(long)]
        company: Option<Uuid>,
    },
    /// Create a demo company, post vouchers, attempt tampering, verify (dev only)
    Demo,
    /// Export Norwegian SAF-T Financial v1.30 XML for a company
    SaftExport {
        /// Company id (or use --orgnr)
        #[arg(long, conflicts_with = "orgnr")]
        company: Option<Uuid>,
        /// Organization number of the company to export
        #[arg(long)]
        orgnr: Option<String>,
        /// Fiscal year to export (whole calendar year)
        #[arg(long, conflicts_with_all = ["from", "to"])]
        year: Option<i32>,
        /// Start date (YYYY-MM-DD); requires --to
        #[arg(long, requires = "to")]
        from: Option<chrono::NaiveDate>,
        /// End date (YYYY-MM-DD); requires --from
        #[arg(long, requires = "from")]
        to: Option<chrono::NaiveDate>,
        /// Contact person, "Fornavn Etternavn" — the Norwegian SAF-T header
        /// requires one
        #[arg(long)]
        contact: String,
        /// Output file; "-" writes to stdout. Defaults to Skatteetaten's
        /// naming convention: "SAF-T Financial_<orgnr>_<timestamp>.xml"
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Mva-spesifikasjon: grunnlag og beregnet avgift per standardkode
    MvaReport {
        /// Company id (or use --orgnr)
        #[arg(long, conflicts_with = "orgnr")]
        company: Option<Uuid>,
        /// Organization number of the company
        #[arg(long)]
        orgnr: Option<String>,
        /// Year to report
        #[arg(long)]
        year: i32,
        /// Termin 1-6 (two-month period); whole year when omitted
        #[arg(long)]
        termin: Option<u8>,
    },
    /// Generate the mva-melding XML for a termin; optionally validate it
    /// against Skatteetaten's API (requires Maskinporten env, see docs/gov.md)
    MvaMelding {
        /// Company id (or use --orgnr)
        #[arg(long, conflicts_with = "orgnr")]
        company: Option<Uuid>,
        /// Organization number of the company
        #[arg(long)]
        orgnr: Option<String>,
        /// Year of the termin
        #[arg(long)]
        year: i32,
        /// Termin 1-6 (two-month period)
        #[arg(long)]
        termin: u8,
        /// Output file; "-" writes to stdout
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Also validate against Skatteetaten's validation API
        #[arg(long)]
        validate: bool,
    },
}

async fn resolve_company(
    pool: &sqlx::PgPool,
    company: Option<Uuid>,
    orgnr: Option<&str>,
) -> Result<Uuid> {
    match (company, orgnr) {
        (Some(id), _) => Ok(id),
        (None, Some(orgnr)) => regnmed_db::find_company_by_orgnr(pool, orgnr)
            .await?
            .with_context(|| format!("no company with orgnr {orgnr}")),
        (None, None) => anyhow::bail!("pass --company or --orgnr"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL is not set — copy .env.example to .env")?;
    let pool = regnmed_db::connect(&url)
        .await
        .context("connecting to database")?;

    match cli.command {
        Command::Migrate => {
            regnmed_db::MIGRATOR.run(&pool).await?;
            println!("migrations up to date");
        }
        Command::VerifyLedger { company } => {
            let companies = match company {
                Some(id) => vec![id],
                None => regnmed_db::all_company_ids(&pool).await?,
            };
            if companies.is_empty() {
                println!("no companies in the database");
            }
            for id in companies {
                let report = regnmed_db::verify_chain(&pool, id).await?;
                println!(
                    "company {id}: chain OK ({} vouchers verified)",
                    report.vouchers_checked
                );
            }
        }
        Command::Demo => demo(&pool).await?,
        Command::SaftExport {
            company,
            orgnr,
            year,
            from,
            to,
            contact,
            out,
        } => saft_export(&pool, company, orgnr, year, from, to, &contact, out).await?,
        Command::MvaReport {
            company,
            orgnr,
            year,
            termin,
        } => mva_report(&pool, company, orgnr, year, termin).await?,
        Command::MvaMelding {
            company,
            orgnr,
            year,
            termin,
            out,
            validate,
        } => mva_melding(&pool, company, orgnr, year, termin, out, validate).await?,
    }
    Ok(())
}

async fn mva_melding(
    pool: &sqlx::PgPool,
    company: Option<Uuid>,
    orgnr: Option<String>,
    year: i32,
    termin: u8,
    out: Option<std::path::PathBuf>,
    validate: bool,
) -> Result<()> {
    use regnmed_core::mva::Termin;

    let company_id = resolve_company(pool, company, orgnr.as_deref()).await?;
    let termin = Termin::new(year, termin).context("--termin must be 1-6")?;

    let orgnr: String = sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    let spes =
        regnmed_db::mva_spesifikasjon(pool, company_id, termin.start(), termin.end()).await?;
    anyhow::ensure!(
        !spes.is_empty(),
        "no VAT postings in {termin} — nothing to report"
    );

    let referanse = format!("regnmed-{}-{}-{}", orgnr, termin.year, termin.number);
    let melding = regnmed_core::mvamelding::build(
        &orgnr,
        termin,
        &referanse,
        env!("CARGO_PKG_VERSION"),
        &spes,
    );
    let xml = regnmed_core::mvamelding::render(&melding);

    match out.as_deref() {
        Some(path) if path == std::path::Path::new("-") => {
            use std::io::Write;
            std::io::stdout().write_all(xml.as_bytes())?;
        }
        maybe_path => {
            let path = maybe_path.map(std::path::PathBuf::from).unwrap_or_else(|| {
                format!(
                    "mva-melding_{}_{}-termin{}.xml",
                    orgnr, termin.year, termin.number
                )
                .into()
            });
            std::fs::write(&path, &xml)?;
            println!(
                "wrote {} ({} linjer, fastsatt merverdiavgift {} kr)",
                path.display(),
                melding.lines.len(),
                melding.fastsatt_kr
            );
        }
    }

    if validate {
        let config = regnmed_gov::maskinporten::MaskinportenConfig::from_env()?;
        let tokens = regnmed_gov::maskinporten::TokenProvider::new(config);
        let client = regnmed_gov::mvamelding::MvaMeldingClient::from_env();
        let outcome = client.validate(&tokens, &xml).await?;
        if outcome.valid {
            println!("Skatteetaten: melding validert uten avvik");
        } else {
            println!("Skatteetaten fant avvik:");
            for finding in &outcome.findings {
                println!("  - {finding}");
            }
            anyhow::bail!("mva-melding did not validate");
        }
    }
    Ok(())
}

async fn mva_report(
    pool: &sqlx::PgPool,
    company: Option<Uuid>,
    orgnr: Option<String>,
    year: i32,
    termin: Option<u8>,
) -> Result<()> {
    use regnmed_core::mva::{Direction, Termin, direction};

    let company_id = resolve_company(pool, company, orgnr.as_deref()).await?;
    let (start, end, label) = match termin {
        Some(n) => {
            let t = Termin::new(year, n).context("--termin must be 1-6")?;
            (t.start(), t.end(), t.to_string())
        }
        None => (
            chrono::NaiveDate::from_ymd_opt(year, 1, 1).context("invalid year")?,
            chrono::NaiveDate::from_ymd_opt(year, 12, 31).context("invalid year")?,
            format!("hele {year}"),
        ),
    };

    let lines = regnmed_db::mva_spesifikasjon(pool, company_id, start, end).await?;
    if lines.is_empty() {
        println!("ingen mva-posteringer i perioden {start} – {end}");
        return Ok(());
    }

    println!("Mva-spesifikasjon, {label} ({start} – {end})");
    println!("beløp i kroner, ledger-fortegn: debet positivt\n");
    println!(
        "{:<5} {:>15} {:>15} {:>8}  beskrivelse",
        "kode", "grunnlag", "beregnet avgift", "sats"
    );
    for line in &lines {
        println!(
            "{:<5} {:>15} {:>15} {:>7}%  {}",
            line.code,
            Ore(line.grunnlag_ore).to_string(),
            Ore(line.avgift_ore).to_string(),
            fmt_bp(line.rate_bp),
            line.description
        );
    }

    // Sales bases post as credits (negative), so payable output VAT is the
    // negated sum; deductible input VAT posts as debits (positive).
    let utgaende: i64 = lines
        .iter()
        .filter(|l| direction(&l.code) == Direction::Utgaende)
        .map(|l| -l.avgift_ore)
        .sum();
    let inngaende: i64 = lines
        .iter()
        .filter(|l| direction(&l.code) == Direction::Inngaende)
        .map(|l| l.avgift_ore)
        .sum();
    let netto = utgaende - inngaende;

    println!();
    println!("Utgående avgift:  {:>15}", Ore(utgaende).to_string());
    println!("Inngående avgift: {:>15}", Ore(inngaende).to_string());
    if netto >= 0 {
        println!("Å betale:         {:>15}", Ore(netto).to_string());
    } else {
        println!("Til gode:         {:>15}", Ore(-netto).to_string());
    }

    if lines
        .iter()
        .any(|l| direction(&l.code) == Direction::OmvendtAvgiftsplikt)
    {
        println!(
            "\nmerk: koder med omvendt avgiftsplikt/innførsel er listet, men\n\
             tosidig behandling skjer i mva-meldingen."
        );
    }
    Ok(())
}

/// Basis points as a display percentage: 2500 → "25", 1111 → "11,11".
fn fmt_bp(bp: i64) -> String {
    match (bp / 100, bp % 100) {
        (whole, 0) => format!("{whole}"),
        (whole, frac) => format!("{whole},{frac:02}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn saft_export(
    pool: &sqlx::PgPool,
    company: Option<Uuid>,
    orgnr: Option<String>,
    year: Option<i32>,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
    contact: &str,
    out: Option<std::path::PathBuf>,
) -> Result<()> {
    use chrono::NaiveDate;

    let company_id = resolve_company(pool, company, orgnr.as_deref()).await?;

    let (start, end) = match (year, from, to) {
        (Some(y), _, _) => (
            NaiveDate::from_ymd_opt(y, 1, 1).context("invalid year")?,
            NaiveDate::from_ymd_opt(y, 12, 31).context("invalid year")?,
        ),
        (None, Some(from), Some(to)) => (from, to),
        _ => anyhow::bail!("pass --year, or --from and --to"),
    };
    anyhow::ensure!(start <= end, "--from must not be after --to");

    let (first_name, last_name) = contact
        .trim()
        .rsplit_once(' ')
        .context("--contact must be \"Fornavn Etternavn\"")?;

    let input =
        regnmed_db::load_saft_input(pool, company_id, start, end, first_name, last_name).await?;

    // Accounts the grouping code list has no exact standard account for are
    // legal to export (nearest is used) but worth a review.
    for account in &input.accounts {
        match regnmed_core::saft::grouping_for(&account.number) {
            Some(g) if !g.exact => eprintln!(
                "note: account {} ({}) is not a standard account; grouped as {} ({})",
                account.number, account.name, g.code, g.category
            ),
            None => anyhow::bail!(
                "account {} cannot be mapped to a grouping code",
                account.number
            ),
            _ => {}
        }
    }

    let xml = regnmed_core::saft::render(&input);
    let transactions: usize = input.journals.iter().map(|j| j.transactions.len()).sum();

    match out.as_deref() {
        Some(path) if path == std::path::Path::new("-") => {
            use std::io::Write;
            std::io::stdout().write_all(xml.as_bytes())?;
        }
        maybe_path => {
            let path = maybe_path.map(std::path::PathBuf::from).unwrap_or_else(|| {
                format!(
                    "SAF-T Financial_{}_{}.xml",
                    input.orgnr,
                    Utc::now().format("%Y%m%d%H%M%S")
                )
                .into()
            });
            std::fs::write(&path, &xml)?;
            println!(
                "wrote {} ({} accounts, {} transactions, {} bytes)",
                path.display(),
                input.accounts.len(),
                transactions,
                xml.len()
            );
        }
    }
    Ok(())
}

/// End-to-end smoke test of the ledger core: posts real vouchers, proves
/// the append-only triggers reject tampering, and verifies the hash chain.
async fn demo(pool: &sqlx::PgPool) -> Result<()> {
    regnmed_db::MIGRATOR.run(pool).await?;

    let orgnr = "999888777";
    let company = match regnmed_db::find_company_by_orgnr(pool, orgnr).await? {
        Some(id) => id,
        None => regnmed_db::create_company(pool, orgnr, "Demo AS").await?,
    };
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok").await?;
    regnmed_db::ensure_account(pool, company, "1920", "Bankinnskudd").await?;
    regnmed_db::ensure_account(pool, company, "3000", "Salgsinntekt, avgiftspliktig").await?;
    regnmed_db::ensure_account(pool, company, "2700", "Utgående merverdiavgift").await?;
    regnmed_db::ensure_account(pool, company, "7770", "Bank- og kortgebyr").await?;

    let today = Utc::now().date_naive();

    let sale = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: today,
        description: "Salg av konsulenttjenester".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(12_500_00),
                vat_code: None,
                description: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-10_000_00),
                vat_code: Some("3".into()),
                description: None,
            },
            EntryDraft {
                account_number: "2700".into(),
                amount: Ore(-2_500_00),
                vat_code: None,
                description: None,
            },
        ],
    };
    let posted = regnmed_db::post_voucher(pool, company, &sale, "demo").await?;
    println!(
        "posted voucher {}-{} (seq {}, hash {})",
        posted.fiscal_year,
        posted.voucher_number,
        posted.chain_seq,
        hex::encode(posted.hash)
    );

    let fee = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: today,
        description: "Bankgebyr".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "7770".into(),
                amount: Ore(150_00),
                vat_code: None,
                description: None,
            },
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(-150_00),
                vat_code: None,
                description: None,
            },
        ],
    };
    let posted2 = regnmed_db::post_voucher(pool, company, &fee, "demo").await?;
    println!(
        "posted voucher {}-{} (seq {}, hash {})",
        posted2.fiscal_year,
        posted2.voucher_number,
        posted2.chain_seq,
        hex::encode(posted2.hash)
    );

    // A purchase with deductible input VAT, so mva-report has both sides.
    regnmed_db::ensure_account(pool, company, "4300", "Innkjøp av varer for videresalg").await?;
    regnmed_db::ensure_account(pool, company, "2710", "Inngående merverdiavgift").await?;
    let purchase = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: today,
        description: "Varekjøp".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "4300".into(),
                amount: Ore(8_000_00),
                vat_code: Some("1".into()),
                description: None,
            },
            EntryDraft {
                account_number: "2710".into(),
                amount: Ore(2_000_00),
                vat_code: None,
                description: None,
            },
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(-10_000_00),
                vat_code: None,
                description: None,
            },
        ],
    };
    let posted3 = regnmed_db::post_voucher(pool, company, &purchase, "demo").await?;
    println!(
        "posted voucher {}-{} (seq {}, hash {})",
        posted3.fiscal_year,
        posted3.voucher_number,
        posted3.chain_seq,
        hex::encode(posted3.hash)
    );

    // An unbalanced voucher must be rejected before it reaches the database.
    let unbalanced = VoucherDraft {
        entries: vec![
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(100_00),
                vat_code: None,
                description: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-99_00),
                vat_code: None,
                description: None,
            },
        ],
        ..sale.clone()
    };
    let err = regnmed_db::post_voucher(pool, company, &unbalanced, "demo")
        .await
        .expect_err("unbalanced voucher must be rejected");
    println!("unbalanced voucher rejected: {err}");

    // Direct tampering must be rejected by the append-only trigger.
    let err = sqlx::query("update entry set amount_ore = amount_ore + 100 where voucher_id = $1")
        .bind(posted.id)
        .execute(pool)
        .await
        .expect_err("ledger mutation must be rejected");
    println!("tamper attempt rejected by database: {err}");

    let report = regnmed_db::verify_chain(pool, company).await?;
    println!(
        "chain verified from genesis: {} vouchers OK",
        report.vouchers_checked
    );

    // Marketplace tenancy: an accountant reaches the client company through
    // her firm's engagement, never directly.
    let kari = regnmed_db::ensure_person(
        pool,
        "demo|kari",
        Some("Kari Regnskapsfører"),
        Some("kari@tallogorden.no"),
    )
    .await?;
    let firm =
        regnmed_db::ensure_firm(pool, "998877665", "Tall & Orden Regnskap AS", "regnskap").await?;
    regnmed_db::ensure_firm_member(pool, firm, kari, "ansatt").await?;
    regnmed_db::ensure_engagement(pool, firm, company, "regnskap").await?;

    for access in regnmed_db::company_access_for_person(pool, kari).await? {
        println!(
            "kari may act for {} ({}) with access '{}' via {}",
            access.name, access.orgnr, access.access, access.via
        );
    }

    println!("demo company id: {company}");
    Ok(())
}
