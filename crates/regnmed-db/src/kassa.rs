//! Kassaoppgjør (#89, docs/kontantsalg.md): posting the day's settlement.
//!
//! The arrangement is pure (`regnmed_core::kassa`); this module looks up
//! the rate that applied on the day, posts, and attaches the Z-report.
//!
//! ONE transaction covers the settlement, the kassadifferanse and the
//! attachment. A day whose difference voucher failed after the
//! settlement posted would leave a till that looks reconciled and is
//! not.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::kassa::{Betalingslinje, Dagsoppgjor, Salgslinje, bygg_differanse, bygg_oppgjor};
use sqlx::PgPool;
use uuid::Uuid;

/// What the caller sends: gross per income account and VAT code, the
/// payment means, and optionally what was actually counted in the till.
#[derive(Debug)]
pub struct DagsoppgjorInn {
    pub dato: NaiveDate,
    pub z_nummer: String,
    pub salg: Vec<(String, Option<String>, i64)>,
    pub betaling: Vec<(String, i64)>,
    pub mva_konto: String,
    /// The cash account, and what was counted in it. Both or neither —
    /// a counted amount without an account cannot be posted anywhere.
    pub kontantkonto: Option<String>,
    pub opptalt_kontant_ore: Option<i64>,
    pub differansekonto: String,
}

#[derive(Debug)]
pub struct BokfortOppgjor {
    pub voucher: (i32, i64),
    /// The difference voucher, when there was a difference at all.
    pub differanse: Option<(i32, i64)>,
    pub differanse_ore: i64,
}

pub async fn bokfor_dagsoppgjor(
    pool: &PgPool,
    company_id: Uuid,
    inn: &DagsoppgjorInn,
    z_rapport: Option<(&str, &str, &[u8])>,
    created_by: &str,
) -> Result<BokfortOppgjor> {
    ensure!(
        inn.kontantkonto.is_some() == inn.opptalt_kontant_ore.is_some(),
        "opptalt kontantbeholdning krever at kontantkontoen oppgis, og omvendt"
    );

    // The rate is the one that applied on the settlement date, from the
    // same dated table the invoice engine and the spesifikasjon read.
    let mut salg = Vec::with_capacity(inn.salg.len());
    for (konto, vat_code, brutto_ore) in &inn.salg {
        let rate_bp = match vat_code {
            None => 0,
            Some(code) => sqlx::query_scalar::<_, i32>(
                "select r.rate_bp from vat_code c
                 join vat_rate r on r.rate_class = c.rate_class
                 where c.code = $1 and r.valid_from <= $2
                 order by r.valid_from desc limit 1",
            )
            .bind(code)
            .bind(inn.dato)
            .fetch_optional(pool)
            .await?
            .map(i64::from)
            .with_context(|| format!("ingen mva-sats for kode {code} per {} ", inn.dato))?,
        };
        salg.push(Salgslinje {
            konto: konto.clone(),
            vat_code: vat_code.clone(),
            rate_bp,
            brutto_ore: *brutto_ore,
        });
    }

    let oppgjor = Dagsoppgjor {
        dato: inn.dato,
        z_nummer: inn.z_nummer.clone(),
        salg,
        betaling: inn
            .betaling
            .iter()
            .map(|(konto, belop_ore)| Betalingslinje {
                konto: konto.clone(),
                belop_ore: *belop_ore,
            })
            .collect(),
        mva_konto: inn.mva_konto.clone(),
    };
    let draft = bygg_oppgjor(&oppgjor).map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut tx = pool.begin().await?;
    let posted = crate::post_voucher_in(&mut tx, company_id, &draft, created_by).await?;

    // The Z-report is the documentation of the settlement (§5-4), so it
    // hangs on the settlement's own bilag.
    if let Some((navn, content_type, bytes)) = z_rapport {
        crate::add_attachment_in(
            &mut tx,
            company_id,
            posted.id,
            navn,
            content_type,
            bytes,
            created_by,
        )
        .await?;
    }

    let mut differanse = None;
    let mut differanse_ore = 0;
    if let (Some(kontantkonto), Some(opptalt)) = (&inn.kontantkonto, inn.opptalt_kontant_ore) {
        let registrert: i64 = inn
            .betaling
            .iter()
            .filter(|(konto, _)| konto == kontantkonto)
            .map(|(_, belop)| *belop)
            .sum();
        differanse_ore = opptalt - registrert;
        if let Some(diff) = bygg_differanse(
            inn.dato,
            &inn.z_nummer,
            kontantkonto,
            &inn.differansekonto,
            registrert,
            opptalt,
        ) {
            let d = crate::post_voucher_in(&mut tx, company_id, &diff, created_by).await?;
            differanse = Some((d.fiscal_year, d.voucher_number));
        }
    }
    tx.commit().await?;
    Ok(BokfortOppgjor {
        voucher: (posted.fiscal_year, posted.voucher_number),
        differanse,
        differanse_ore,
    })
}
