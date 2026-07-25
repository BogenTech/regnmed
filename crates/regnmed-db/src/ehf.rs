//! EHF ut og inn (docs/ehf.md, #14).
//!
//! **Ut:** dokumentet bygges fra den utstedte fakturaens egne, låste
//! rader og rendres på forespørsel. Det er med vilje ikke lagret som
//! vedlegg slik PDF-en er: PDF-en ER salgsdokumentet (bokførings-
//! forskriften §5-1, oppbevaringsplikt fra utstedelsen), mens EHF-en
//! er en transportkonvolutt utledet av de samme uforanderlige tallene.
//! Når et aksesspunkt faktisk sender den, er det sendingen som skal
//! logges — utsendelsesmønsteret fra #32.
//!
//! **Inn:** en mottatt EHF lagres som den er i bilagsinnboksen
//! (uforanderlig fra ankomst, hash-sjekket — #21), og
//! bokføringsforslaget regnes ut av originalen hver gang det spørres.
//! Ingenting utledet lagres, så et forbedret forslag gjelder også for
//! dokumenter som allerede ligger der.

use anyhow::{Context, Result};
use regnmed_core::ehf::{Dokumenttype, EhfDokument, EhfLinje, EhfPart};
use regnmed_core::ehf_import::{MottattFaktura, parse};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Splits "Storgata 1, 0155 Oslo" into street / postnr / poststed.
/// Addresses are free text in our master data; EHF wants them apart.
/// A shape we do not recognize goes out whole as the street line —
/// better a complete address in one field than a wrong split.
fn split_adresse(adresse: Option<String>) -> (Option<String>, Option<String>, Option<String>) {
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
                i.credits_invoice_id,
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
    let mva_sum: i64 = linjer.iter().map(|l| l.mva_ore).sum();

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
        selger: EhfPart {
            navn: head.get("selger_navn"),
            orgnr: head.get("selger_orgnr"),
            adresse: selger_gate,
            postnr: selger_postnr,
            poststed: selger_poststed,
            land: "NO".into(),
            mva_registrert: mva_sum != 0,
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

#[cfg(test)]
mod tests {
    use super::split_adresse;

    #[test]
    fn adressen_deles_naar_formen_er_gjenkjennelig() {
        assert_eq!(
            split_adresse(Some("Storgata 1, 0155 Oslo".into())),
            (
                Some("Storgata 1".into()),
                Some("0155".into()),
                Some("Oslo".into())
            )
        );
        // Ukjent form: hele adressen går ut som gatelinje, ikke feilfordelt.
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
