//! Migreringsimport, filtier (docs/migration.md, #19) — det SAF-T ikke
//! bærer: kontaktopplysninger og åpne reskontroposter.
//!
//! Kontakter er ren registerdata og importeres idempotent (kjør filen
//! to ganger, få samme register). Åpne poster er noe helt annet: de
//! blir BILAG, og derfor gjelder hovedbokens regler.
//!
//! Åpne poster erstatter samlelinjen på reskontrokontoen — de legges
//! ikke oppå den. Derfor krever importen at kontoen står i null før
//! den kjøres, og sier tydelig fra med den faktiske saldoen hvis ikke.
//! Rekkefølgen ved migrering er dermed:
//!
//! 1. Kontakter (så postene har noen å peke på).
//! 2. Åpningsbalanse UTEN reskontrokontoene (docs/migration.md).
//! 3. Åpne poster — én partslinje per post mot motkontoen, ETT bilag,
//!    én transaksjon.
//!
//! Etterpå er reskontrosaldoen lik summen av de åpne postene fordi det
//! er de samme radene — ikke fordi noe ble avstemt i etterkant.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::migreringcsv::{ApenPostRad, KontaktRad, PartKind};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::{PostedVoucher, post_voucher_in};

#[derive(Debug, Default)]
pub struct ContactsReport {
    pub opprettet: usize,
    pub oppdatert: usize,
    pub advarsler: Vec<String>,
}

/// Creates or updates parties from an imported contact list. Matching,
/// in order: orgnr, then the old system's number when it is numeric
/// (kundenr 10001 stays 10001 — continuity the accountant can see),
/// then the name. Contact details that fail validation are warned
/// about and skipped; ONE bad kontonummer must not stop the file.
pub async fn import_contacts(
    pool: &PgPool,
    company_id: Uuid,
    rader: &[KontaktRad],
) -> Result<ContactsReport> {
    let mut report = ContactsReport::default();
    for rad in rader {
        let kind = rad.kind.as_str();
        let existing = find_party(
            pool,
            company_id,
            kind,
            rad.nummer.as_deref(),
            &rad.navn,
            rad.orgnr.as_deref(),
        )
        .await?;
        let party_id = match existing {
            Some((id, _)) => {
                report.oppdatert += 1;
                id
            }
            None => {
                // A numeric vendor number becomes the party_no; anything
                // else (S-1, L-9) gets our own numbering, and the name
                // stays the link back.
                let party_no = rad
                    .nummer
                    .as_deref()
                    .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                let created = crate::reskontro::create_party(
                    pool,
                    company_id,
                    kind,
                    &rad.navn,
                    rad.orgnr.as_deref(),
                    party_no,
                )
                .await;
                match created {
                    Ok((id, _)) => {
                        report.opprettet += 1;
                        id
                    }
                    Err(e) => {
                        report
                            .advarsler
                            .push(format!("{}: kunne ikke opprettes ({e})", rad.navn));
                        continue;
                    }
                }
            }
        };
        if rad.adresse.is_none() && rad.epost.is_none() && rad.kontonummer.is_none() {
            continue;
        }
        let full = crate::settings::update_party_contact(
            pool,
            company_id,
            party_id,
            rad.adresse.as_deref(),
            rad.epost.as_deref(),
            rad.kontonummer.as_deref(),
        )
        .await;
        if let Err(e) = full {
            report.advarsler.push(format!(
                "{}: kontaktinfo delvis hoppet over ({e})",
                rad.navn
            ));
            // Retry without the fields most likely to be at fault, so a
            // bad kontonummer does not also lose the e-mail address.
            let _ = crate::settings::update_party_contact(
                pool,
                company_id,
                party_id,
                rad.adresse.as_deref(),
                None,
                None,
            )
            .await;
        }
    }
    Ok(report)
}

async fn find_party(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    nummer: Option<&str>,
    navn: &str,
    orgnr: Option<&str>,
) -> Result<Option<(Uuid, String)>> {
    let row = sqlx::query(
        "select id, name from party
         where company_id = $1 and kind = $2
           and ( ($3::text is not null and orgnr = $3)
              or ($4::text is not null and party_no = $4)
              or lower(name) = lower($5) )
         order by ($3::text is not null and orgnr = $3) desc,
                  ($4::text is not null and party_no = $4) desc
         limit 1",
    )
    .bind(company_id)
    .bind(kind)
    .bind(orgnr.filter(|o| !o.is_empty()))
    .bind(nummer.filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty()))
    .bind(navn)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("id"), r.get("name"))))
}

#[derive(Debug)]
pub struct OpenItemsPlan {
    pub antall: usize,
    /// Signed as it will hit the reskontro account.
    pub sum_ore: i64,
    /// Parties the file references that do not exist yet — they will be
    /// created by the import, listed here so nobody is surprised.
    pub nye_parter: Vec<String>,
    /// The account's balance right now; must be zero to import.
    pub konto_saldo_ore: i64,
    pub advarsler: Vec<String>,
}

fn line_amount(kind: PartKind, belop_ore: i64) -> i64 {
    match kind {
        // A customer owing us is a debit; a supplier we owe is a credit.
        // Credit notes keep their own sign through the negation.
        PartKind::Kunde => belop_ore,
        PartKind::Leverandor => -belop_ore,
    }
}

/// What the import would do — the preview the portal shows before
/// anyone commits. Reads only.
pub async fn plan_open_items(
    pool: &PgPool,
    company_id: Uuid,
    kind: PartKind,
    konto: &str,
    rader: &[ApenPostRad],
) -> Result<OpenItemsPlan> {
    let saldo: Option<i64> = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company_id)
    .bind(konto)
    .fetch_optional(pool)
    .await?;
    let konto_saldo_ore = saldo.context("no such account")?;

    let mut nye_parter = Vec::new();
    let mut advarsler = Vec::new();
    for rad in rader {
        let found = find_party(
            pool,
            company_id,
            kind.as_str(),
            Some(rad.part.as_str()),
            rad.part_navn.as_deref().unwrap_or(&rad.part),
            None,
        )
        .await?;
        if found.is_none() {
            let navn = rad.part_navn.clone().unwrap_or_else(|| rad.part.clone());
            if !nye_parter.contains(&navn) {
                nye_parter.push(navn);
            }
        }
    }
    if !nye_parter.is_empty() {
        advarsler.push(format!(
            "{} part(er) finnes ikke og opprettes ved import — importer kontaktlisten først \
             hvis du vil ha adresser og kontonumre med",
            nye_parter.len()
        ));
    }
    Ok(OpenItemsPlan {
        antall: rader.len(),
        sum_ore: rader.iter().map(|r| line_amount(kind, r.belop_ore)).sum(),
        nye_parter,
        konto_saldo_ore,
        advarsler,
    })
}

#[derive(Debug)]
pub struct OpenItemsReport {
    pub posted: PostedVoucher,
    pub antall: usize,
    pub sum_ore: i64,
    pub opprettede_parter: usize,
    pub advarsler: Vec<String>,
}

/// Posts the open items as ONE voucher: one party-carrying line per
/// item on the reskontro account, balanced against `motkonto` — all in
/// one transaction, so a file either lands whole or not at all.
pub async fn import_open_items(
    pool: &PgPool,
    company_id: Uuid,
    kind: PartKind,
    konto: &str,
    motkonto: &str,
    dato: NaiveDate,
    rader: &[ApenPostRad],
    created_by: &str,
) -> Result<OpenItemsReport> {
    ensure!(!rader.is_empty(), "filen inneholder ingen åpne poster");
    ensure!(
        konto != motkonto,
        "konto og motkonto kan ikke være den samme"
    );

    let mut tx = pool.begin().await?;
    let saldo: Option<i64> = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company_id)
    .bind(konto)
    .fetch_optional(&mut *tx)
    .await?;
    let saldo = saldo.context("no such account")?;
    if saldo != 0 {
        bail!(
            "konto {konto} har allerede saldo {} øre — åpne poster ERSTATTER samlelinjen, \
             de legges ikke oppå den. Utelat kontoen fra åpningsbalansen (eller reverser \
             samlelinjen) og kjør importen på nytt",
            saldo
        );
    }

    // The account must carry the reskontro flag for party lines to be
    // accepted — the opening balance defers the flag on purpose
    // (crate::opening), and this is where it comes back.
    let updated =
        sqlx::query("update account set reskontro_kind = $3 where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(konto)
            .bind(kind.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
    ensure!(updated == 1, "no account {konto}");

    let mut advarsler = Vec::new();
    let mut opprettede_parter = 0usize;
    let mut entries = Vec::with_capacity(rader.len() + 1);
    for rad in rader {
        let navn = rad.part_navn.clone().unwrap_or_else(|| rad.part.clone());
        let found = find_party_in(
            &mut tx,
            company_id,
            kind.as_str(),
            Some(rad.part.as_str()),
            &navn,
        )
        .await?;
        let party_no = match found {
            Some(no) => no,
            None => {
                let no =
                    create_party_in(&mut tx, company_id, kind.as_str(), &navn, &rad.part).await?;
                opprettede_parter += 1;
                no
            }
        };
        let mut tekst = rad.dokument.clone().unwrap_or_else(|| "Åpen post".into());
        if let Some(kid) = &rad.kid {
            tekst = format!("{tekst} (KID {kid})");
        }
        if let Some(forfall) = rad.forfall {
            tekst = format!("{tekst} forfall {forfall}");
        }
        entries.push(EntryDraft {
            account_number: konto.to_string(),
            amount: Ore(line_amount(kind, rad.belop_ore)),
            vat_code: None,
            description: Some(tekst),
            party_no: Some(party_no),
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    let sum_ore: i64 = entries.iter().map(|e| e.amount.0).sum();
    entries.push(EntryDraft {
        account_number: motkonto.to_string(),
        amount: Ore(-sum_ore),
        vat_code: None,
        description: Some("Motpost åpne poster ved migrering".into()),
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    });

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!(
            "Åpne poster {} ved migrering",
            match kind {
                PartKind::Kunde => "kunder",
                PartKind::Leverandor => "leverandører",
            }
        ),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
    tx.commit().await?;

    if opprettede_parter > 0 {
        advarsler.push(format!(
            "{opprettede_parter} part(er) ble opprettet fra postlisten uten kontaktopplysninger"
        ));
    }
    Ok(OpenItemsReport {
        posted,
        antall: rader.len(),
        sum_ore,
        opprettede_parter,
        advarsler,
    })
}

async fn find_party_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    kind: &str,
    nummer: Option<&str>,
    navn: &str,
) -> Result<Option<String>> {
    let numeric = nummer.filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let row = sqlx::query(
        "select party_no from party
         where company_id = $1 and kind = $2
           and ( ($3::text is not null and party_no = $3) or lower(name) = lower($4) )
         order by ($3::text is not null and party_no = $3) desc
         limit 1",
    )
    .bind(company_id)
    .bind(kind)
    .bind(numeric)
    .bind(navn)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| r.get("party_no")))
}

async fn create_party_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    kind: &str,
    navn: &str,
    vendor_ref: &str,
) -> Result<String> {
    let numeric = (!vendor_ref.is_empty() && vendor_ref.chars().all(|c| c.is_ascii_digit()))
        .then(|| vendor_ref.to_string());
    let party_no = match numeric {
        Some(no) => no,
        None => {
            let first: i64 = if kind == "kunde" { 10_000 } else { 20_000 };
            let next: i64 = sqlx::query_scalar(
                "select coalesce(max(party_no::bigint) + 1, $2)
                 from party where company_id = $1 and kind = $3",
            )
            .bind(company_id)
            .bind(first)
            .bind(kind)
            .fetch_one(&mut **tx)
            .await?;
            next.to_string()
        }
    };
    sqlx::query(
        "insert into party (id, company_id, party_no, kind, name)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(&party_no)
    .bind(kind)
    .bind(navn)
    .execute(&mut **tx)
    .await
    .context("creating party from open items")?;
    Ok(party_no)
}
