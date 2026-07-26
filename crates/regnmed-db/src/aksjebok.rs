//! Aksjeeierbok og aksjonærregisteroppgave (docs/aksjonaer.md, #43).
//!
//! Beholdningen er alltid BEREGNET fra hendelsene (migration 0034), på
//! samme måte som saldoer beregnes fra bilag. Ingen spørring her lagrer
//! et eierandelstall, og ingen oppdaterer et.

use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, NaiveDate};
use regnmed_core::Ore;
use regnmed_core::aksjebok::{Aarsbevegelse, Hendelse, Transaksjonstype};
use regnmed_core::aksjonaeroppgave as oppgave;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

/// Default accounts for the utbytte posting. Utbytte is a distribution
/// of equity, not a cost: annen egenkapital is debited and the
/// (not yet paid) obligation to the shareholders credited.
pub const KONTO_ANNEN_EGENKAPITAL: &str = "2050";
pub const KONTO_AVSATT_UTBYTTE: &str = "2800";

#[derive(Debug, Clone)]
pub struct Aksjonaer {
    pub id: Uuid,
    pub kind: String,
    pub navn: String,
    /// Aksjeloven §4-5 asks for the birth DATE, not the birth number.
    /// Derived from the fødselsnummer we hold for the oppgave; the
    /// number itself never leaves this module.
    pub fodselsdato: Option<NaiveDate>,
    pub orgnr: Option<String>,
    pub utenlandsk_id: Option<String>,
    pub adresse: Option<String>,
    pub postnummer: Option<String>,
    pub poststed: Option<String>,
    pub landkode: Option<String>,
    pub note: Option<String>,
    /// Holding as of the requested date — computed, never stored.
    pub antall_aksjer: i64,
    pub andel_bp: i64,
}

#[derive(Debug, Clone)]
pub struct Aksjehendelse {
    pub id: Uuid,
    pub shareholder_id: Uuid,
    pub aksjonaer: String,
    pub type_: String,
    pub type_navn: String,
    pub dato: NaiveDate,
    pub antall: i64,
    pub belop_ore: Option<i64>,
    pub motpart: Option<String>,
    pub note: Option<String>,
    pub created_by: String,
}

#[derive(Debug, Clone)]
pub struct Utbyttevedtak {
    pub id: Uuid,
    pub besluttet_dato: NaiveDate,
    pub per_aksje_ore: i64,
    pub totalt_ore: i64,
    pub voucher_id: Option<Uuid>,
    pub note: Option<String>,
    pub created_by: String,
}

#[derive(Debug, Clone)]
pub struct NyAksjonaer {
    pub kind: String,
    pub navn: String,
    pub fodselsnummer: Option<String>,
    pub orgnr: Option<String>,
    pub utenlandsk_id: Option<String>,
    pub adresse: Option<String>,
    pub postnummer: Option<String>,
    pub poststed: Option<String>,
    pub landkode: Option<String>,
    pub note: Option<String>,
}

/// Registers a shareholder. The identifier is validated here — a
/// fødselsnummer or orgnr with a broken check digit is a typo we can
/// catch now instead of at filing time, months later.
pub async fn create_aksjonaer(
    pool: &PgPool,
    company_id: Uuid,
    ny: &NyAksjonaer,
    created_by: &str,
) -> Result<Uuid> {
    match ny.kind.as_str() {
        "person" => {
            let fnr = ny
                .fodselsnummer
                .as_deref()
                .context("personlig aksjonær må ha fødselsnummer")?;
            ensure!(
                regnmed_core::fnr::is_valid(fnr),
                "ugyldig fødselsnummer (kontrollsifrene stemmer ikke)"
            );
        }
        "selskap" => {
            let orgnr = ny
                .orgnr
                .as_deref()
                .context("selskapsaksjonær må ha organisasjonsnummer")?;
            ensure!(
                regnmed_core::orgnr::is_valid(orgnr),
                "ugyldig organisasjonsnummer (kontrollsifferet stemmer ikke)"
            );
        }
        "utenlandsk" => {
            let id = ny
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
    .bind(&ny.kind)
    .bind(&ny.fodselsnummer)
    .bind(&ny.orgnr)
    .bind(&ny.utenlandsk_id)
    .bind(&ny.navn)
    .bind(&ny.adresse)
    .bind(&ny.postnummer)
    .bind(&ny.poststed)
    .bind(&ny.landkode)
    .bind(&ny.note)
    .bind(created_by)
    .execute(pool)
    .await
    .context("aksjonæren finnes allerede i denne aksjeeierboken")?;
    Ok(id)
}

/// Editable contact data. Identity is not here — the trigger refuses it.
pub async fn update_aksjonaer_kontakt(
    pool: &PgPool,
    company_id: Uuid,
    shareholder_id: Uuid,
    navn: &str,
    adresse: Option<&str>,
    postnummer: Option<&str>,
    poststed: Option<&str>,
    landkode: Option<&str>,
) -> Result<()> {
    ensure!(!navn.trim().is_empty(), "navn kan ikke være tomt");
    let done = sqlx::query(
        "update shareholder set navn = $3, adresse = $4, postnummer = $5,
                poststed = $6, landkode = $7
         where id = $1 and company_id = $2",
    )
    .bind(shareholder_id)
    .bind(company_id)
    .bind(navn)
    .bind(adresse)
    .bind(postnummer)
    .bind(poststed)
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
    dato: NaiveDate,
) -> Result<Vec<Aksjonaer>> {
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
    .bind(dato)
    .bind(avgangstyper())
    .fetch_all(pool)
    .await?;

    let total: i64 = rows.iter().map(|r| r.get::<i64, _>("antall")).sum();
    Ok(rows
        .iter()
        .map(|r| {
            let antall: i64 = r.get("antall");
            Aksjonaer {
                id: r.get("id"),
                kind: r.get("kind"),
                navn: r.get("navn"),
                fodselsdato: r
                    .get::<Option<String>, _>("fodselsnummer")
                    .as_deref()
                    .and_then(regnmed_core::fnr::fodselsdato),
                orgnr: r.get("orgnr"),
                utenlandsk_id: r.get("utenlandsk_id"),
                adresse: r.get("adresse"),
                postnummer: r.get("postnummer"),
                poststed: r.get("poststed"),
                landkode: r.get("landkode"),
                note: r.get("note"),
                antall_aksjer: antall,
                // Basispunkter, ingen flyttall.
                andel_bp: if total > 0 {
                    (antall as i128 * 10_000 / total as i128) as i64
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
    regnmed_core::aksjebok::ALLE
        .iter()
        .filter(|t| !t.er_tilgang())
        .map(|t| t.slug().to_string())
        .collect()
}

pub async fn list_hendelser(
    pool: &PgPool,
    company_id: Uuid,
    ar: Option<i32>,
) -> Result<Vec<Aksjehendelse>> {
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
    .bind(ar)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let slug: String = r.get("type");
            Aksjehendelse {
                id: r.get("id"),
                shareholder_id: r.get("shareholder_id"),
                aksjonaer: r.get("aksjonaer"),
                type_navn: Transaksjonstype::fra_slug(&slug)
                    .map(|t| t.navn().to_string())
                    .unwrap_or_else(|| slug.clone()),
                type_: slug,
                dato: r.get("dato"),
                antall: r.get("antall"),
                belop_ore: r.get("belop_ore"),
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
    dato: NaiveDate,
    antall: i64,
    belop_ore: Option<i64>,
    motpart_id: Option<Uuid>,
    motpart_type_slug: Option<&str>,
    note: Option<&str>,
    created_by: &str,
) -> Result<Uuid> {
    let type_ = Transaksjonstype::fra_slug(type_slug)
        .with_context(|| format!("ukjent transaksjonstype «{type_slug}»"))?;
    ensure!(antall > 0, "antall aksjer må være positivt");

    let mut tx = pool.begin().await?;
    // Aksjonæren må høre til dette selskapet.
    let eier: Option<Uuid> = sqlx::query_scalar(
        "select id from shareholder where id = $1 and company_id = $2 for update",
    )
    .bind(shareholder_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?;
    eier.context("no such aksjonær")?;

    // En avgang kan ikke ta flere aksjer enn eieren har på datoen.
    if !type_.er_tilgang() {
        let beholdning = beholdning_i_tx(&mut tx, shareholder_id, dato).await?;
        ensure!(
            beholdning >= antall,
            "aksjonæren har {beholdning} aksjer den {dato} og kan ikke avhende {antall}"
        );
    }

    let id = Uuid::new_v4();
    insert_event(
        &mut tx,
        id,
        company_id,
        shareholder_id,
        type_.slug(),
        dato,
        antall,
        belop_ore,
        motpart_id,
        note,
        created_by,
    )
    .await?;

    // Motparten, når overdragelsen har en kjent annen side.
    if let (Some(motpart_id), Some(motpart_slug)) = (motpart_id, motpart_type_slug) {
        let motpart_type = Transaksjonstype::fra_slug(motpart_slug)
            .with_context(|| format!("ukjent transaksjonstype «{motpart_slug}»"))?;
        ensure!(
            motpart_type.er_tilgang() != type_.er_tilgang(),
            "de to sidene av en overdragelse må gå hver sin vei"
        );
        if !motpart_type.er_tilgang() {
            let beholdning = beholdning_i_tx(&mut tx, motpart_id, dato).await?;
            ensure!(
                beholdning >= antall,
                "motparten har bare {beholdning} aksjer den {dato}"
            );
        }
        insert_event(
            &mut tx,
            Uuid::new_v4(),
            company_id,
            motpart_id,
            motpart_type.slug(),
            dato,
            antall,
            belop_ore,
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
    dato: NaiveDate,
    antall: i64,
    belop_ore: Option<i64>,
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
    .bind(dato)
    .bind(antall)
    .bind(belop_ore)
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
    dato: NaiveDate,
) -> Result<i64> {
    let antall: Option<i64> = sqlx::query_scalar(
        "select sum(case when type = any($3) then -antall else antall end)::bigint
         from share_event where shareholder_id = $1 and dato <= $2",
    )
    .bind(shareholder_id)
    .bind(dato)
    .bind(avgangstyper())
    .fetch_one(tx.as_mut())
    .await?;
    Ok(antall.unwrap_or(0))
}

/// Every event for one shareholder, as the core fold wants them.
async fn hendelser_for(pool: &PgPool, shareholder_id: Uuid) -> Result<Vec<Hendelse>> {
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
            Some(Hendelse {
                dato: r.get("dato"),
                type_: Transaksjonstype::fra_slug(&slug)?,
                antall: r.get("antall"),
                vederlag_ore: r.get("belop_ore"),
            })
        })
        .collect())
}

/// Registers an utbytte decision and posts it in ONE transaction.
///
/// The per-shareholder split is never stored: it is antall aksjer on the
/// decision date times utbytte per aksje, so the parts always sum to the
/// whole.
pub async fn create_utbytte(
    pool: &PgPool,
    company_id: Uuid,
    besluttet_dato: NaiveDate,
    per_aksje_ore: i64,
    note: Option<&str>,
    created_by: &str,
) -> Result<Utbyttevedtak> {
    ensure!(per_aksje_ore > 0, "utbytte per aksje må være positivt");
    let bok = aksjeeierbok(pool, company_id, besluttet_dato).await?;
    let antall: i64 = bok.iter().map(|a| a.antall_aksjer).sum();
    ensure!(
        antall > 0,
        "aksjeeierboken er tom den {besluttet_dato} — registrer stiftelsen først"
    );
    let totalt_ore = antall
        .checked_mul(per_aksje_ore)
        .context("utbyttebeløpet er urimelig stort")?;

    let mut tx = pool.begin().await?;
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: besluttet_dato,
        description: format!("Utbytte, vedtatt {besluttet_dato}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: KONTO_ANNEN_EGENKAPITAL.into(),
                amount: Ore(totalt_ore),
                vat_code: None,
                description: Some("Avsatt utbytte".into()),
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            EntryDraft {
                account_number: KONTO_AVSATT_UTBYTTE.into(),
                amount: Ore(-totalt_ore),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
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
    .bind(besluttet_dato)
    .bind(per_aksje_ore)
    .bind(note)
    .bind(posted.id)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Utbyttevedtak {
        id,
        besluttet_dato,
        per_aksje_ore,
        totalt_ore,
        voucher_id: Some(posted.id),
        note: note.map(str::to_string),
        created_by: created_by.to_string(),
    })
}

pub async fn list_utbytte(
    pool: &PgPool,
    company_id: Uuid,
    ar: Option<i32>,
) -> Result<Vec<Utbyttevedtak>> {
    let rows = sqlx::query(
        "select id, besluttet_dato, per_aksje_ore, note, voucher_id, created_by
         from dividend
         where company_id = $1 and ($2::int is null or extract(year from besluttet_dato) = $2)
         order by besluttet_dato desc",
    )
    .bind(company_id)
    .bind(ar)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        let dato: NaiveDate = r.get("besluttet_dato");
        let per_aksje: i64 = r.get("per_aksje_ore");
        let antall: i64 = aksjeeierbok(pool, company_id, dato)
            .await?
            .iter()
            .map(|a| a.antall_aksjer)
            .sum();
        out.push(Utbyttevedtak {
            id: r.get("id"),
            besluttet_dato: dato,
            per_aksje_ore: per_aksje,
            totalt_ore: antall * per_aksje,
            voucher_id: r.get("voucher_id"),
            note: r.get("note"),
            created_by: r.get("created_by"),
        });
    }
    Ok(out)
}

// ------------------------------------------------------------- oppgaven

/// Everything the RF-1086 filing needs, built from the aksjeeierbok.
pub struct Oppgavesett {
    pub hovedskjema: oppgave::Hovedskjema,
    pub underskjemaer: Vec<(Uuid, oppgave::Underskjema)>,
}

/// Builds the year's oppgave. Amounts come from the aksjebok, not from
/// anything the user retypes for the filing.
pub async fn bygg_oppgave(pool: &PgPool, company_id: Uuid, ar: i32) -> Result<Oppgavesett> {
    let selskap = sqlx::query("select orgnr, name, address, email from company where id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such company")?;
    let orgnr: String = selskap.get("orgnr");
    // Adressen er fritekst hos oss; oppgaven vil ha den delt. Samme
    // regel som EHF bruker, ett sted.
    let (gate, postnr, poststed) = crate::ehf::split_adresse(selskap.get("address"));

    // Regnskapsåret er kalenderåret (docs/regelverk.md, #52) — og for
    // aksjonærregisteroppgaven er det uansett inntektsåret som gjelder.
    let siste_i_ar = NaiveDate::from_ymd_opt(ar, 12, 31).context("ugyldig år")?;

    let bok = aksjeeierbok(pool, company_id, siste_i_ar).await?;
    ensure!(
        !bok.is_empty(),
        "aksjeeierboken er tom — registrer aksjonærene og stiftelsen før oppgaven"
    );

    let utbytte_rader = list_utbytte(pool, company_id, Some(ar)).await?;

    let mut antall_i_ar = 0i64;
    let mut antall_i_fjor = 0i64;
    let mut nyutstedelser: Vec<oppgave::Nyutstedelse> = Vec::new();
    let mut underskjemaer = Vec::new();

    // Pålydende er kjent gjennom hendelsene bare når noen har registrert
    // stiftelsen med et beløp; ellers står det som null og selskapet må
    // fylle det. Aksjekapitalen utledes av antall x pålydende.
    let palydende_ore = palydende_ore(pool, company_id).await?;

    for a in &bok {
        let hendelser = hendelser_for(pool, a.id).await?;
        let bevegelse: Aarsbevegelse = regnmed_core::aksjebok::aarsbevegelse(&hendelser, ar);
        antall_i_ar += bevegelse.utgaende;
        antall_i_fjor += bevegelse.inngaende;

        let mut bevegelser = Vec::new();
        for h in hendelser.iter().filter(|h| h.dato.year() == ar) {
            bevegelser.push(oppgave::Bevegelse {
                dato: h.dato,
                type_: h.type_,
                antall: h.antall,
                belop_ore: h.vederlag_ore,
            });
            // Nyutstedelser rapporteres også på selskapsnivå.
            if matches!(
                h.type_,
                Transaksjonstype::Stiftelse | Transaksjonstype::Nyemisjon
            ) {
                nyutstedelser.push(oppgave::Nyutstedelse {
                    dato: h.dato,
                    type_: h.type_,
                    antall_nye: h.antall,
                    antall_etter: 0, // fylles når totalen er kjent
                    palydende_ore,
                    overkurs_ore: 0,
                });
            }
        }

        let utbytte = utbytte_rader
            .iter()
            .filter_map(|u| {
                let eid = regnmed_core::aksjebok::beholdning(&hendelser, u.besluttet_dato);
                (eid > 0).then_some(oppgave::Utbyttepost {
                    dato: u.besluttet_dato,
                    belop_ore: eid * u.per_aksje_ore,
                    antall_aksjer: eid,
                })
            })
            .collect();

        underskjemaer.push((
            a.id,
            oppgave::Underskjema {
                orgnr: orgnr.clone(),
                inntektsar: ar,
                id: aksjonaerid(pool, a).await?,
                navn: a.navn.clone(),
                adresse: a.adresse.clone(),
                postnummer: a.postnummer.clone(),
                poststed: a.poststed.clone(),
                landkode: a.landkode.clone(),
                antall_aksjer: oppgave::Fjoraret {
                    fjoraret: bevegelse.inngaende,
                    i_ar: bevegelse.utgaende,
                },
                utbytte,
                bevegelser,
            },
        ));
    }

    // "Antall aksjer etter stiftelse/nyemisjon" er totalen PÅ HENDELSENS
    // DATO, ikke ved årets slutt. Med én emisjon er det samme tall; med
    // to ville årsslutt-totalen vært feil for den første.
    nyutstedelser.sort_by_key(|n| n.dato);
    for n in &mut nyutstedelser {
        n.antall_etter = aksjeeierbok(pool, company_id, n.dato)
            .await?
            .iter()
            .map(|a| a.antall_aksjer)
            .sum();
    }

    let aksjekapital = oppgave::Fjoraret {
        fjoraret: antall_i_fjor * palydende_ore,
        i_ar: antall_i_ar * palydende_ore,
    };

    Ok(Oppgavesett {
        hovedskjema: oppgave::Hovedskjema {
            selskap: oppgave::Selskap {
                orgnr,
                navn: selskap.get("name"),
                adresse: gate,
                postnummer: postnr,
                poststed,
                kontakt_navn: None,
                kontakt_epost: selskap.get("email"),
            },
            inntektsar: ar,
            aksjekapital,
            palydende_ore: oppgave::Fjoraret {
                fjoraret: palydende_ore,
                i_ar: palydende_ore,
            },
            antall_aksjer: oppgave::Fjoraret {
                fjoraret: antall_i_fjor,
                i_ar: antall_i_ar,
            },
            innbetalt_aksjekapital: aksjekapital,
            overkurs: oppgave::Fjoraret::default(),
            utbytte: utbytte_rader
                .iter()
                .map(|u| oppgave::Utdeling {
                    dato: u.besluttet_dato,
                    per_aksje_ore: u.per_aksje_ore,
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
async fn aksjonaerid(pool: &PgPool, a: &Aksjonaer) -> Result<Option<oppgave::Aksjonaerid>> {
    if let Some(orgnr) = &a.orgnr {
        return Ok(Some(oppgave::Aksjonaerid::Organisasjonsnummer(
            orgnr.clone(),
        )));
    }
    if let Some(utl) = &a.utenlandsk_id {
        return Ok(Some(oppgave::Aksjonaerid::Utenlandsk(utl.clone())));
    }
    let fnr: Option<String> =
        sqlx::query_scalar("select fodselsnummer from shareholder where id = $1")
            .bind(a.id)
            .fetch_one(pool)
            .await?;
    Ok(fnr.map(oppgave::Aksjonaerid::Fodselsnummer))
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
            let antall: i64 = r.get("antall");
            let belop: i64 = r.get("belop_ore");
            if antall > 0 { belop / antall } else { 0 }
        }
        None => 0,
    })
}
