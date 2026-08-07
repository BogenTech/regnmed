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
//! 4. Reskontro mot hovedbok: per account, the sum of the parties' own
//!    postings against the account's saldo — plus the two ways an
//!    amount can sit on the wrong side of that equation (a party of the
//!    wrong kind, a party on an account that is not flagged at all).
//! 5. Balansekontroll: all entries sum to zero.
//! 6. Periodelåsing status (informational: current lock and history).
//! 7. Regelverkssatser: no monitored sats domain is older than its
//!    known change cadence — the machine's side of the yearly
//!    regelverksrevisjon (docs/regelverk.md).

use anyhow::Result;
use regnmed_core::revisjon::{AnkerInfo, Kontroll, ReskontroKonto, RevisjonInput};
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

    // 4. Reskontro mot hovedbok — the real tie-out: per account, the sum
    // of the parties' own postings against the account's saldo. Both
    // sides are pure SUM queries over the same entries; the
    // reconciliation is `regnmed_core::revisjon::reskontro_kontroll`.
    //
    // The account list is every flagged account PLUS every unflagged
    // account that carries party postings — an amount in a party's
    // spesifikasjon that no reskontro account holds is exactly the kind
    // of divergence a check limited to flagged accounts cannot see.
    let reskontro = sqlx::query(
        "select a.number, a.reskontro_kind,
                coalesce(sum(e.amount_ore), 0)::bigint as hovedbok_ore,
                coalesce(sum(e.amount_ore) filter (where e.party_id is not null), 0)::bigint
                    as reskontro_ore,
                count(e.id) filter (where e.party_id is null)::bigint as uten_part_antall,
                count(e.id) filter (where e.party_id is not null)::bigint as med_part_antall,
                count(e.id) filter (where p.kind is not null
                                      and p.kind is distinct from a.reskontro_kind)::bigint
                    as feil_kind_antall,
                coalesce(sum(e.amount_ore) filter (where p.kind is not null
                                      and p.kind is distinct from a.reskontro_kind), 0)::bigint
                    as feil_kind_ore,
                coalesce(array_agg(distinct p.kind) filter (where p.kind is not null),
                         array[]::text[]) as part_kinds
         from account a
         left join entry e on e.account_id = a.id
         left join party p on p.id = e.party_id
         where a.company_id = $1
           and (a.reskontro_kind is not null
                or exists (select 1 from entry x
                           where x.account_id = a.id and x.party_id is not null))
         group by a.number, a.reskontro_kind
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let kontoer: Vec<ReskontroKonto> = reskontro
        .iter()
        .map(|r| {
            let mut part_kinds: Vec<String> = r.get("part_kinds");
            part_kinds.sort(); // array_agg has no order; the report is deterministic
            ReskontroKonto {
                konto: r.get("number"),
                flagg: r.get("reskontro_kind"),
                hovedbok_ore: r.get("hovedbok_ore"),
                reskontro_ore: r.get("reskontro_ore"),
                uten_part_antall: r.get("uten_part_antall"),
                med_part_antall: r.get("med_part_antall"),
                feil_kind_antall: r.get("feil_kind_antall"),
                feil_kind_ore: r.get("feil_kind_ore"),
                part_kinds,
            }
        })
        .collect();
    kontroller.push(regnmed_core::revisjon::reskontro_kontroll(&kontoer));

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

    // 7. Regelverkssatser (docs/regelverk.md): outdated satser would
    // silently produce unlawful gebyrer/renter — surfaced here so the
    // yearly regelverksrevisjon is verified, not remembered.
    let satser = crate::load_satser(pool).await?;
    let foreldede = regnmed_core::sats::foreldede_domener(&satser, chrono::Utc::now().date_naive());
    kontroller.push(Kontroll {
        navn: "Regelverkssatser".into(),
        ok: foreldede.is_empty(),
        detalj: if foreldede.is_empty() {
            let domener: std::collections::HashSet<_> =
                satser.iter().map(|s| s.domene.as_str()).collect();
            format!(
                "{} satsdomener i registeret; ingen overvåket sats er eldre enn sin kadens",
                domener.len()
            )
        } else {
            foreldede
                .iter()
                .map(|f| match f.siste {
                    Some(date) => format!("{} sist oppdatert {date}", f.domene),
                    None => format!("{} mangler i satsregisteret", f.domene),
                })
                .collect::<Vec<_>>()
                .join("; ")
        },
    });

    // 8. Importert historikk (informational): which external files the
    // ledger was built from (docs/migration.md, saft_import_log). The
    // full hashes are printed so the revisor can hash the source
    // system's export (`shasum -a 256 <fil>`) and compare byte for byte
    // with what was actually imported. History imported before the log
    // existed (migration 0054) is stated, never hidden.
    let imp_vouchers: i64 = sqlx::query_scalar(
        "select count(*)::bigint from voucher v join journal j on j.id = v.journal_id
         where v.company_id = $1 and j.code = 'IMP'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    let importlogg = crate::saft_import_log(pool, company_id).await?;
    kontroller.push(Kontroll {
        navn: "Importert historikk".into(),
        ok: true,
        detalj: if imp_vouchers == 0 {
            "ingen importert historikk — hovedboken er ført i sin helhet i regnmed".into()
        } else if importlogg.is_empty() {
            format!(
                "{imp_vouchers} bilag i importjournalen uten dokumenterte kildefiler \
                 (importert før importloggen fantes)"
            )
        } else {
            format!(
                "{imp_vouchers} bilag i importjournalen fra {} fil(er): {}",
                importlogg.len(),
                importlogg
                    .iter()
                    .map(|r| format!(
                        "sha256 {} ({} bilag, importert {} av {})",
                        r.sha256_hex,
                        r.vouchers,
                        r.created_at.date_naive(),
                        r.created_by
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
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
