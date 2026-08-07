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

    // 9. Dokumentasjon (informational, #85): bokføringsloven §10 wants
    // booked information documented, and "which bilag lack it?" is the
    // question a bokettersyn opens with — the system could not answer it.
    //
    // An INFORMASJONSKONTROLL, not an AVVIK: a missing attachment is not
    // proof of a missing document. It may sit in a permanent archive
    // elsewhere, and documentation legitimately arrives after the
    // posting. Making vedlegg mandatory would only teach people to
    // upload rubbish to get past the form.
    //
    // Bilag that carry their documentation BY CONSTRUCTION are not
    // counted: faktura and innboks copy the document onto the voucher in
    // the issuing transaction, so they simply have one. Importjournalen
    // is documented by its source files, which kontroll 8 hashes.
    let udokumenterte: i64 = sqlx::query_scalar(
        "select count(*)::bigint from voucher v
         where v.company_id = $1
           and not exists (select 1 from attachment a where a.voucher_id = v.id)
           and not exists (select 1 from journal j
                           where j.id = v.journal_id and j.code = 'IMP')",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    let totalt: i64 = sqlx::query_scalar(
        "select count(*)::bigint from voucher v
         where v.company_id = $1
           and not exists (select 1 from journal j
                           where j.id = v.journal_id and j.code = 'IMP')",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    let eldste: Vec<String> = sqlx::query(
        "select v.fiscal_year, v.voucher_number, v.voucher_date, v.description
         from voucher v
         where v.company_id = $1
           and not exists (select 1 from attachment a where a.voucher_id = v.id)
           and not exists (select 1 from journal j
                           where j.id = v.journal_id and j.code = 'IMP')
         order by v.voucher_date, v.chain_seq limit 10",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| {
        format!(
            "{}-{} {} {}",
            r.get::<i32, _>("fiscal_year"),
            r.get::<i64, _>("voucher_number"),
            r.get::<chrono::NaiveDate, _>("voucher_date"),
            r.get::<String, _>("description")
        )
    })
    .collect();
    kontroller.push(Kontroll {
        navn: "Dokumentasjon".into(),
        ok: true,
        detalj: if udokumenterte == 0 {
            format!("alle {totalt} bilag utenfor importjournalen har vedlegg")
        } else {
            format!(
                "{udokumenterte} av {totalt} bilag mangler vedlegg i regnmed \
                 (informasjon, ikke avvik — dokumentasjonen kan finnes i annet \
                 oppbevaringsmedium; bokføringsloven §10 krever at den finnes, \
                 ikke at den ligger her). Eldste: {}",
                eldste.join("; ")
            )
        },
    });

    // 10. Balansedokumentasjon (#88): bokføringsloven §11 wants
    // documentation of what each balance post CONSISTS OF at period end.
    //
    // Unlike kontroll 9 this IS an avvik. The difference is what the law
    // asks of each: §10 says booked information shall be documented, and
    // the documentation may lawfully live in another oppbevaringsmedium
    // — so a missing attachment is not proof of anything. §11 says the
    // documentation SHALL EXIST for the balance post, and regnmed is
    // where the company records that it does. Nobody else can say it for
    // them.
    //
    // Measured at the latest closed period. Without one there is nothing
    // to document yet, and saying so beats inventing a deadline.
    let siste_las: Option<chrono::NaiveDate> = sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    kontroller.push(match siste_las {
        None => Kontroll {
            navn: "Balansedokumentasjon".into(),
            ok: true,
            detalj: "ingen periode er låst ennå — balansepostene dokumenteres ved periodeslutt \
                     (bokføringsloven §11)"
                .into(),
        },
        Some(periode) => {
            let linjer = crate::balansedok::balanse_status(pool, company_id, periode).await?;
            let udokumentert: Vec<&crate::balansedok::BalanseLinje> =
                linjer.iter().filter(|l| l.avstemt.is_none()).collect();
            let flyttet: Vec<(&str, i64)> = linjer
                .iter()
                .filter_map(|l| l.avvik_ore().map(|d| (l.konto.as_str(), d)))
                .collect();
            let mut deler = Vec::new();
            if !udokumentert.is_empty() {
                deler.push(format!(
                    "{} av {} balansekontoer med saldo mangler dokumentasjon per {periode}: {}",
                    udokumentert.len(),
                    linjer.len(),
                    udokumentert
                        .iter()
                        .take(10)
                        .map(|l| format!("{} {}", l.konto, l.kontonavn))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            // Documented, then posted to. Not the same failing, and the
            // report must not blur them: the avstemming happened, the
            // saldo simply moved afterwards.
            if !flyttet.is_empty() {
                deler.push(format!(
                    "bokført videre etter avstemming: {}",
                    flyttet
                        .iter()
                        .map(|(k, d)| format!("{k} ({} øre)", d))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Kontroll {
                navn: "Balansedokumentasjon".into(),
                ok: deler.is_empty(),
                detalj: if deler.is_empty() {
                    format!(
                        "alle {} balansekontoer med saldo er dokumentert per {periode}",
                        linjer.len()
                    )
                } else {
                    deler.join("; ")
                },
            }
        }
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
