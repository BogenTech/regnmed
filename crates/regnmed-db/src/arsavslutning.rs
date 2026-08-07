//! Årsavslutning (#84, docs/arsavslutning.md): resultatdisponering og
//! skattekostnad, as an ordinary voucher.
//!
//! The disposition account is 8800, and that is Skatteetatens choice
//! rather than ours — its næringsspesifikasjon code list gives 8800 its
//! own grouping category, separate from every income-statement line
//! (`regnmed_core::regnskap::DISPONERING_KONTO`).
//!
//! Ordering: the closing SETS the period lock rather than requiring it,
//! because the voucher is dated 31.12 and the database trigger from
//! migration 0011 refuses postings into a locked period regardless of
//! what the application thinks. Same protection, opposite order — see
//! the migration for the full reasoning.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Konti the closing uses. Named here rather than guessed per call:
/// 8300/2500 is the ordinary betalbar-skatt pair, 8800/2050 the
/// disposition.
pub const SKATTEKOSTNAD_KONTO: &str = "8300";
pub const BETALBAR_SKATT_KONTO: &str = "2500";
pub const OPPTJENT_EK_KONTO: &str = "2050";

#[derive(Debug)]
pub struct Arsavslutning {
    pub ar: i32,
    pub voucher: (i32, i64),
    pub resultat_for_skatt_ore: i64,
    pub skattekostnad_ore: i64,
    pub disponert_ore: i64,
    pub created_by: String,
}

/// The fiscal year's span — the calendar-year assumption lives in ONE
/// place (`regnmed_core::regnskapsar`, docs/regelverk.md), and this goes
/// through that seam like the posting and the SAF-T export do.
fn periode(ar: i32) -> Result<(NaiveDate, NaiveDate)> {
    regnmed_core::regnskapsar::regnskapsar_periode(ar).with_context(|| format!("ugyldig år {ar}"))
}

/// The year's result before tax, from the ledger — presentation sign,
/// positive = overskudd. Read here rather than taken from the caller:
/// the number that gets disponert must be the ledger's own.
pub async fn resultat_for_aret(pool: &PgPool, company_id: Uuid, ar: i32) -> Result<i64> {
    let (fra, til) = periode(ar)?;
    let lines = crate::saldo_lines(pool, company_id, Some(fra), til, None, None).await?;
    Ok(regnmed_core::regnskap::resultat(&lines).arsresultat_ore)
}

/// Closes a year: accrues the tax, transfers the result to equity, and
/// locks the year — one voucher, one transaction.
///
/// `skattekostnad_ore` is the CALLER'S number, not a percentage of the
/// accounting profit. Taxable income is not accounting income
/// (permanent and temporary differences; `saldo_rapport` already
/// computes the latter for anleggsmidler), so deriving it here would be
/// inventing a tax return. Zero is a legitimate answer and must be
/// stated, never assumed.
pub async fn avslutt_ar(
    pool: &PgPool,
    company_id: Uuid,
    ar: i32,
    skattekostnad_ore: i64,
    created_by: &str,
) -> Result<Arsavslutning> {
    ensure!(
        skattekostnad_ore >= 0,
        "skattekostnaden kan ikke være negativ — en skattefordel er utsatt skatt, som ikke er bygget"
    );
    let (_, ar_slutt) = periode(ar)?;

    let alt_avsluttet: Option<i32> =
        sqlx::query_scalar("select ar from arsavslutning where company_id = $1 and ar = $2")
            .bind(company_id)
            .bind(ar)
            .fetch_optional(pool)
            .await?;
    ensure!(
        alt_avsluttet.is_none(),
        "{ar} er allerede avsluttet — en korreksjon er et reverserende bilag, ikke en ny avslutning"
    );

    // The lock would refuse our own voucher, so say so plainly instead
    // of letting the trigger produce a confusing error two layers down.
    let las: Option<NaiveDate> = sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    if let Some(las) = las {
        ensure!(
            las < ar_slutt,
            "perioden er låst til og med {las}, så årsavslutningsbilaget for {ar} \
             (datert {ar_slutt}) kan ikke posteres. Årsavslutningen låser året selv — \
             lås derfor ikke {ar} på forhånd"
        );
    }

    let resultat_for_skatt_ore = resultat_for_aret(pool, company_id, ar).await?;
    let disponert_ore = resultat_for_skatt_ore - skattekostnad_ore;

    let linje = |konto: &str, belop: i64, tekst: &str| EntryDraft {
        account_number: konto.to_string(),
        amount: Ore(belop),
        vat_code: None,
        description: Some(tekst.to_string()),
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    };
    let mut entries = Vec::with_capacity(4);
    if skattekostnad_ore != 0 {
        entries.push(linje(
            SKATTEKOSTNAD_KONTO,
            skattekostnad_ore,
            "Skattekostnad",
        ));
        entries.push(linje(
            BETALBAR_SKATT_KONTO,
            -skattekostnad_ore,
            "Betalbar skatt",
        ));
    }
    // A zero result still closes the year — the year happened, and the
    // arsavslutning row is what says so. The voucher just has no
    // disposition lines then.
    if disponert_ore != 0 {
        entries.push(linje(
            regnmed_core::regnskap::DISPONERING_KONTO,
            disponert_ore,
            "Disponering av årets resultat",
        ));
        entries.push(linje(
            OPPTJENT_EK_KONTO,
            -disponert_ore,
            "Overført til annen egenkapital",
        ));
    }
    ensure!(
        !entries.is_empty(),
        "{ar} har verken resultat eller skattekostnad å disponere"
    );

    for (nr, navn) in [
        (SKATTEKOSTNAD_KONTO, "Skattekostnad"),
        (BETALBAR_SKATT_KONTO, "Betalbar skatt"),
        (
            regnmed_core::regnskap::DISPONERING_KONTO,
            "Disponering av årets resultat",
        ),
        (OPPTJENT_EK_KONTO, "Annen egenkapital"),
    ] {
        crate::ensure_account(pool, company_id, nr, navn).await?;
    }

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: ar_slutt,
        description: format!("Årsavslutning {ar}"),
        reverses: None,
        entries,
    };

    let mut tx = pool.begin().await?;
    let posted = crate::post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
    sqlx::query(
        "insert into arsavslutning
             (id, company_id, ar, voucher_id, resultat_for_skatt_ore,
              skattekostnad_ore, disponert_ore, created_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(ar)
    .bind(posted.id)
    .bind(resultat_for_skatt_ore)
    .bind(skattekostnad_ore)
    .bind(disponert_ore)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    // The lock is part of the same act: after this, the year is both
    // disponert and closed to new postings.
    sqlx::query(
        "insert into period_lock (id, company_id, locked_through, set_by) values ($1,$2,$3,$4)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(ar_slutt)
    .bind(format!("årsavslutning {ar} ({created_by})"))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Arsavslutning {
        ar,
        voucher: (posted.fiscal_year, posted.voucher_number),
        resultat_for_skatt_ore,
        skattekostnad_ore,
        disponert_ore,
        created_by: created_by.to_string(),
    })
}

/// The closed years, newest first.
pub async fn list_arsavslutninger(pool: &PgPool, company_id: Uuid) -> Result<Vec<Arsavslutning>> {
    Ok(sqlx::query(
        "select a.ar, v.fiscal_year, v.voucher_number, a.resultat_for_skatt_ore,
                a.skattekostnad_ore, a.disponert_ore, a.created_by
         from arsavslutning a join voucher v on v.id = a.voucher_id
         where a.company_id = $1 order by a.ar desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| Arsavslutning {
        ar: r.get("ar"),
        voucher: (r.get("fiscal_year"), r.get("voucher_number")),
        resultat_for_skatt_ore: r.get("resultat_for_skatt_ore"),
        skattekostnad_ore: r.get("skattekostnad_ore"),
        disponert_ore: r.get("disponert_ore"),
        created_by: r.get("created_by"),
    })
    .collect())
}
