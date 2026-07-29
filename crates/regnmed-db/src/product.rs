//! Product register and simple inventory (docs/produkter.md, #39).
//!
//! The register is editable master data; document lines COPY the values
//! at issue time (resolve_product_line), so issued documents never
//! change when the register does. Beholdning and verdi are pure
//! computations over the insert-only movement log
//! (regnmed-core::lager, gjennomsnittsmetoden) — never stored.
//!
//! Salg movements are inserted by the invoice path itself
//! (record_sales_in, called inside create_invoice_in) so stock and
//! ledger commit or roll back together. Varetelling inserts the
//! justeringer AND posts the value adjustment against the booked lager
//! saldo in one transaction.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::lager::{Bevegelse, LagerStatus, verdsett};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::invoice::InvoiceLineDraft;
use crate::ledger::post_voucher_in;

#[derive(Debug, Clone)]
pub struct ProductDraft {
    pub nummer: String,
    pub navn: String,
    pub salgspris_ore: i64,
    pub vat_code: Option<String>,
    pub konto: String,
    pub lagerfort: bool,
}

pub async fn create_product(pool: &PgPool, company_id: Uuid, draft: &ProductDraft) -> Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into product (id, company_id, nummer, navn, salgspris_ore, vat_code,
                              konto, lagerfort)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&draft.nummer)
    .bind(&draft.navn)
    .bind(draft.salgspris_ore)
    .bind(&draft.vat_code)
    .bind(&draft.konto)
    .bind(draft.lagerfort)
    .execute(pool)
    .await
    .with_context(|| {
        format!(
            "could not create product {} (duplicate nummer?)",
            draft.nummer
        )
    })?;
    Ok(id)
}

/// Everything except the nummer is editable — issued documents carry
/// their own copies, so register edits are always safe.
#[allow(clippy::too_many_arguments)]
pub async fn update_product(
    pool: &PgPool,
    company_id: Uuid,
    nummer: &str,
    navn: Option<&str>,
    salgspris_ore: Option<i64>,
    vat_code: Option<Option<&str>>,
    konto: Option<&str>,
    aktiv: Option<bool>,
    lagerfort: Option<bool>,
) -> Result<()> {
    let updated = sqlx::query(
        "update product set
             navn = coalesce($3, navn),
             salgspris_ore = coalesce($4, salgspris_ore),
             vat_code = case when $5 then $6 else vat_code end,
             konto = coalesce($7, konto),
             aktiv = coalesce($8, aktiv),
             lagerfort = coalesce($9, lagerfort),
             updated_at = now()
         where company_id = $1 and nummer = $2",
    )
    .bind(company_id)
    .bind(nummer)
    .bind(navn)
    .bind(salgspris_ore)
    .bind(vat_code.is_some())
    .bind(vat_code.flatten())
    .bind(konto)
    .bind(aktiv)
    .bind(lagerfort)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no product {nummer}");
    Ok(())
}

#[derive(Debug)]
pub struct ProductRow {
    pub nummer: String,
    pub navn: String,
    pub salgspris_ore: i64,
    pub vat_code: Option<String>,
    pub konto: String,
    pub aktiv: bool,
    pub lagerfort: bool,
}

pub async fn list_products(pool: &PgPool, company_id: Uuid) -> Result<Vec<ProductRow>> {
    let rows = sqlx::query(
        "select nummer, navn, salgspris_ore, vat_code, konto, aktiv, lagerfort
         from product where company_id = $1 order by nummer",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ProductRow {
            nummer: r.get("nummer"),
            navn: r.get("navn"),
            salgspris_ore: r.get("salgspris_ore"),
            vat_code: r.get("vat_code"),
            konto: r.get("konto"),
            aktiv: r.get("aktiv"),
            lagerfort: r.get("lagerfort"),
        })
        .collect())
}

/// One document line as the caller specified it: either free text
/// (description + price required) or a product reference whose register
/// values fill everything the caller did not override. The result is a
/// full COPY — the draft carries the product id only for lager and
/// traceability.
#[derive(Debug, Default)]
pub struct ProductLineSpec {
    pub produkt: Option<String>,
    pub description: Option<String>,
    pub account: Option<String>,
    pub quantity_milli: i64,
    pub unit_price_ore: Option<i64>,
    pub vat_code: Option<String>,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

pub async fn resolve_product_line(
    pool: &PgPool,
    company_id: Uuid,
    spec: ProductLineSpec,
) -> Result<InvoiceLineDraft> {
    match &spec.produkt {
        Some(nummer) => {
            let product = sqlx::query(
                "select id, navn, salgspris_ore, vat_code, konto, aktiv
                 from product where company_id = $1 and nummer = $2",
            )
            .bind(company_id)
            .bind(nummer)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no product {nummer}"))?;
            ensure!(
                product.get::<bool, _>("aktiv"),
                "product {nummer} is deactivated"
            );
            Ok(InvoiceLineDraft {
                description: spec
                    .description
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| product.get("navn")),
                account_number: spec.account.unwrap_or_else(|| product.get("konto")),
                quantity_milli: spec.quantity_milli,
                unit_price_ore: spec
                    .unit_price_ore
                    .unwrap_or_else(|| product.get("salgspris_ore")),
                vat_code: spec.vat_code.or_else(|| product.get("vat_code")),
                avdeling: spec.avdeling,
                prosjekt: spec.prosjekt,
                product_id: Some(product.get("id")),
            })
        }
        None => {
            let description = spec
                .description
                .filter(|d| !d.is_empty())
                .context("a line without a product needs a description")?;
            let unit_price_ore = spec
                .unit_price_ore
                .context("a line without a product needs a unit price")?;
            Ok(InvoiceLineDraft {
                description,
                account_number: spec.account.unwrap_or_else(|| "3000".into()),
                quantity_milli: spec.quantity_milli,
                unit_price_ore,
                vat_code: spec.vat_code,
                avdeling: spec.avdeling,
                prosjekt: spec.prosjekt,
                product_id: None,
            })
        }
    }
}

/// Called inside create_invoice_in: every line referencing a LAGERFØRT
/// product gets a salg movement mirroring the line quantity (negated —
/// a kreditnota line has negative quantity, so its movement returns the
/// stock). Same transaction as the posting: stock never drifts from
/// the ledger's view of what was sold.
pub(crate) async fn record_sales_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    invoice_id: Uuid,
    invoice_date: NaiveDate,
    lines: &[InvoiceLineDraft],
    created_by: &str,
) -> Result<()> {
    for line in lines {
        let Some(product_id) = line.product_id else {
            continue;
        };
        if line.quantity_milli == 0 {
            continue;
        }
        let lagerfort: bool =
            sqlx::query_scalar("select lagerfort from product where id = $1 and company_id = $2")
                .bind(product_id)
                .bind(company_id)
                .fetch_optional(&mut **tx)
                .await?
                .context("line references a product from another company")?;
        if !lagerfort {
            continue;
        }
        sqlx::query(
            "insert into inventory_movement
                 (id, company_id, product_id, dato, kind, antall_milli, invoice_id, created_by)
             values ($1, $2, $3, $4, 'salg', $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(company_id)
        .bind(product_id)
        .bind(invoice_date)
        .bind(-line.quantity_milli)
        .bind(invoice_id)
        .bind(created_by)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Manual movements: varekjøp (positive, anskaffelseskost per unit) and
/// justering (either sign, note required — the DB checks both rules
/// too). Salg only ever comes from invoicing.
#[allow(clippy::too_many_arguments)]
pub async fn register_movement(
    pool: &PgPool,
    company_id: Uuid,
    nummer: &str,
    dato: NaiveDate,
    kind: &str,
    antall_milli: i64,
    kostpris_ore: Option<i64>,
    note: Option<&str>,
    created_by: &str,
) -> Result<()> {
    ensure!(
        matches!(kind, "kjop" | "justering"),
        "kind must be kjop or justering (salg comes from invoicing)"
    );
    let product =
        sqlx::query("select id, lagerfort from product where company_id = $1 and nummer = $2")
            .bind(company_id)
            .bind(nummer)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no product {nummer}"))?;
    ensure!(
        product.get::<bool, _>("lagerfort"),
        "product {nummer} is not lagerført"
    );
    sqlx::query(
        "insert into inventory_movement
             (id, company_id, product_id, dato, kind, antall_milli, kostpris_ore, note, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(product.get::<Uuid, _>("id"))
    .bind(dato)
    .bind(kind)
    .bind(antall_milli)
    .bind(kostpris_ore)
    .bind(note)
    .bind(created_by)
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("could not register movement: {e}"))?;
    Ok(())
}

/// Movements in valuation order for one product. Runs on the pool or
/// inside a transaction (varetelling must see its own justeringer).
async fn product_status<'e, E>(executor: E, product_id: Uuid) -> Result<LagerStatus>
where
    E: sqlx::PgExecutor<'e>,
{
    let rows = sqlx::query(
        "select antall_milli, kostpris_ore from inventory_movement
         where product_id = $1 order by dato, created_at, id",
    )
    .bind(product_id)
    .fetch_all(executor)
    .await?;
    let bevegelser: Vec<Bevegelse> = rows
        .iter()
        .map(|r| Bevegelse {
            antall_milli: r.get("antall_milli"),
            kostpris_ore: r.get("kostpris_ore"),
        })
        .collect();
    Ok(verdsett(&bevegelser))
}

#[derive(Debug)]
pub struct InventoryRow {
    pub nummer: String,
    pub navn: String,
    pub antall_milli: i64,
    pub verdi_ore: i64,
    pub gjennomsnitt_ore: Option<i64>,
}

/// Lagerstatus per lagerført product — this is also the telleliste
/// (bokføringsforskriften §6-1: varetelling at year-end, valued at
/// anskaffelseskost, here the gjennomsnitt over the movement log).
pub async fn inventory_status(pool: &PgPool, company_id: Uuid) -> Result<Vec<InventoryRow>> {
    let products = sqlx::query(
        "select id, nummer, navn from product
         where company_id = $1 and lagerfort order by nummer",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut rows = Vec::with_capacity(products.len());
    for p in &products {
        let status = product_status(pool, p.get("id")).await?;
        rows.push(InventoryRow {
            nummer: p.get("nummer"),
            navn: p.get("navn"),
            antall_milli: status.antall_milli,
            verdi_ore: status.verdi_ore,
            gjennomsnitt_ore: status.gjennomsnitt_ore(),
        });
    }
    Ok(rows)
}

#[derive(Debug)]
pub struct MovementRow {
    pub dato: NaiveDate,
    pub kind: String,
    pub antall_milli: i64,
    pub kostpris_ore: Option<i64>,
    pub note: Option<String>,
    pub invoice_no: Option<i64>,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_movements(
    pool: &PgPool,
    company_id: Uuid,
    nummer: &str,
) -> Result<Vec<MovementRow>> {
    let rows = sqlx::query(
        "select m.dato, m.kind, m.antall_milli, m.kostpris_ore, m.note, i.invoice_no,
                m.created_by, m.created_at
         from inventory_movement m
         join product p on p.id = m.product_id
         left join invoice i on i.id = m.invoice_id
         where m.company_id = $1 and p.nummer = $2
         order by m.dato desc, m.created_at desc",
    )
    .bind(company_id)
    .bind(nummer)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| MovementRow {
            dato: r.get("dato"),
            kind: r.get("kind"),
            antall_milli: r.get("antall_milli"),
            kostpris_ore: r.get("kostpris_ore"),
            note: r.get("note"),
            invoice_no: r.get("invoice_no"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct TellingLinje {
    pub nummer: String,
    pub talt_milli: i64,
}

#[derive(Debug, Clone)]
pub struct TellingKonti {
    /// Defaults at the API layer: GL, 1460, 4390 (beholdningsendring).
    pub journal_code: String,
    pub lager_konto: String,
    pub endring_konto: String,
}

#[derive(Debug)]
pub struct TellingJustering {
    pub nummer: String,
    pub bokfort_milli: i64,
    pub talt_milli: i64,
}

#[derive(Debug)]
pub struct TellingResult {
    pub justeringer: Vec<TellingJustering>,
    /// Inventory value after the count (gjennomsnittsmetoden).
    pub verdi_ore: i64,
    /// Booked balance on the lager account before adjustment (only
    /// computed when posting was requested).
    pub bokfort_ore: Option<i64>,
    pub voucher: Option<(i32, i64)>,
}

/// Varetelling: counted quantities become justering movements (note
/// carries the date), and — when `post` is given — the difference
/// between the counted inventory VALUE and the booked lager saldo is
/// posted as an ordinary voucher (debit lager when the value grew,
/// credit beholdningsendring). Movements and voucher commit or roll
/// back together.
pub async fn varetelling(
    pool: &PgPool,
    company_id: Uuid,
    dato: NaiveDate,
    linjer: &[TellingLinje],
    post: Option<&TellingKonti>,
    created_by: &str,
) -> Result<TellingResult> {
    ensure!(!linjer.is_empty(), "varetellingen har ingen linjer");
    let mut tx = pool.begin().await?;
    let mut justeringer = Vec::new();
    for linje in linjer {
        let product = sqlx::query(
            "select id, lagerfort from product
             where company_id = $1 and nummer = $2 for update",
        )
        .bind(company_id)
        .bind(&linje.nummer)
        .fetch_optional(&mut *tx)
        .await?
        .with_context(|| format!("no product {}", linje.nummer))?;
        ensure!(
            product.get::<bool, _>("lagerfort"),
            "product {} is not lagerført",
            linje.nummer
        );
        let product_id: Uuid = product.get("id");
        let status = product_status(&mut *tx, product_id).await?;
        let diff = linje.talt_milli - status.antall_milli;
        if diff != 0 {
            sqlx::query(
                "insert into inventory_movement
                     (id, company_id, product_id, dato, kind, antall_milli, note, created_by)
                 values ($1, $2, $3, $4, 'justering', $5, $6, $7)",
            )
            .bind(Uuid::now_v7())
            .bind(company_id)
            .bind(product_id)
            .bind(dato)
            .bind(diff)
            .bind(format!("Varetelling {dato}"))
            .bind(created_by)
            .execute(&mut *tx)
            .await?;
        }
        justeringer.push(TellingJustering {
            nummer: linje.nummer.clone(),
            bokfort_milli: status.antall_milli,
            talt_milli: linje.talt_milli,
        });
    }

    // Total value AFTER the adjustments, over every lagerført product.
    let product_ids: Vec<Uuid> =
        sqlx::query_scalar("select id from product where company_id = $1 and lagerfort")
            .bind(company_id)
            .fetch_all(&mut *tx)
            .await?;
    let mut verdi_ore = 0i64;
    for id in product_ids {
        verdi_ore += product_status(&mut *tx, id).await?.verdi_ore;
    }

    let mut bokfort_ore = None;
    let mut voucher = None;
    if let Some(konti) = post {
        let bokfort: i64 = sqlx::query_scalar(
            "select coalesce(sum(e.amount_ore), 0)::bigint
             from entry e join account a on a.id = e.account_id
             where a.company_id = $1 and a.number = $2",
        )
        .bind(company_id)
        .bind(&konti.lager_konto)
        .fetch_one(&mut *tx)
        .await?;
        bokfort_ore = Some(bokfort);
        let endring = verdi_ore - bokfort;
        if endring != 0 {
            let draft = VoucherDraft {
                journal_code: konti.journal_code.clone(),
                voucher_date: dato,
                description: format!("Varetelling {dato}"),
                reverses: None,
                entries: vec![
                    EntryDraft {
                        account_number: konti.lager_konto.clone(),
                        amount: Ore(endring),
                        vat_code: None,
                        description: Some("Varelager etter telling".into()),
                        party_no: None,
                        avdeling: None,
                        prosjekt: None,
                        valuta: None,
                    },
                    EntryDraft {
                        account_number: konti.endring_konto.clone(),
                        amount: Ore(-endring),
                        vat_code: None,
                        description: Some("Beholdningsendring".into()),
                        party_no: None,
                        avdeling: None,
                        prosjekt: None,
                        valuta: None,
                    },
                ],
            };
            draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
            let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
            voucher = Some((posted.fiscal_year, posted.voucher_number));
        }
    }
    tx.commit().await?;
    Ok(TellingResult {
        justeringer,
        verdi_ore,
        bokfort_ore,
        voucher,
    })
}
