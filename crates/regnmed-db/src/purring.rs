//! Betalingsoppfølging: forfalte fakturaer (alltid beregnet fra åpne
//! poster, aldri lagret tilstand) og purreskritt — regelverket anvendes
//! rent i `regnmed-core::purring`, satsene kommer fra satsregisteret,
//! og gebyr/rente som kreves bokføres i samme transaksjon som skrittet
//! registreres (docs/purring.md).

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::purring::{
    PurringDokument, RenteBeregning, Steg, TidligereSkritt, build_krav_voucher, forsinkelsesrente,
    render_dokument, valider_steg,
};
use regnmed_core::sats::sats_on;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;
use crate::sats::load_satser;

#[derive(Debug)]
pub struct OverdueInvoice {
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub party_no: String,
    pub party_name: String,
    pub due_date: NaiveDate,
    pub days_overdue: i64,
    /// Aldersfordeling: "1-14", "15-30" eller "30+".
    pub bucket: &'static str,
    pub remaining_ore: i64,
    pub last_steg: Option<String>,
    pub last_sent: Option<NaiveDate>,
}

/// Åpne, forfalte fakturaer per `per_date`, med aldersintervall og siste
/// purreskritt. Kreditnotaer teller aldri som forfalt.
pub async fn overdue_invoices(
    pool: &PgPool,
    company_id: Uuid,
    per_date: NaiveDate,
) -> Result<Vec<OverdueInvoice>> {
    let rows = sqlx::query(
        "select i.id, i.invoice_no, p.party_no, p.name as party_name, i.due_date,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore,
                r.steg as last_steg, r.sent_date as last_sent
         from invoice i
         join party p on p.id = i.party_id
         join entry e on e.id = i.receivable_entry_id
         left join lateral (
             select steg, sent_date from invoice_reminder
             where invoice_id = i.id
             order by created_at desc limit 1
         ) r on true
         where i.company_id = $1 and i.credits_invoice_id is null and i.due_date < $2
         order by i.due_date, i.invoice_no",
    )
    .bind(company_id)
    .bind(per_date)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter(|r| r.get::<i64, _>("remaining_ore") > 0)
        .map(|r| {
            let due_date: NaiveDate = r.get("due_date");
            let days_overdue = (per_date - due_date).num_days();
            OverdueInvoice {
                invoice_id: r.get("id"),
                invoice_no: r.get("invoice_no"),
                party_no: r.get("party_no"),
                party_name: r.get("party_name"),
                due_date,
                days_overdue,
                bucket: match days_overdue {
                    ..=14 => "1-14",
                    15..=30 => "15-30",
                    _ => "30+",
                },
                remaining_ore: r.get("remaining_ore"),
                last_steg: r.get("last_steg"),
                last_sent: r.get("last_sent"),
            }
        })
        .collect())
}

#[derive(Debug)]
pub struct ReminderDraft {
    pub steg: String,
    /// None → dagens dato (current_date i databasen).
    pub sent_date: Option<NaiveDate>,
    pub frist_date: NaiveDate,
    pub gebyr_ore: i64,
    /// Krev påløpt forsinkelsesrente i tillegg til hovedkravet.
    pub med_rente: bool,
    /// Skyldner er næringsdrivende: gebyrtaket er standardkompensasjonen
    /// (forsinkelsesrenteloven §3a), ikke purregebyr_maks.
    pub naeringsdrivende: bool,
    pub gebyr_account: String,
    pub rente_account: String,
}

#[derive(Debug)]
pub struct ReminderResult {
    pub reminder_id: Option<Uuid>,
    pub steg: String,
    pub sent_date: NaiveDate,
    pub frist_date: NaiveDate,
    pub remaining_ore: i64,
    pub gebyr_ore: i64,
    pub maks_gebyr_ore: i64,
    pub rente_ore: i64,
    pub total_ore: i64,
    pub kid: String,
    pub document: String,
    pub voucher: Option<(i32, i64)>,
}

struct InvoiceFacts {
    invoice_no: i64,
    invoice_date: NaiveDate,
    due_date: NaiveDate,
    kid: String,
    party_no: String,
    party_name: String,
    company_name: String,
    orgnr: String,
    journal_code: String,
    receivable_account: String,
    remaining_ore: i64,
    historikk: Vec<TidligereSkritt>,
}

async fn invoice_facts(pool: &PgPool, company_id: Uuid, invoice_id: Uuid) -> Result<InvoiceFacts> {
    let row = sqlx::query(
        "select i.invoice_no, i.invoice_date, i.due_date, i.kid,
                p.party_no, p.name as party_name,
                c.name as company_name, c.orgnr,
                j.code as journal_code,
                (select a.number from entry x join account a on a.id = x.account_id
                 where x.id = i.receivable_entry_id) as receivable_account,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore
         from invoice i
         join party p on p.id = i.party_id
         join company c on c.id = i.company_id
         join entry e on e.id = i.receivable_entry_id
         join voucher v on v.id = i.voucher_id
         join journal j on j.id = v.journal_id
         where i.id = $1 and i.company_id = $2 and i.credits_invoice_id is null",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice (or it is a kreditnota)")?;

    let reminder_rows = sqlx::query(
        "select steg, gebyr_ore from invoice_reminder where invoice_id = $1 order by created_at",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;
    let historikk = reminder_rows
        .iter()
        .map(|r| {
            let steg = r.get::<String, _>("steg");
            Ok(TidligereSkritt {
                steg: Steg::parse(&steg).with_context(|| format!("ukjent steg {steg}"))?,
                gebyr_ore: r.get("gebyr_ore"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(InvoiceFacts {
        invoice_no: row.get("invoice_no"),
        invoice_date: row.get("invoice_date"),
        due_date: row.get("due_date"),
        kid: row.get("kid"),
        party_no: row.get("party_no"),
        party_name: row.get("party_name"),
        company_name: row.get("company_name"),
        orgnr: row.get("orgnr"),
        journal_code: row.get("journal_code"),
        receivable_account: row.get("receivable_account"),
        remaining_ore: row.get("remaining_ore"),
        historikk,
    })
}

struct PreparedReminder {
    facts: InvoiceFacts,
    steg: Steg,
    sent_date: NaiveDate,
    maks_gebyr_ore: i64,
    rente: RenteBeregning,
    document: String,
}

/// Felles vei for forhåndsvisning og registrering: fakta, satser,
/// regelverkssjekk og rendering — uten å skrive noe.
async fn prepare(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    draft: &ReminderDraft,
) -> Result<PreparedReminder> {
    let steg = Steg::parse(&draft.steg).with_context(|| {
        format!(
            "ukjent steg {} (paminnelse|purring|inkassovarsel)",
            draft.steg
        )
    })?;
    let facts = invoice_facts(pool, company_id, invoice_id).await?;
    ensure!(
        facts.remaining_ore > 0,
        "fakturaen er gjort opp — ingenting å purre"
    );

    let sent_date = match draft.sent_date {
        Some(d) => d,
        None => {
            sqlx::query_scalar("select current_date")
                .fetch_one(pool)
                .await?
        }
    };
    let satser = load_satser(pool).await?;
    let gebyr_domene = if draft.naeringsdrivende {
        "standardkompensasjon"
    } else {
        "purregebyr_maks"
    };
    let maks_gebyr_ore = sats_on(&satser, gebyr_domene, sent_date).unwrap_or(0);

    let rente = if draft.med_rente {
        forsinkelsesrente(facts.remaining_ore, facts.due_date, sent_date, &satser)?
    } else {
        RenteBeregning {
            perioder: vec![],
            sum_ore: 0,
        }
    };
    valider_steg(
        steg,
        sent_date,
        draft.frist_date,
        facts.due_date,
        draft.gebyr_ore,
        maks_gebyr_ore,
        &facts.historikk,
    )?;

    let document = render_dokument(&PurringDokument {
        steg,
        selskap: facts.company_name.clone(),
        orgnr: facts.orgnr.clone(),
        kunde_navn: facts.party_name.clone(),
        kunde_nr: facts.party_no.clone(),
        faktura_no: facts.invoice_no,
        faktura_dato: facts.invoice_date,
        forfall: facts.due_date,
        sent_date,
        frist_date: draft.frist_date,
        restbelop_ore: facts.remaining_ore,
        rente: rente.clone(),
        gebyr_ore: draft.gebyr_ore,
        kid: facts.kid.clone(),
    });
    Ok(PreparedReminder {
        facts,
        steg,
        sent_date,
        maks_gebyr_ore,
        rente,
        document,
    })
}

fn result_of(p: &PreparedReminder, draft: &ReminderDraft) -> ReminderResult {
    ReminderResult {
        reminder_id: None,
        steg: p.steg.as_str().to_string(),
        sent_date: p.sent_date,
        frist_date: draft.frist_date,
        remaining_ore: p.facts.remaining_ore,
        gebyr_ore: draft.gebyr_ore,
        maks_gebyr_ore: p.maks_gebyr_ore,
        rente_ore: p.rente.sum_ore,
        total_ore: p.facts.remaining_ore + draft.gebyr_ore + p.rente.sum_ore,
        kid: p.facts.kid.clone(),
        document: p.document.clone(),
        voucher: None,
    }
}

/// Forhåndsvisning: beregner gebyrtak, rente og dokument uten å skrive.
pub async fn preview_reminder(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    draft: &ReminderDraft,
) -> Result<ReminderResult> {
    let prepared = prepare(pool, company_id, invoice_id, draft).await?;
    Ok(result_of(&prepared, draft))
}

/// Registrerer skrittet: én transaksjon som bokfører gebyr/rente (når
/// krevd) og setter inn purreraden — feiler bokføringen finnes intet
/// skritt, samme mønster som bilagsinnboksen.
pub async fn create_reminder(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    draft: &ReminderDraft,
    created_by: &str,
) -> Result<ReminderResult> {
    let prepared = prepare(pool, company_id, invoice_id, draft).await?;
    let krav_ore = draft.gebyr_ore + prepared.rente.sum_ore;

    let mut tx = pool.begin().await?;
    let posted = if krav_ore > 0 {
        let voucher = build_krav_voucher(
            &prepared.facts.journal_code,
            prepared.sent_date,
            prepared.steg,
            prepared.facts.invoice_no,
            &prepared.facts.party_no,
            &prepared.facts.receivable_account,
            &draft.gebyr_account,
            draft.gebyr_ore,
            &draft.rente_account,
            prepared.rente.sum_ore,
        )?;
        Some(post_voucher_in(&mut tx, company_id, &voucher, created_by).await?)
    } else {
        None
    };

    let reminder_id = Uuid::now_v7();
    sqlx::query(
        "insert into invoice_reminder (id, invoice_id, steg, sent_date, frist_date,
                                       remaining_ore, gebyr_ore, rente_ore, voucher_id,
                                       document, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(reminder_id)
    .bind(invoice_id)
    .bind(prepared.steg.as_str())
    .bind(prepared.sent_date)
    .bind(draft.frist_date)
    .bind(prepared.facts.remaining_ore)
    .bind(draft.gebyr_ore)
    .bind(prepared.rente.sum_ore)
    .bind(posted.as_ref().map(|p| p.id))
    .bind(&prepared.document)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut result = result_of(&prepared, draft);
    result.reminder_id = Some(reminder_id);
    result.voucher = posted.map(|p| (p.fiscal_year, p.voucher_number));
    Ok(result)
}

#[derive(Debug)]
pub struct ReminderRow {
    pub reminder_id: Uuid,
    pub steg: String,
    pub sent_date: NaiveDate,
    pub frist_date: NaiveDate,
    pub remaining_ore: i64,
    pub gebyr_ore: i64,
    pub rente_ore: i64,
    pub voucher: Option<(i32, i64)>,
    pub created_by: String,
}

/// Purrehistorikken for en faktura, eldst først (insert-only — dette er
/// hele hendelsesloggen).
pub async fn list_reminders(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<ReminderRow>> {
    let rows = sqlx::query(
        "select r.id, r.steg, r.sent_date, r.frist_date, r.remaining_ore,
                r.gebyr_ore, r.rente_ore, r.created_by,
                v.fiscal_year, v.voucher_number
         from invoice_reminder r
         join invoice i on i.id = r.invoice_id
         left join voucher v on v.id = r.voucher_id
         where r.invoice_id = $1 and i.company_id = $2
         order by r.created_at",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ReminderRow {
            reminder_id: r.get("id"),
            steg: r.get("steg"),
            sent_date: r.get("sent_date"),
            frist_date: r.get("frist_date"),
            remaining_ore: r.get("remaining_ore"),
            gebyr_ore: r.get("gebyr_ore"),
            rente_ore: r.get("rente_ore"),
            voucher: r
                .get::<Option<i32>, _>("fiscal_year")
                .zip(r.get::<Option<i64>, _>("voucher_number")),
            created_by: r.get("created_by"),
        })
        .collect())
}

/// Det lagrede dokumentet for ett skritt — bevis, gjenutstedbart for alltid.
pub async fn reminder_document(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    reminder_id: Uuid,
) -> Result<String> {
    let doc: Option<String> = sqlx::query_scalar(
        "select r.document from invoice_reminder r
         join invoice i on i.id = r.invoice_id
         where r.id = $1 and r.invoice_id = $2 and i.company_id = $3",
    )
    .bind(reminder_id)
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    match doc {
        Some(d) => Ok(d),
        None => bail!("no such reminder"),
    }
}
