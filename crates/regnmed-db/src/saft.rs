//! Loads the input for a SAF-T Financial export from the database.
//!
//! Reads only — the export is a projection of the ledger, and rendering is
//! pure (`regnmed_core::saft::render`).

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::saft::{
    SaftAccount, SaftAnalysisType, SaftInput, SaftJournal, SaftLine, SaftParty, SaftTaxCode,
    SaftTransaction,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Assembles everything `regnmed_core::saft::render` needs for one company
/// and date range. The contact person is required by the Norwegian SAF-T
/// header and is not master data we hold, so the caller provides it.
pub async fn load_saft_input(
    pool: &PgPool,
    company_id: Uuid,
    start: NaiveDate,
    end: NaiveDate,
    contact_first_name: &str,
    contact_last_name: &str,
) -> Result<SaftInput> {
    let company = sqlx::query("select orgnr, name from company where id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such company")?;

    let accounts = sqlx::query(
        "select a.number, a.name, a.created_at,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0)::bigint as opening_ore,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0)::bigint as closing_ore
         from account a
         left join entry e on e.account_id = a.id
         left join voucher v on v.id = e.voucher_id
         where a.company_id = $1
         group by a.id
         order by a.number",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| SaftAccount {
        number: row.get("number"),
        name: row.get("name"),
        created: row.get::<DateTime<Utc>, _>("created_at").date_naive(),
        opening_ore: row.get("opening_ore"),
        closing_ore: row.get("closing_ore"),
    })
    .collect();

    // TaxTable rate: the rate in force at the end of the selection period;
    // per-line TaxInformation carries the rate at each voucher's date.
    let party_rows = sqlx::query(
        "select p.party_no, p.kind, p.name, p.orgnr,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0)::bigint as opening_ore,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0)::bigint as closing_ore,
                min(a.number) as balance_account
         from party p
         left join entry e on e.party_id = p.id
         left join voucher v on v.id = e.voucher_id
         left join account a on a.id = e.account_id
         where p.company_id = $1
         group by p.id
         order by p.party_no",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;
    let (mut customers, mut suppliers) = (Vec::new(), Vec::new());
    for row in &party_rows {
        let party = SaftParty {
            party_no: row.get("party_no"),
            name: row.get("name"),
            orgnr: row.get("orgnr"),
            balance_account: row.get("balance_account"),
            opening_ore: row.get("opening_ore"),
            closing_ore: row.get("closing_ore"),
        };
        if row.get::<String, _>("kind") == "kunde" {
            customers.push(party);
        } else {
            suppliers.push(party);
        }
    }

    let tax_codes = sqlx::query(
        "select distinct vc.code, vc.description,
                coalesce(r.rate_bp, (vc.rate_percent * 100)::integer)::bigint as percent_bp
         from entry e
         join voucher v on v.id = e.voucher_id
         join vat_code vc on vc.code = e.vat_code
         left join lateral (
             select rate_bp from vat_rate
             where rate_class = vc.rate_class and valid_from <= $3
             order by valid_from desc limit 1
         ) r on true
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by vc.code",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| SaftTaxCode {
        code: row.get("code"),
        description: row.get("description"),
        percent_bp: row.get("percent_bp"),
    })
    .collect();

    // Dimension registry → AnalysisTypeTable ("AVD"/"PRO" type codes,
    // docs/dimensjoner.md).
    let analysis_types = sqlx::query(
        "select kind, code, name, active from dimension
         where company_id = $1 order by kind, code",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        let kind: String = row.get("kind");
        let (analysis_type, type_description) = if kind == "avdeling" {
            ("AVD", "Avdeling")
        } else {
            ("PRO", "Prosjekt")
        };
        SaftAnalysisType {
            analysis_type: analysis_type.into(),
            type_description: type_description.into(),
            id: row.get("code"),
            id_description: row.get("name"),
            active: row.get("active"),
        }
    })
    .collect();

    let voucher_rows = sqlx::query(
        "select j.code as journal_code, j.name as journal_name,
                v.id, v.fiscal_year, v.voucher_number, v.voucher_date, v.description,
                v.created_by, v.created_at,
                rv.fiscal_year as reversed_year, rv.voucher_number as reversed_number
         from voucher v
         join journal j on j.id = v.journal_id
         left join voucher rv on rv.id = v.reverses_voucher_id
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by j.code, v.fiscal_year, v.voucher_number",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let line_rows = sqlx::query(
        "select e.voucher_id, e.line_no, a.number as account_number,
                e.amount_ore, e.vat_code, e.description, r.rate_bp,
                p.party_no, p.kind as party_kind,
                da.code as avdeling, dp.code as prosjekt
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join dimension da on da.id = e.avdeling_id
         left join dimension dp on dp.id = e.prosjekt_id
         left join vat_code vc on vc.code = e.vat_code
         left join lateral (
             select rate_bp from vat_rate
             where rate_class = vc.rate_class and valid_from <= v.voucher_date
             order by valid_from desc limit 1
         ) r on true
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by e.voucher_id, e.line_no",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let mut lines_by_voucher: std::collections::HashMap<Uuid, Vec<SaftLine>> =
        std::collections::HashMap::new();
    for row in line_rows {
        lines_by_voucher
            .entry(row.get("voucher_id"))
            .or_default()
            .push(SaftLine {
                line_no: row.get("line_no"),
                account_number: row.get("account_number"),
                description: row.get("description"),
                amount_ore: row.get("amount_ore"),
                vat_code: row.get("vat_code"),
                tax_percent_bp: row.get::<Option<i32>, _>("rate_bp").map(i64::from),
                customer_id: match row.get::<Option<String>, _>("party_kind").as_deref() {
                    Some("kunde") => row.get("party_no"),
                    _ => None,
                },
                supplier_id: match row.get::<Option<String>, _>("party_kind").as_deref() {
                    Some("leverandor") => row.get("party_no"),
                    _ => None,
                },
                avdeling: row.get("avdeling"),
                prosjekt: row.get("prosjekt"),
            });
    }

    let mut journals: Vec<SaftJournal> = Vec::new();
    for row in voucher_rows {
        let journal_code: String = row.get("journal_code");
        if journals.last().is_none_or(|j| j.code != journal_code) {
            journals.push(SaftJournal {
                code: journal_code,
                name: row.get("journal_name"),
                transactions: Vec::new(),
            });
        }
        let voucher_id: Uuid = row.get("id");
        let reverses = match (
            row.get::<Option<i32>, _>("reversed_year"),
            row.get::<Option<i64>, _>("reversed_number"),
        ) {
            (Some(year), Some(number)) => Some(format!("{year}-{number}")),
            _ => None,
        };
        journals
            .last_mut()
            .expect("journal was just pushed")
            .transactions
            .push(SaftTransaction {
                fiscal_year: row.get("fiscal_year"),
                number: row.get("voucher_number"),
                date: row.get("voucher_date"),
                description: row.get("description"),
                created_by: row.get("created_by"),
                created_at: row.get("created_at"),
                reverses,
                lines: lines_by_voucher.remove(&voucher_id).unwrap_or_default(),
            });
    }

    Ok(SaftInput {
        orgnr: company.get("orgnr"),
        company_name: company.get("name"),
        contact_first_name: contact_first_name.to_string(),
        contact_last_name: contact_last_name.to_string(),
        file_created: Utc::now().date_naive(),
        software_version: env!("CARGO_PKG_VERSION").to_string(),
        start,
        end,
        accounts,
        customers,
        suppliers,
        tax_codes,
        analysis_types,
        journals,
    })
}
