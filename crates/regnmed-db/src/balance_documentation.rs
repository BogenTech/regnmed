//! Balansedokumentasjon (#88, docs/balansedokumentasjon.md):
//! bokføringsloven §11 wants documentation of what each balance account
//! CONSISTS OF at period end, not merely that it balances.
//!
//! Insert-only, like the period lock and the attestation trail: a
//! correction is a new row and the newest one applies. An avstemming
//! that could be rewritten afterwards is not documentation.
//!
//! The booked saldo is STORED at the moment of avstemming. That is the
//! point rather than a redundancy: if the account has been posted to
//! since, the report says so. A snapshot that silently followed the
//! ledger would hide exactly the difference the avstemming exists to
//! catch.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::hash::sha256;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One balance account's standing for a period.
#[derive(Debug)]
pub struct BalanseLinje {
    pub konto: String,
    pub kontonavn: String,
    /// Booked saldo at period end, computed now.
    pub saldo_ore: i64,
    /// The newest avstemming for the period, when there is one.
    pub avstemt: Option<Avstemming>,
}

#[derive(Debug)]
pub struct Avstemming {
    pub id: Uuid,
    pub saldo_ore: i64,
    pub forklaring: String,
    pub avstemt_dato: NaiveDate,
    pub avstemt_av: String,
    pub har_vedlegg: bool,
    pub vedlegg_navn: Option<String>,
}

impl BalanseLinje {
    /// Documented, and for the saldo the account actually has. A
    /// difference is not "undocumented" — it is documented and then
    /// moved, which is a different thing to tell the reader.
    pub fn avvik_ore(&self) -> Option<i64> {
        self.avstemt
            .as_ref()
            .map(|a| self.saldo_ore - a.saldo_ore)
            .filter(|d| *d != 0)
    }
}

/// Every balance account (class 1–2) with a nonzero saldo at `periode`,
/// alongside the newest avstemming for that period.
///
/// Accounts that end the period at zero are left out: there is nothing
/// to document about a saldo of nothing, and listing them would bury the
/// accounts that matter under the ones that do not.
pub async fn balanse_status(
    pool: &PgPool,
    company_id: Uuid,
    periode: NaiveDate,
) -> Result<Vec<BalanseLinje>> {
    let rows = sqlx::query(
        "select a.number as konto, a.name as kontonavn,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $2), 0)::bigint
                    as saldo_ore
         from account a
         left join entry e on e.account_id = a.id
         left join voucher v on v.id = e.voucher_id
         where a.company_id = $1 and a.number ~ '^[12]'
         group by a.number, a.name
         having coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $2), 0) <> 0
         order by a.number",
    )
    .bind(company_id)
    .bind(periode)
    .fetch_all(pool)
    .await?;

    let mut linjer = Vec::with_capacity(rows.len());
    for row in &rows {
        let konto: String = row.get("konto");
        let avstemt = sqlx::query(
            "select d.id, d.saldo_ore, d.forklaring, d.avstemt_dato,
                    coalesce(p.name, p.oidc_sub) as avstemt_av,
                    d.vedlegg_sha256 is not null as har_vedlegg, d.vedlegg_navn
             from balanse_dokumentasjon d
             join person p on p.id = d.avstemt_av
             where d.company_id = $1 and d.periode = $2 and d.konto = $3
             order by d.created_at desc limit 1",
        )
        .bind(company_id)
        .bind(periode)
        .bind(&konto)
        .fetch_optional(pool)
        .await?
        .map(|r| Avstemming {
            id: r.get("id"),
            saldo_ore: r.get("saldo_ore"),
            forklaring: r.get("forklaring"),
            avstemt_dato: r.get("avstemt_dato"),
            avstemt_av: r.get("avstemt_av"),
            har_vedlegg: r.get("har_vedlegg"),
            vedlegg_navn: r.get("vedlegg_navn"),
        });
        linjer.push(BalanseLinje {
            konto,
            kontonavn: row.get("kontonavn"),
            saldo_ore: row.get("saldo_ore"),
            avstemt,
        });
    }
    Ok(linjer)
}

/// Records an avstemming. The saldo is read HERE rather than taken from
/// the caller: the documented figure must be the ledger's own, or the
/// whole control is a self-report.
#[allow(clippy::too_many_arguments)]
pub async fn avstem(
    pool: &PgPool,
    company_id: Uuid,
    konto: &str,
    periode: NaiveDate,
    forklaring: &str,
    vedlegg: Option<(&str, &str, &[u8])>,
    avstemt_av: Uuid,
    avstemt_dato: NaiveDate,
) -> Result<Uuid> {
    ensure!(
        !forklaring.trim().is_empty(),
        "avstemmingen må forklare hva saldoen består av"
    );
    let finnes: bool = sqlx::query_scalar(
        "select exists(select 1 from account where company_id = $1 and number = $2)",
    )
    .bind(company_id)
    .bind(konto)
    .fetch_one(pool)
    .await?;
    ensure!(finnes, "ukjent konto {konto}");
    ensure!(
        konto.starts_with('1') || konto.starts_with('2'),
        "{konto} er ikke en balansekonto — §11 gjelder balansepostene"
    );

    let saldo_ore: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e
         join account a on a.id = e.account_id
         join voucher v on v.id = e.voucher_id
         where a.company_id = $1 and a.number = $2 and v.voucher_date <= $3",
    )
    .bind(company_id)
    .bind(konto)
    .bind(periode)
    .fetch_one(pool)
    .await?;

    let (navn, type_, bytes, sha) = match vedlegg {
        Some((navn, type_, bytes)) => {
            ensure!(!bytes.is_empty(), "vedlegget er tomt");
            let sha = sha256(bytes).to_vec();
            (Some(navn), Some(type_), Some(bytes), Some(sha))
        }
        None => (None, None, None, None),
    };

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into balanse_dokumentasjon
             (id, company_id, konto, periode, saldo_ore, forklaring,
              vedlegg, vedlegg_navn, vedlegg_type, vedlegg_sha256,
              avstemt_av, avstemt_dato)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(id)
    .bind(company_id)
    .bind(konto)
    .bind(periode)
    .bind(saldo_ore)
    .bind(forklaring.trim())
    .bind(bytes)
    .bind(navn)
    .bind(type_)
    .bind(sha)
    .bind(avstemt_av)
    .bind(avstemt_dato)
    .execute(pool)
    .await?;
    Ok(id)
}

/// The stored vedlegg, with its hash re-verified — the same discipline
/// as bilagsvedleggene: what comes out is proven to be what went in.
pub async fn hent_vedlegg(
    pool: &PgPool,
    company_id: Uuid,
    id: Uuid,
) -> Result<(String, String, Vec<u8>)> {
    let row = sqlx::query(
        "select vedlegg, vedlegg_navn, vedlegg_type, vedlegg_sha256
         from balanse_dokumentasjon where id = $1 and company_id = $2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent avstemming")?;
    let bytes: Option<Vec<u8>> = row.get("vedlegg");
    let bytes = bytes.context("avstemmingen har ikke vedlegg")?;
    let lagret: Vec<u8> = row.get("vedlegg_sha256");
    ensure!(
        sha256(&bytes).to_vec() == lagret,
        "vedlegget er endret siden det ble lagret — hashen stemmer ikke"
    );
    Ok((
        row.get::<Option<String>, _>("vedlegg_navn")
            .unwrap_or_default(),
        row.get::<Option<String>, _>("vedlegg_type")
            .unwrap_or_else(|| "application/octet-stream".into()),
        bytes,
    ))
}

/// The full trail for one account and period, newest first — a
/// correction is a new row, and the reader can see both.
pub async fn historikk(
    pool: &PgPool,
    company_id: Uuid,
    konto: &str,
    periode: NaiveDate,
) -> Result<Vec<Avstemming>> {
    Ok(sqlx::query(
        "select d.id, d.saldo_ore, d.forklaring, d.avstemt_dato,
                coalesce(p.name, p.oidc_sub) as avstemt_av,
                d.vedlegg_sha256 is not null as har_vedlegg, d.vedlegg_navn
         from balanse_dokumentasjon d
         join person p on p.id = d.avstemt_av
         where d.company_id = $1 and d.konto = $2 and d.periode = $3
         order by d.created_at desc",
    )
    .bind(company_id)
    .bind(konto)
    .bind(periode)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| Avstemming {
        id: r.get("id"),
        saldo_ore: r.get("saldo_ore"),
        forklaring: r.get("forklaring"),
        avstemt_dato: r.get("avstemt_dato"),
        avstemt_av: r.get("avstemt_av"),
        har_vedlegg: r.get("har_vedlegg"),
        vedlegg_navn: r.get("vedlegg_navn"),
    })
    .collect())
}
