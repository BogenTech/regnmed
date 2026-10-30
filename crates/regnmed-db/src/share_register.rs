//! Aksjeeierbok and aksjonærregisteroppgave (docs/aksjonaer.md, #43).
//!
//! The holding is always COMPUTED from the events (migration 0034), the
//! same way balances are computed from bilag. No query here stores an
//! ownership figure, and none updates one.

use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, NaiveDate};
use regnmed_core::Ore;
use regnmed_core::share_register::{ShareEvent, TransactionType, YearMovement};
use regnmed_core::shareholder_return as tax_return;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

/// Default accounts for the dividend posting. A dividend is a distribution
/// of equity, not a cost: annen egenkapital is debited and the
/// (not yet paid) obligation to the shareholders credited.
pub const ACCOUNT_OTHER_EQUITY: &str = "2050";
pub const ACCOUNT_DIVIDEND_PAYABLE: &str = "2800";

#[derive(Debug, Clone)]
pub struct Shareholder {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    /// Aksjeloven §4-5 asks for the birth DATE, not the birth number.
    /// Derived from the fødselsnummer we hold for the oppgave; the
    /// number itself never leaves this module.
    pub birth_date: Option<NaiveDate>,
    pub orgnumber: Option<String>,
    pub utenlandsk_id: Option<String>,
    pub address: Option<String>,
    pub postnummer: Option<String>,
    pub postal_city: Option<String>,
    pub landkode: Option<String>,
    pub note: Option<String>,
    /// Holding as of the requested date — computed, never stored.
    pub share_count: i64,
    pub andel_bp: i64,
}

#[derive(Debug, Clone)]
pub struct ShareEventRow {
    pub id: Uuid,
    pub shareholder_id: Uuid,
    pub shareholder: String,
    pub type_: String,
    pub type_name: String,
    pub date: NaiveDate,
    pub quantity: i64,
    pub amount_ore: Option<i64>,
    pub motpart: Option<String>,
    pub note: Option<String>,
    pub created_by: String,
}

#[derive(Debug, Clone)]
pub struct DividendResolution {
    pub id: Uuid,
    pub decided_date: NaiveDate,
    pub per_share_ore: i64,
    pub totalt_ore: i64,
    pub voucher_id: Option<Uuid>,
    pub note: Option<String>,
    pub created_by: String,
}

#[derive(Debug, Clone)]
pub struct NewShareholder {
    pub kind: String,
    pub name: String,
    pub fodselsnummer: Option<String>,
    pub orgnumber: Option<String>,
    pub utenlandsk_id: Option<String>,
    pub address: Option<String>,
    pub postnummer: Option<String>,
    pub postal_city: Option<String>,
    pub landkode: Option<String>,
    pub note: Option<String>,
}

/// Registers a shareholder. The identifier is validated here — a
/// fødselsnummer or orgnr with a broken check digit is a typo we can
/// catch now instead of at filing time, months later.
pub async fn create_shareholder(
    pool: &PgPool,
    company_id: Uuid,
    new: &NewShareholder,
    created_by: &str,
) -> Result<Uuid> {
    match new.kind.as_str() {
        "person" => {
            let fnr = new
                .fodselsnummer
                .as_deref()
                .context("personlig aksjonær må ha fødselsnummer")?;
            ensure!(
                regnmed_core::nin::is_valid(fnr),
                "ugyldig fødselsnummer (kontrollsifrene stemmer ikke)"
            );
        }
        "selskap" => {
            let orgnumber = new
                .orgnumber
                .as_deref()
                .context("selskapsaksjonær må ha organisasjonsnummer")?;
            ensure!(
                regnmed_core::orgnumber::is_valid(orgnumber),
                "ugyldig organisasjonsnummer (kontrollsifferet stemmer ikke)"
            );
        }
        "utenlandsk" => {
            let id = new
                .utenlandsk_id
                .as_deref()
                .context("utenlandsk aksjonær må ha aksjonær-ID (UTL…)")?;
            ensure!(
                id.len() == 12
                    && id.starts_with("UTL")
                    && id[3..].chars().all(|c| c.is_ascii_digit()),
                "utenlandsk aksjonær-ID har formen UTL etterfulgt av ni siffer"
            );
        }
        other => bail!("ukjent aksjonærtype «{other}»"),
    }

    let id = Uuid::new_v4();
    sqlx::query(
        "insert into shareholder
             (id, company_id, kind, fodselsnummer, orgnr, utenlandsk_id, navn,
              adresse, postnummer, poststed, landkode, note, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&new.kind)
    .bind(&new.fodselsnummer)
    .bind(&new.orgnumber)
    .bind(&new.utenlandsk_id)
    .bind(&new.name)
    .bind(&new.address)
    .bind(&new.postnummer)
    .bind(&new.postal_city)
    .bind(&new.landkode)
    .bind(&new.note)
    .bind(created_by)
    .execute(pool)
    .await
    .context("aksjonæren finnes allerede i denne aksjeeierboken")?;
    Ok(id)
}

/// Editable contact data. Identity is not here — the trigger refuses it.
pub async fn update_shareholder_contact(
    pool: &PgPool,
    company_id: Uuid,
    shareholder_id: Uuid,
    name: &str,
    address: Option<&str>,
    postnummer: Option<&str>,
    postal_city: Option<&str>,
    landkode: Option<&str>,
) -> Result<()> {
    ensure!(!name.trim().is_empty(), "navn kan ikke være tomt");
    let done = sqlx::query(
        "update shareholder set navn = $3, adresse = $4, postnummer = $5,
                poststed = $6, landkode = $7
         where id = $1 and company_id = $2",
    )
    .bind(shareholder_id)
    .bind(company_id)
    .bind(name)
    .bind(address)
    .bind(postnummer)
    .bind(postal_city)
    .bind(landkode)
    .execute(pool)
    .await?;
    ensure!(done.rows_affected() == 1, "no such aksjonær");
    Ok(())
}

/// The aksjeeierbok as of `dato` — aksjeloven §4-5's register.
///
/// Shareholders with a zero holding are kept: someone who sold out
/// during the year is still part of this year's oppgave, and the book
/// should show that they were once an owner.
pub async fn aksjeeierbok(
    pool: &PgPool,
    company_id: Uuid,
    date: NaiveDate,
) -> Result<Vec<Shareholder>> {
    let rows = sqlx::query(
        "select s.id, s.kind, s.navn, s.fodselsnummer, s.orgnr, s.utenlandsk_id,
                s.adresse, s.postnummer, s.poststed, s.landkode, s.note,
                coalesce((select sum(case when e.type = any($3) then -e.antall else e.antall end)::bigint
                          from share_event e
                          where e.shareholder_id = s.id and e.dato <= $2), 0) as antall
         from shareholder s
         where s.company_id = $1
         order by antall desc, s.navn",
    )
    .bind(company_id)
    .bind(date)
    .bind(avgangstyper())
    .fetch_all(pool)
    .await?;

    let total: i64 = rows.iter().map(|r| r.get::<i64, _>("antall")).sum();
    Ok(rows
        .iter()
        .map(|r| {
            let quantity: i64 = r.get("antall");
            Shareholder {
                id: r.get("id"),
                kind: r.get("kind"),
                name: r.get("navn"),
                birth_date: r
                    .get::<Option<String>, _>("fodselsnummer")
                    .as_deref()
                    .and_then(regnmed_core::nin::birth_date),
                orgnumber: r.get("orgnr"),
                utenlandsk_id: r.get("utenlandsk_id"),
                address: r.get("adresse"),
                postnummer: r.get("postnummer"),
                postal_city: r.get("poststed"),
                landkode: r.get("landkode"),
                note: r.get("note"),
                share_count: quantity,
                // Basispunkter, ingen flyttall.
                andel_bp: if total > 0 {
                    (quantity as i128 * 10_000 / total as i128) as i64
                } else {
                    0
                },
            }
        })
        .collect())
}

/// The avgang type slugs, as SQL needs them to resolve direction.
/// Derived from the core enum so the two can never drift apart.
fn avgangstyper() -> Vec<String> {
    regnmed_core::share_register::ALL
        .iter()
        .filter(|t| !t.is_acquisition())
        .map(|t| t.slug().to_string())
        .collect()
}

pub async fn list_hendelser(
    pool: &PgPool,
    company_id: Uuid,
    year: Option<i32>,
) -> Result<Vec<ShareEventRow>> {
    let rows = sqlx::query(
        "select e.id, e.shareholder_id, s.navn as aksjonaer, e.type, e.dato, e.antall,
                e.belop_ore, e.note, e.created_by,
                (select m.navn from shareholder m where m.id = e.motpart_id) as motpart
         from share_event e
         join shareholder s on s.id = e.shareholder_id
         where e.company_id = $1
           and ($2::int is null or extract(year from e.dato) = $2)
         order by e.dato desc, e.created_at desc",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let slug: String = r.get("type");
            ShareEventRow {
                id: r.get("id"),
                shareholder_id: r.get("shareholder_id"),
                shareholder: r.get("aksjonaer"),
                type_name: TransactionType::from_slug(&slug)
                    .map(|t| t.name().to_string())
                    .unwrap_or_else(|| slug.clone()),
                type_: slug,
                date: r.get("dato"),
                quantity: r.get("antall"),
                amount_ore: r.get("belop_ore"),
                motpart: r.get("motpart"),
                note: r.get("note"),
                created_by: r.get("created_by"),
            }
        })
        .collect())
}

/// Records a movement. A transfer between two shareholders is TWO rows
/// written in ONE transaction — an avgang on the seller and a tilgang on
/// the buyer — so the book can never hold half a sale.
#[allow(clippy::too_many_arguments)]
pub async fn record_hendelse(
    pool: &PgPool,
    company_id: Uuid,
    shareholder_id: Uuid,
    type_slug: &str,
    date: NaiveDate,
    quantity: i64,
    amount_ore: Option<i64>,
    motpart_id: Option<Uuid>,
    motpart_type_slug: Option<&str>,
    note: Option<&str>,
    created_by: &str,
) -> Result<Uuid> {
    let type_ = TransactionType::from_slug(type_slug)
        .with_context(|| format!("ukjent transaksjonstype «{type_slug}»"))?;
    ensure!(quantity > 0, "antall aksjer må være positivt");

    let mut tx = pool.begin().await?;
    // The shareholder must belong to this company.
    let owner: Option<Uuid> = sqlx::query_scalar(
        "select id from shareholder where id = $1 and company_id = $2 for update",
    )
    .bind(shareholder_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?;
    owner.context("no such aksjonær")?;

    // An avgang cannot take more shares than the owner holds on the date.
    if !type_.is_acquisition() {
        let holding = beholdning_i_tx(&mut tx, shareholder_id, date).await?;
        ensure!(
            holding >= quantity,
            "aksjonæren har {holding} aksjer den {date} og kan ikke avhende {quantity}"
        );
    }

    let id = Uuid::new_v4();
    insert_event(
        &mut tx,
        id,
        company_id,
        shareholder_id,
        type_.slug(),
        date,
        quantity,
        amount_ore,
        motpart_id,
        note,
        created_by,
    )
    .await?;

    // The counterparty, when the transfer has a known other side.
    if let (Some(motpart_id), Some(motpart_slug)) = (motpart_id, motpart_type_slug) {
        let motpart_type = TransactionType::from_slug(motpart_slug)
            .with_context(|| format!("ukjent transaksjonstype «{motpart_slug}»"))?;
        ensure!(
            motpart_type.is_acquisition() != type_.is_acquisition(),
            "de to sidene av en overdragelse må gå hver sin vei"
        );
        if !motpart_type.is_acquisition() {
            let holding = beholdning_i_tx(&mut tx, motpart_id, date).await?;
            ensure!(
                holding >= quantity,
                "motparten har bare {holding} aksjer den {date}"
            );
        }
        insert_event(
            &mut tx,
            Uuid::new_v4(),
            company_id,
            motpart_id,
            motpart_type.slug(),
            date,
            quantity,
            amount_ore,
            Some(shareholder_id),
            note,
            created_by,
        )
        .await?;
    }

    tx.commit().await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    company_id: Uuid,
    shareholder_id: Uuid,
    type_slug: &str,
    date: NaiveDate,
    quantity: i64,
    amount_ore: Option<i64>,
    motpart_id: Option<Uuid>,
    note: Option<&str>,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into share_event
             (id, company_id, shareholder_id, type, dato, antall, belop_ore,
              motpart_id, note, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(company_id)
    .bind(shareholder_id)
    .bind(type_slug)
    .bind(date)
    .bind(quantity)
    .bind(amount_ore)
    .bind(motpart_id)
    .bind(note)
    .bind(created_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn beholdning_i_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    shareholder_id: Uuid,
    date: NaiveDate,
) -> Result<i64> {
    let quantity: Option<i64> = sqlx::query_scalar(
        "select sum(case when type = any($3) then -antall else antall end)::bigint
         from share_event where shareholder_id = $1 and dato <= $2",
    )
    .bind(shareholder_id)
    .bind(date)
    .bind(avgangstyper())
    .fetch_one(tx.as_mut())
    .await?;
    Ok(quantity.unwrap_or(0))
}

/// Every event for one shareholder, as the core fold wants them.
async fn hendelser_for(pool: &PgPool, shareholder_id: Uuid) -> Result<Vec<ShareEvent>> {
    let rows = sqlx::query(
        "select type, dato, antall, belop_ore from share_event
         where shareholder_id = $1 order by dato, created_at",
    )
    .bind(shareholder_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let slug: String = r.get("type");
            Some(ShareEvent {
                date: r.get("dato"),
                type_: TransactionType::from_slug(&slug)?,
                quantity: r.get("antall"),
                consideration_ore: r.get("belop_ore"),
            })
        })
        .collect())
}

/// Registers an utbytte decision and posts it in ONE transaction.
///
/// The per-shareholder split is never stored: it is the share count on the
/// decision date times utbytte per aksje, so the parts always sum to the
/// whole.
pub async fn create_utbytte(
    pool: &PgPool,
    company_id: Uuid,
    decided_date: NaiveDate,
    per_share_ore: i64,
    note: Option<&str>,
    created_by: &str,
) -> Result<DividendResolution> {
    ensure!(per_share_ore > 0, "utbytte per aksje må være positivt");
    let bok = aksjeeierbok(pool, company_id, decided_date).await?;
    let quantity: i64 = bok.iter().map(|a| a.share_count).sum();
    ensure!(
        quantity > 0,
        "aksjeeierboken er tom den {decided_date} — registrer stiftelsen først"
    );
    let totalt_ore = quantity
        .checked_mul(per_share_ore)
        .context("utbyttebeløpet er urimelig stort")?;

    let mut tx = pool.begin().await?;
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: decided_date,
        description: format!("Utbytte, vedtatt {decided_date}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: ACCOUNT_OTHER_EQUITY.into(),
                amount: Ore(totalt_ore),
                vat_code: None,
                description: Some("Avsatt utbytte".into()),
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: ACCOUNT_DIVIDEND_PAYABLE.into(),
                amount: Ore(-totalt_ore),
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
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;

    let id = Uuid::new_v4();
    sqlx::query(
        "insert into dividend
             (id, company_id, besluttet_dato, per_aksje_ore, note, voucher_id, created_by)
         values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(decided_date)
    .bind(per_share_ore)
    .bind(note)
    .bind(posted.id)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(DividendResolution {
        id,
        decided_date,
        per_share_ore,
        totalt_ore,
        voucher_id: Some(posted.id),
        note: note.map(str::to_string),
        created_by: created_by.to_string(),
    })
}

pub async fn list_utbytte(
    pool: &PgPool,
    company_id: Uuid,
    year: Option<i32>,
) -> Result<Vec<DividendResolution>> {
    let rows = sqlx::query(
        "select id, besluttet_dato, per_aksje_ore, note, voucher_id, created_by
         from dividend
         where company_id = $1 and ($2::int is null or extract(year from besluttet_dato) = $2)
         order by besluttet_dato desc",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        let date: NaiveDate = r.get("besluttet_dato");
        let per_aksje: i64 = r.get("per_aksje_ore");
        let quantity: i64 = aksjeeierbok(pool, company_id, date)
            .await?
            .iter()
            .map(|a| a.share_count)
            .sum();
        out.push(DividendResolution {
            id: r.get("id"),
            decided_date: date,
            per_share_ore: per_aksje,
            totalt_ore: quantity * per_aksje,
            voucher_id: r.get("voucher_id"),
            note: r.get("note"),
            created_by: r.get("created_by"),
        });
    }
    Ok(out)
}

// ------------------------------------------------------------- oppgaven

/// Everything the RF-1086 filing needs, built from the aksjeeierbok.
pub struct ReturnSet {
    pub hovedskjema: tax_return::MainForm,
    pub underskjemaer: Vec<(Uuid, tax_return::SubForm)>,
}

/// Builds the year's oppgave. Amounts come from the aksjebok, not from
/// anything the user retypes for the filing.
pub async fn bygg_oppgave(pool: &PgPool, company_id: Uuid, year: i32) -> Result<ReturnSet> {
    let company = sqlx::query("select orgnr, name, address, email from company where id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such company")?;
    let orgnumber: String = company.get("orgnr");
    // The address is free text with us; the oppgave wants it split. The
    // same rule EHF uses, in one place.
    let (gate, postnr, postal_city) = crate::ehf::split_address(company.get("address"));

    // The fiscal year is the calendar year (docs/regelverk.md, #52) — and
    // for the aksjonærregisteroppgave it is the inntektsår that governs
    // anyway.
    let last_this_year = NaiveDate::from_ymd_opt(year, 12, 31).context("ugyldig år")?;

    let bok = aksjeeierbok(pool, company_id, last_this_year).await?;
    ensure!(
        !bok.is_empty(),
        "aksjeeierboken er tom — registrer aksjonærene og stiftelsen før oppgaven"
    );

    let utbytte_rader = list_utbytte(pool, company_id, Some(year)).await?;

    let mut shares_this_year = 0i64;
    let mut shares_last_year = 0i64;
    let mut nyutstedelser: Vec<tax_return::NewIssue> = Vec::new();
    let mut underskjemaer = Vec::new();

    // The nominal value is known through the events only when somebody
    // registered the stiftelse with an amount; otherwise it stands at
    // zero and the company must fill it in. The share capital is derived
    // from count x nominal value.
    let palydende_ore = palydende_ore(pool, company_id).await?;

    for a in &bok {
        let events = hendelser_for(pool, a.id).await?;
        let bevegelse: YearMovement = regnmed_core::share_register::year_movement(&events, year);
        shares_this_year += bevegelse.closing;
        shares_last_year += bevegelse.opening;

        let mut bevegelser = Vec::new();
        for h in events.iter().filter(|h| h.date.year() == year) {
            bevegelser.push(tax_return::ShareMovement {
                date: h.date,
                type_: h.type_,
                quantity: h.quantity,
                amount_ore: h.consideration_ore,
            });
            // New issues are reported at company level too.
            if matches!(
                h.type_,
                TransactionType::Stiftelse | TransactionType::Nyemisjon
            ) {
                nyutstedelser.push(tax_return::NewIssue {
                    date: h.date,
                    type_: h.type_,
                    new_shares: h.quantity,
                    shares_after: 0, // fylles når totalen er kjent
                    palydende_ore,
                    overkurs_ore: 0,
                });
            }
        }

        let dividend = utbytte_rader
            .iter()
            .filter_map(|u| {
                let eid = regnmed_core::share_register::holding(&events, u.decided_date);
                (eid > 0).then_some(tax_return::DividendEntry {
                    date: u.decided_date,
                    amount_ore: eid * u.per_share_ore,
                    share_count: eid,
                })
            })
            .collect();

        underskjemaer.push((
            a.id,
            tax_return::SubForm {
                orgnumber: orgnumber.clone(),
                inntektsar: year,
                id: aksjonaerid(pool, a).await?,
                name: a.name.clone(),
                address: a.address.clone(),
                postnummer: a.postnummer.clone(),
                postal_city: a.postal_city.clone(),
                landkode: a.landkode.clone(),
                share_count: tax_return::PriorYear {
                    fjoraret: bevegelse.opening,
                    this_year: bevegelse.closing,
                },
                dividend,
                bevegelser,
            },
        ));
    }

    // "Antall aksjer etter stiftelse/nyemisjon" is the total ON THE
    // EVENT'S DATE, not at year end. With one emisjon they are the same
    // number; with two, the year-end total would be wrong for the first.
    nyutstedelser.sort_by_key(|n| n.date);
    for n in &mut nyutstedelser {
        n.shares_after = aksjeeierbok(pool, company_id, n.date)
            .await?
            .iter()
            .map(|a| a.share_count)
            .sum();
    }

    let aksjekapital = tax_return::PriorYear {
        fjoraret: shares_last_year * palydende_ore,
        this_year: shares_this_year * palydende_ore,
    };

    Ok(ReturnSet {
        hovedskjema: tax_return::MainForm {
            company: tax_return::Company {
                orgnumber,
                name: company.get("name"),
                address: gate,
                postnummer: postnr,
                postal_city,
                contact_name: None,
                contact_email: company.get("email"),
            },
            inntektsar: year,
            aksjekapital,
            palydende_ore: tax_return::PriorYear {
                fjoraret: palydende_ore,
                this_year: palydende_ore,
            },
            share_count: tax_return::PriorYear {
                fjoraret: shares_last_year,
                this_year: shares_this_year,
            },
            innbetalt_aksjekapital: aksjekapital,
            overkurs: tax_return::PriorYear::default(),
            dividend: utbytte_rader
                .iter()
                .map(|u| tax_return::Distribution {
                    date: u.decided_date,
                    per_share_ore: u.per_share_ore,
                    totalt_ore: u.totalt_ore,
                })
                .collect(),
            nyutstedelser,
        },
        underskjemaer,
    })
}

/// Resolves the identifier the oppgave files under.
///
/// The fødselsnummer is read HERE, straight from the table, and not
/// carried on [`Aksjonaer`] — that struct is what the aksjeeierbok and
/// the portal show, and per aksjeloven §4-5 it holds only the birth
/// date. The number exists for one purpose, and takes one path.
async fn aksjonaerid(pool: &PgPool, a: &Shareholder) -> Result<Option<tax_return::ShareholderId>> {
    if let Some(orgnumber) = &a.orgnumber {
        return Ok(Some(tax_return::ShareholderId::Organisasjonsnummer(
            orgnumber.clone(),
        )));
    }
    if let Some(utl) = &a.utenlandsk_id {
        return Ok(Some(tax_return::ShareholderId::Utenlandsk(utl.clone())));
    }
    let fnr: Option<String> =
        sqlx::query_scalar("select fodselsnummer from shareholder where id = $1")
            .bind(a.id)
            .fetch_one(pool)
            .await?;
    Ok(fnr.map(tax_return::ShareholderId::Fodselsnummer))
}

/// Pålydende per aksje, derived from the founding event when it carries
/// an amount. Zero when unknown — better an obvious zero the filer must
/// correct than an invented number.
async fn palydende_ore(pool: &PgPool, company_id: Uuid) -> Result<i64> {
    let row = sqlx::query(
        "select antall, belop_ore from share_event
         where company_id = $1 and type = 'stiftelse' and belop_ore is not null
         order by dato limit 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some(r) => {
            let quantity: i64 = r.get("antall");
            let amount: i64 = r.get("belop_ore");
            if quantity > 0 { amount / quantity } else { 0 }
        }
        None => 0,
    })
}
