//! EHF outbound and inbound (docs/ehf.md, #14).
//!
//! **Outbound:** the document is built from the issued faktura's own,
//! locked rows and rendered on request. It is deliberately not stored as
//! an attachment the way the PDF is: the PDF IS the sales document
//! (bokføringsforskriften §5-1, retention from the moment of issue),
//! whereas the EHF is a transport envelope derived from the same
//! immutable numbers. When an access point actually sends it, it is the
//! sending that must be logged — the utsendelse pattern from #32.
//!
//! **Inbound:** a received EHF is stored as-is in the bilagsinnboks
//! (immutable from arrival, hash-checked — #21), and the posting
//! suggestion is computed from the original every time it is asked for.
//! Nothing derived is stored, so an improved suggestion applies to
//! documents already sitting there as well.

use anyhow::{Context, Result};
use regnmed_core::ehf::{Dokumenttype, EhfDokument, EhfLinje, EhfPart};
use regnmed_core::ehf_import::{MottattFaktura, parse};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Splits "Storgata 1, 0155 Oslo" into street / postnr / poststed.
/// Addresses are free text in our master data; EHF wants them apart.
/// A shape we do not recognize goes out whole as the street line —
/// better a complete address in one field than a wrong split.
pub(crate) fn split_adresse(
    adresse: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(adresse) = adresse
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
    else {
        return (None, None, None);
    };
    let Some((gate, resten)) = adresse.rsplit_once(',') else {
        return (Some(adresse), None, None);
    };
    let resten = resten.trim();
    let (postnr, poststed) = match resten.split_once(' ') {
        Some((nr, sted)) if nr.len() == 4 && nr.chars().all(|c| c.is_ascii_digit()) => {
            (Some(nr.to_string()), Some(sted.trim().to_string()))
        }
        _ => return (Some(adresse), None, None),
    };
    (Some(gate.trim().to_string()), postnr, poststed)
}

/// Builds the EHF document for an issued invoice from its stored rows.
pub async fn invoice_ehf(pool: &PgPool, company_id: Uuid, invoice_id: Uuid) -> Result<String> {
    let head = sqlx::query(
        "select i.invoice_no, i.invoice_date, i.due_date, i.kid, i.valuta,
                i.credits_invoice_id, i.delivery_date, i.delivery_place,
                (select k.invoice_no from invoice k where k.id = i.credits_invoice_id)
                    as krediterer_nr,
                c.name as selger_navn, c.orgnr as selger_orgnr, c.address as selger_adresse,
                c.bank_account, c.email as selger_epost,
                p.name as kjoper_navn, p.orgnr as kjoper_orgnr, p.address as kjoper_adresse,
                p.email as kjoper_epost, p.party_no
         from invoice i
         join company c on c.id = i.company_id
         join party p on p.id = i.party_id
         where i.id = $1 and i.company_id = $2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice")?;

    let lines = sqlx::query(
        "select l.description, l.quantity_milli, l.unit_price_ore, l.net_ore, l.vat_ore,
                l.vat_code
         from invoice_line l where l.invoice_id = $1 order by l.line_no",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;

    // The rate that applied on the invoice date — the same dated lookup
    // the posting used, so the document cannot drift from the ledger.
    let invoice_date: chrono::NaiveDate = head.get("invoice_date");
    let rates = crate::mva::load_vat_rates(pool).await?;
    let mut linjer = Vec::with_capacity(lines.len());
    for line in &lines {
        let vat_code: Option<String> = line.get("vat_code");
        let mva_sats_bp = match &vat_code {
            Some(code) => {
                let rate_class: Option<String> =
                    sqlx::query_scalar("select rate_class from vat_code where code = $1")
                        .bind(code)
                        .fetch_optional(pool)
                        .await?;
                rate_class
                    .and_then(|class| regnmed_core::mva::rate_on(&rates, &class, invoice_date))
            }
            None => None,
        };
        linjer.push(EhfLinje {
            beskrivelse: line.get("description"),
            antall_milli: line.get("quantity_milli"),
            // Our quantities are unitless counts; EN 16931 wants a UN/ECE
            // Rec 20 code, and "EA" (each) is the honest default.
            enhet: "EA".into(),
            enhetspris_ore: line.get("unit_price_ore"),
            netto_ore: line.get("net_ore"),
            mva_sats_bp,
            mva_ore: line.get("vat_ore"),
        });
    }

    let (selger_gate, selger_postnr, selger_poststed) = split_adresse(head.get("selger_adresse"));
    let (kjoper_gate, kjoper_postnr, kjoper_poststed) = split_adresse(head.get("kjoper_adresse"));
    let krediterer_nr: Option<i64> = head.get("krediterer_nr");
    let reg = crate::settings::registrering_on(pool, company_id, invoice_date).await?;

    let doc = EhfDokument {
        dokumenttype: if head.get::<Option<Uuid>, _>("credits_invoice_id").is_some() {
            Dokumenttype::Kreditnota
        } else {
            Dokumenttype::Faktura
        },
        fakturanr: head.get::<i64, _>("invoice_no").to_string(),
        krediterer: krediterer_nr.map(|no| no.to_string()),
        fakturadato: invoice_date,
        forfallsdato: Some(head.get("due_date")),
        valuta: head
            .get::<Option<String>, _>("valuta")
            .unwrap_or_else(|| "NOK".into()),
        // BT-10 is the buyer's own reference; we send the customer
        // number we know them by until an order reference exists.
        kjopers_referanse: head.get::<Option<String>, _>("party_no"),
        // BT-72. Null only on invoices issued before #81 — the element
        // is then omitted rather than filled with a guess.
        leveringsdato: head.get("delivery_date"),
        leveringssted: head.get("delivery_place"),
        selger: EhfPart {
            navn: head.get("selger_navn"),
            orgnr: head.get("selger_orgnr"),
            adresse: selger_gate,
            postnr: selger_postnr,
            poststed: selger_poststed,
            land: "NO".into(),
            // Registreringsstatus på fakturadatoen, ikke «bar dette
            // dokumentet mva» — en eksportfaktura fra en registrert
            // selger skal fortsatt ha PartyTaxScheme (#81).
            mva_registrert: reg.mva_registrert,
            epost: head.get("selger_epost"),
        },
        kjoper: EhfPart {
            navn: head.get("kjoper_navn"),
            // Without an orgnr we cannot address the receiver in Peppol;
            // the endpoint says so rather than shipping a blank id.
            orgnr: head
                .get::<Option<String>, _>("kjoper_orgnr")
                .context("kjøperen mangler organisasjonsnummer — EHF krever mottakerens orgnr")?,
            adresse: kjoper_gate,
            postnr: kjoper_postnr,
            poststed: kjoper_poststed,
            land: "NO".into(),
            mva_registrert: false,
            epost: head.get("kjoper_epost"),
        },
        kontonummer: head.get("bank_account"),
        kid: head.get::<Option<String>, _>("kid"),
        linjer,
    };
    Ok(regnmed_core::ehf::render(&doc))
}

#[derive(Debug)]
pub struct EhfForslag {
    pub mottatt: MottattFaktura,
    /// The supplier in our register, matched on orgnr — None means the
    /// import would have to create one.
    pub leverandor_no: Option<String>,
    pub leverandor_navn: Option<String>,
    pub advarsler: Vec<String>,
}

/// Reads a stored inbox document as EHF and derives the posting
/// suggestion. Nothing is written — the human posts it through the
/// ordinary innboks flow (with attestering, when the policy is on).
pub async fn inbox_ehf_forslag(
    pool: &PgPool,
    company_id: Uuid,
    document_id: Uuid,
) -> Result<EhfForslag> {
    let (_, _, content) = crate::innboks::get_inbox_document(pool, company_id, document_id).await?;
    let xml = String::from_utf8(content).context("dokumentet er ikke tekst — er det en EHF?")?;
    let mottatt = parse(&xml).map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut advarsler = Vec::new();
    let (leverandor_no, leverandor_navn) = match &mottatt.selger_orgnr {
        Some(orgnr) => {
            let row = sqlx::query(
                "select party_no, name from party
                 where company_id = $1 and kind = 'leverandor' and orgnr = $2 limit 1",
            )
            .bind(company_id)
            .bind(orgnr)
            .fetch_optional(pool)
            .await?;
            match row {
                Some(r) => (
                    Some(r.get::<String, _>("party_no")),
                    Some(r.get::<String, _>("name")),
                ),
                None => {
                    advarsler.push(format!(
                        "leverandøren {} (orgnr {orgnr}) finnes ikke i reskontroen ennå",
                        mottatt.selger_navn
                    ));
                    (None, None)
                }
            }
        }
        None => {
            advarsler.push("dokumentet oppgir ikke avsenderens organisasjonsnummer".into());
            (None, None)
        }
    };
    if mottatt.netto_ore + mottatt.mva_ore != mottatt.brutto_ore {
        advarsler.push(format!(
            "netto + mva ({}) stemmer ikke med forfalt beløp ({}) — kontroller dokumentet",
            mottatt.netto_ore + mottatt.mva_ore,
            mottatt.brutto_ore
        ));
    }
    if mottatt.valuta != "NOK" {
        advarsler.push(format!(
            "fakturaen er i {} — bokfør med valuta (docs/valuta.md)",
            mottatt.valuta
        ));
    }
    Ok(EhfForslag {
        mottatt,
        leverandor_no,
        leverandor_navn,
        advarsler,
    })
}

/// The general suggestion for ANY inbox document (docs/bilagstolkning.md,
/// #34). EHF is the exact case; a PDF with a text layer is the
/// heuristic one; a scan without text yields nothing at all — and says
/// so, rather than inventing numbers.
#[derive(Debug)]
pub struct Bokforingsforslag {
    /// "ehf" | "pdf-tekst" | "tekst" | "ingen"
    pub kilde: &'static str,
    pub selger_navn: Option<String>,
    pub orgnr: Option<String>,
    pub fakturanr: Option<String>,
    pub dato: Option<chrono::NaiveDate>,
    pub forfall: Option<chrono::NaiveDate>,
    pub kid: Option<String>,
    pub kontonummer: Option<String>,
    pub netto_ore: Option<i64>,
    pub mva_ore: Option<i64>,
    pub brutto_ore: Option<i64>,
    pub leverandor_no: Option<String>,
    pub leverandor_navn: Option<String>,
    /// The account this supplier was posted to last time — the single
    /// most useful suggestion we can make, and it comes from the
    /// company's own history, not from a model.
    pub konto: Option<String>,
    pub konto_begrunnelse: Option<String>,
    /// Per-field provenance, shown next to the value in the UI.
    pub begrunnelser: Vec<(String, String)>,
    pub advarsler: Vec<String>,
}

pub async fn inbox_forslag(
    pool: &PgPool,
    company_id: Uuid,
    document_id: Uuid,
) -> Result<Bokforingsforslag> {
    let (_, content_type, content) =
        crate::innboks::get_inbox_document(pool, company_id, document_id).await?;

    let mut forslag = Bokforingsforslag {
        kilde: "ingen",
        selger_navn: None,
        orgnr: None,
        fakturanr: None,
        dato: None,
        forfall: None,
        kid: None,
        kontonummer: None,
        netto_ore: None,
        mva_ore: None,
        brutto_ore: None,
        leverandor_no: None,
        leverandor_navn: None,
        konto: None,
        konto_begrunnelse: None,
        begrunnelser: Vec::new(),
        advarsler: Vec::new(),
    };

    // 1. EHF — the structured case, where nothing is guessed.
    let text = String::from_utf8(content.clone()).ok();
    if let Some(xml) = text
        .as_deref()
        .filter(|t| t.contains("<Invoice") || t.contains("<CreditNote"))
        && let Ok(m) = regnmed_core::ehf_import::parse(xml)
    {
        forslag.kilde = "ehf";
        forslag.selger_navn = Some(m.selger_navn.clone());
        forslag.orgnr = m.selger_orgnr.clone();
        forslag.fakturanr = Some(m.fakturanr.clone());
        forslag.dato = m.fakturadato;
        forslag.forfall = m.forfallsdato;
        forslag.kid = m.kid.clone();
        forslag.kontonummer = m.kontonummer.clone();
        forslag.netto_ore = Some(m.netto_ore);
        forslag.mva_ore = Some(m.mva_ore);
        forslag.brutto_ore = Some(m.brutto_ore);
        forslag
            .begrunnelser
            .push(("alle".into(), "lest direkte fra EHF-dokumentet".into()));
        if m.valuta != "NOK" {
            forslag
                .advarsler
                .push(format!("fakturaen er i {} — bokfør med valuta", m.valuta));
        }
    } else {
        // 2. A PDF text layer, or a plain-text document.
        let tekst = if content_type.contains("pdf") || content.starts_with(b"%PDF") {
            regnmed_core::pdftekst::extract(&content).inspect(|_| forslag.kilde = "pdf-tekst")
        } else {
            text.filter(|t| t.len() > 20)
                .inspect(|_| forslag.kilde = "tekst")
        };
        match tekst {
            None => {
                forslag.advarsler.push(
                    "fant ingen lesbar tekst i dokumentet — et skannet bilde må bokføres manuelt \
                     (OCR er ikke en del av kjernen)"
                        .into(),
                );
                return Ok(forslag);
            }
            Some(tekst) => {
                let t = regnmed_core::bilagstolk::tolk(&tekst);
                let mut note = |felt: &str, begrunnelse: &str| {
                    forslag
                        .begrunnelser
                        .push((felt.to_string(), begrunnelse.to_string()));
                };
                if let Some(f) = &t.orgnr {
                    forslag.orgnr = Some(f.verdi.clone());
                    note("orgnr", &f.begrunnelse);
                }
                if let Some(f) = &t.fakturanr {
                    forslag.fakturanr = Some(f.verdi.clone());
                    note("fakturanr", &f.begrunnelse);
                }
                if let Some(f) = &t.kid {
                    forslag.kid = Some(f.verdi.clone());
                    note("kid", &f.begrunnelse);
                }
                if let Some(f) = &t.kontonummer {
                    forslag.kontonummer = Some(f.verdi.clone());
                    note("kontonummer", &f.begrunnelse);
                }
                if let Some(f) = &t.dato {
                    forslag.dato = Some(f.verdi);
                    note("dato", &f.begrunnelse);
                }
                if let Some(f) = &t.forfall {
                    forslag.forfall = Some(f.verdi);
                    note("forfall", &f.begrunnelse);
                }
                if let Some(f) = &t.belop_ore {
                    forslag.brutto_ore = Some(f.verdi);
                    note("brutto", &f.begrunnelse);
                }
                if let Some(f) = &t.mva_ore {
                    forslag.mva_ore = Some(f.verdi);
                    note("mva", &f.begrunnelse);
                }
                if let (Some(brutto), Some(mva)) = (forslag.brutto_ore, forslag.mva_ore) {
                    forslag.netto_ore = Some(brutto - mva);
                }
            }
        }
    }

    // 3. The supplier, and what we posted for them last time.
    if let Some(orgnr) = &forslag.orgnr {
        let row = sqlx::query(
            "select party_no, name from party
             where company_id = $1 and kind = 'leverandor' and orgnr = $2 limit 1",
        )
        .bind(company_id)
        .bind(orgnr)
        .fetch_optional(pool)
        .await?;
        match row {
            Some(r) => {
                let party_no: String = r.get("party_no");
                forslag.leverandor_navn = Some(r.get("name"));
                if let Some((konto, dato)) =
                    siste_kostnadskonto(pool, company_id, &party_no).await?
                {
                    forslag.konto_begrunnelse = Some(format!(
                        "samme leverandør ble sist bokført på {konto} ({dato})"
                    ));
                    forslag.konto = Some(konto);
                }
                forslag.leverandor_no = Some(party_no);
            }
            None => forslag.advarsler.push(format!(
                "orgnr {orgnr} finnes ikke i leverandørreskontroen — opprett parten først"
            )),
        }
    }
    Ok(forslag)
}

/// The cost account most recently used on a voucher that also touched
/// this supplier. Pure query over the company's own history.
async fn siste_kostnadskonto(
    pool: &PgPool,
    company_id: Uuid,
    party_no: &str,
) -> Result<Option<(String, chrono::NaiveDate)>> {
    let row = sqlx::query(
        "select a.number, v.voucher_date
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         where v.company_id = $1
           and a.number >= '4000'
           and e.amount_ore > 0
           and exists (
               select 1 from entry pe join party p on p.id = pe.party_id
               where pe.voucher_id = v.id and p.party_no = $2 and p.company_id = $1
                 and p.kind = 'leverandor'
           )
         order by v.voucher_date desc, v.voucher_number desc
         limit 1",
    )
    .bind(company_id)
    .bind(party_no)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("number"), r.get("voucher_date"))))
}

#[cfg(test)]
mod tests {
    use super::split_adresse;

    #[test]
    fn the_address_is_split_when_its_shape_is_recognisable() {
        assert_eq!(
            split_adresse(Some("Storgata 1, 0155 Oslo".into())),
            (
                Some("Storgata 1".into()),
                Some("0155".into()),
                Some("Oslo".into())
            )
        );
        // Unknown shape: the whole address goes out as the street line, never misfiled.
        assert_eq!(
            split_adresse(Some("Postboks 42 Sentrum".into())),
            (Some("Postboks 42 Sentrum".into()), None, None)
        );
        assert_eq!(
            split_adresse(Some("Storgata 1, Oslo".into())),
            (Some("Storgata 1, Oslo".into()), None, None)
        );
        assert_eq!(split_adresse(None), (None, None, None));
    }
}
