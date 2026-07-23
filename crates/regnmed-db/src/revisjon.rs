//! Assembles the revisor's verification report: runs every check the
//! system can make about its own ledger and reports the outcome honestly
//! — a failed check goes into the report as AVVIK, it is never an error
//! that hides the document.
//!
//! Kontroller:
//! 1. Hash-kjeden re-walked from genesis (content + links + head).
//! 2. Attachment content re-hashed against stored SHA-256.
//! 3. External anchors: anchored heads still on the chain, roots
//!    recompute (docs/anchoring.md).
//! 4. Reskontro mot hovedbok: on reskontro-flagged accounts every entry
//!    carries a party, so subledger == hovedbok by construction — the
//!    check proves the invariant actually holds in the data.
//! 5. Balansekontroll: all entries sum to zero.
//! 6. Periodelåsing status (informational: current lock and history).

use anyhow::Result;
use regnmed_core::revisjon::{AnkerInfo, Kontroll, RevisjonInput};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn build_revisjon_report(
    pool: &PgPool,
    company_id: Uuid,
    generated_by: &str,
    app_version: &str,
) -> Result<RevisjonInput> {
    let company = sqlx::query("select orgnr, name from company where id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    let head = sqlx::query("select last_seq, last_hash from chain_head where company_id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await?;

    let mut kontroller = Vec::new();

    // 1. Chain re-walk — an error is a finding, not a crash.
    kontroller.push(match crate::verify_chain(pool, company_id).await {
        Ok(report) => Kontroll {
            navn: "Hash-kjede fra genesis".into(),
            ok: true,
            detalj: format!(
                "{} bilag re-hashet fra lagret innhold; alle lenker og kjedehodet stemmer",
                report.vouchers_checked
            ),
        },
        Err(e) => Kontroll {
            navn: "Hash-kjede fra genesis".into(),
            ok: false,
            detalj: e.to_string(),
        },
    });

    // 2. Attachment content hashes.
    kontroller.push(match crate::verify_attachments(pool, company_id).await {
        Ok(count) => Kontroll {
            navn: "Bilagsvedlegg".into(),
            ok: true,
            detalj: format!("{count} vedlegg re-hashet mot lagret SHA-256"),
        },
        Err(e) => Kontroll {
            navn: "Bilagsvedlegg".into(),
            ok: false,
            detalj: e.to_string(),
        },
    });

    // 3. External anchors.
    let anchor_check = crate::verify_company_anchors(pool, company_id).await?;
    kontroller.push(Kontroll {
        navn: "Ekstern forankring".into(),
        ok: anchor_check.problems.is_empty(),
        detalj: if anchor_check.problems.is_empty() {
            format!(
                "{} forankringer kontrollert mot den levende kjeden",
                anchor_check.snapshots_checked
            )
        } else {
            anchor_check.problems.join("; ")
        },
    });

    // 4. Reskontro mot hovedbok: entries without a party on flagged
    // accounts would make the subledger diverge from the account.
    let reskontro = sqlx::query(
        "select a.number,
                coalesce(sum(e.amount_ore) filter (where e.party_id is null), 0)::bigint as uten_part,
                count(*) filter (where e.party_id is null)::bigint as uten_part_antall
         from account a
         left join entry e on e.account_id = a.id
         where a.company_id = $1 and a.reskontro_kind is not null
         group by a.number
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let avvik: Vec<String> = reskontro
        .iter()
        .filter(|r| r.get::<i64, _>("uten_part_antall") != 0)
        .map(|r| {
            format!(
                "konto {}: {} posteringer uten part",
                r.get::<String, _>("number"),
                r.get::<i64, _>("uten_part_antall")
            )
        })
        .collect();
    kontroller.push(Kontroll {
        navn: "Reskontro mot hovedbok".into(),
        ok: avvik.is_empty(),
        detalj: if avvik.is_empty() {
            format!(
                "{} reskontrokontoer avstemt; hver postering bærer part",
                reskontro.len()
            )
        } else {
            avvik.join("; ")
        },
    });

    // 5. Balansekontroll.
    let total: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join voucher v on v.id = e.voucher_id
         where v.company_id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    kontroller.push(Kontroll {
        navn: "Balansekontroll".into(),
        ok: total == 0,
        detalj: if total == 0 {
            "summen av alle posteringer er nøyaktig null".into()
        } else {
            format!("posteringene summerer til {total} øre, ikke null")
        },
    });

    // 6. Periodelåsing (informational).
    let lock: Option<chrono::NaiveDate> = sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    let lock_rows: i64 =
        sqlx::query_scalar("select count(*)::bigint from period_lock where company_id = $1")
            .bind(company_id)
            .fetch_one(pool)
            .await?;
    kontroller.push(Kontroll {
        navn: "Periodelåsing".into(),
        ok: true,
        detalj: match lock {
            Some(date) => format!(
                "låst til og med {date}; {lock_rows} hendelser i den ureviderbare låshistorikken"
            ),
            None => "ingen periode er låst ennå".into(),
        },
    });

    let ankere = crate::company_anchors(pool, company_id)
        .await?
        .into_iter()
        .map(|a| AnkerInfo {
            tidspunkt: a.created_at.to_rfc3339(),
            root_hex: hex::encode(a.root_hash),
            siste_sekvens: a.last_seq,
            vitner: a
                .witnesses
                .iter()
                .map(|w| {
                    format!(
                        "{} {} ({})",
                        w.method,
                        w.reference,
                        w.witnessed_at.to_rfc3339()
                    )
                })
                .collect(),
        })
        .collect();

    Ok(RevisjonInput {
        orgnr: company.get("orgnr"),
        selskap: company.get("name"),
        generert: chrono::Utc::now().to_rfc3339(),
        generert_av: generated_by.to_string(),
        programversjon: app_version.to_string(),
        kjede_sekvens: head.get("last_seq"),
        kjede_hode_hex: hex::encode(head.get::<Vec<u8>, _>("last_hash")),
        kontroller,
        ankere,
    })
}
