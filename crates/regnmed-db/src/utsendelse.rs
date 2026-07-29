//! E-postutsendelse (docs/faktura.md, #32): assembles what a mail needs
//! from the database (recipient, subject, body, the stored PDF) and
//! keeps the insert-only utsendelseslogg. The queue publish itself
//! lives in regnmed-api — this crate stays NATS-free.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use regnmed_core::Ore;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::attachment::get_attachment;
use crate::settings::invoice_pdf_attachment_id;

#[derive(Debug)]
pub struct EmailPayload {
    pub to: String,
    pub subject: String,
    pub text: String,
    /// The company's own address — replies go there, never to regnmed.
    pub reply_to: Option<String>,
    /// The document to attach, when there is one. A salgsdokument mail
    /// always carries its PDF; an invitation (#66) carries nothing but
    /// words, so this is None there.
    pub attachment: Option<MailDocument>,
    pub invoice_id: Option<Uuid>,
    pub reminder_id: Option<Uuid>,
    pub invitation_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct MailDocument {
    pub filename: String,
    pub pdf: Vec<u8>,
}

struct MailFacts {
    company_name: String,
    company_email: Option<String>,
    party_email: Option<String>,
    invoice_no: i64,
    due_date: NaiveDate,
    kid: String,
    gross_ore: i64,
    is_credit_note: bool,
}

async fn mail_facts(pool: &PgPool, company_id: Uuid, invoice_id: Uuid) -> Result<MailFacts> {
    let row = sqlx::query(
        "select c.name as company_name, c.email as company_email,
                p.email as party_email, i.invoice_no, i.due_date, i.kid,
                (i.credits_invoice_id is not null) as is_credit_note,
                e.amount_ore as gross_ore
         from invoice i
         join company c on c.id = i.company_id
         join party p on p.id = i.party_id
         join entry e on e.id = i.receivable_entry_id
         where i.id = $1 and i.company_id = $2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice")?;
    Ok(MailFacts {
        company_name: row.get("company_name"),
        company_email: row.get("company_email"),
        party_email: row.get("party_email"),
        invoice_no: row.get("invoice_no"),
        due_date: row.get("due_date"),
        kid: row.get("kid"),
        gross_ore: row.get("gross_ore"),
        is_credit_note: row.get("is_credit_note"),
    })
}

fn recipient(override_to: Option<&str>, party_email: &Option<String>) -> Result<String> {
    match override_to.map(str::trim).filter(|s| !s.is_empty()) {
        Some(to) => Ok(to.to_string()),
        None => party_email
            .clone()
            .filter(|e| !e.is_empty())
            .context("kunden mangler e-postadresse — sett den på parten eller oppgi en"),
    }
}

/// The invoice mail: the stored salgsdokument attached, a short body
/// with beløp/forfall/KID, replies to the company.
pub async fn invoice_email_payload(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    override_to: Option<&str>,
) -> Result<EmailPayload> {
    let facts = mail_facts(pool, company_id, invoice_id).await?;
    let attachment_id = invoice_pdf_attachment_id(pool, company_id, invoice_id)
        .await?
        .context("fakturaen mangler PDF")?;
    let (meta, pdf) = get_attachment(pool, company_id, attachment_id).await?;

    let dokument = if facts.is_credit_note {
        "kreditnota"
    } else {
        "faktura"
    };
    let subject = format!(
        "{} {} fra {}",
        if facts.is_credit_note {
            "Kreditnota"
        } else {
            "Faktura"
        },
        facts.invoice_no,
        facts.company_name
    );
    let mut text = format!(
        "Hei,\n\nvedlagt følger {dokument} {} fra {}.\n\nBeløp:   {} kr\n",
        facts.invoice_no,
        facts.company_name,
        Ore(facts.gross_ore)
    );
    if !facts.is_credit_note {
        text.push_str(&format!(
            "Forfall: {}\nKID:     {}\n",
            facts.due_date, facts.kid
        ));
    }
    text.push_str(&format!("\nMed vennlig hilsen\n{}\n", facts.company_name));

    Ok(EmailPayload {
        to: recipient(override_to, &facts.party_email)?,
        subject,
        text,
        reply_to: facts.company_email.filter(|e| !e.is_empty()),
        attachment: Some(MailDocument {
            filename: meta.filename,
            pdf,
        }),
        invoice_id: Some(invoice_id),
        reminder_id: None,
        invitation_id: None,
    })
}

/// The purring mail: the stored text document rendered to PDF.
pub async fn reminder_email_payload(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    reminder_id: Uuid,
    override_to: Option<&str>,
) -> Result<EmailPayload> {
    let facts = mail_facts(pool, company_id, invoice_id).await?;
    let row = sqlx::query(
        "select steg, document from invoice_reminder where id = $1 and invoice_id = $2",
    )
    .bind(reminder_id)
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?
    .context("no such reminder")?;
    let steg: String = row.get("steg");
    let document: String = row.get("document");
    let tittel = regnmed_core::purring::Steg::parse(&steg)
        .context("ukjent steg")?
        .tittel();

    Ok(EmailPayload {
        to: recipient(override_to, &facts.party_email)?,
        subject: format!(
            "{tittel} — faktura {} fra {}",
            facts.invoice_no, facts.company_name
        ),
        text: format!(
            "Hei,\n\nvedlagt følger {} for faktura {} fra {}.\n\nMed vennlig hilsen\n{}\n",
            tittel.to_lowercase(),
            facts.invoice_no,
            facts.company_name,
            facts.company_name
        ),
        reply_to: facts.company_email.filter(|e| !e.is_empty()),
        attachment: Some(MailDocument {
            filename: format!("{steg}-faktura-{}.pdf", facts.invoice_no),
            pdf: regnmed_core::fakturapdf::render_tekst_pdf(&document),
        }),
        invoice_id: Some(invoice_id),
        reminder_id: Some(reminder_id),
        invitation_id: None,
    })
}

/// The invitation mail (#66).
///
/// **Carries no secret.** The link goes to the portal's front page and
/// nothing else; redemption is still the address logging in through the
/// IdP, so a forwarded invitation grants the forwarder nothing. That is
/// what lets this mail be plain text with no token to leak.
///
/// `portal_base` is the public URL of the portal when one is configured;
/// without it the mail still explains itself and simply omits the link,
/// rather than printing a URL that goes nowhere.
pub async fn invitation_email_payload(
    pool: &PgPool,
    company_id: Uuid,
    invitation_id: Uuid,
    portal_base: Option<&str>,
) -> Result<EmailPayload> {
    let row = sqlx::query(
        "select i.epost, i.role, c.name as company_name, c.email as company_email,
                coalesce(p.name, p.oidc_sub) as invited_by
         from company_invitation i
         join company c on c.id = i.company_id
         join person p on p.id = i.invited_by
         where i.id = $1 and i.company_id = $2
           and i.accepted_at is null and i.revoked_at is null",
    )
    .bind(invitation_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ingen åpen invitasjon med den id-en")?;

    let epost: String = row.get("epost");
    let rolle: String = row.get("role");
    let company_name: String = row.get("company_name");
    let company_email: Option<String> = row.get("company_email");
    let invited_by: String = row.get("invited_by");

    let mut text = format!(
        "Hei,\n\n{invited_by} har gitt deg tilgang til regnskapet til \
         {company_name} i regnmed, med rollen «{rolle}».\n\n"
    );
    match portal_base {
        Some(base) => text.push_str(&format!(
            "Logg inn her, så er tilgangen på plass:\n{}\n\n",
            base.trim_end_matches('/')
        )),
        None => text.push_str("Logg inn i regnmed, så er tilgangen på plass.\n\n"),
    }
    text.push_str(
        "Bruk denne e-postadressen når du logger inn — tilgangen henger på \
         adressen, ikke på denne meldingen, så det hjelper ingen å \
         videresende den.\n\nMed vennlig hilsen\n",
    );
    text.push_str(&company_name);
    text.push('\n');

    Ok(EmailPayload {
        to: epost,
        subject: format!("Du har fått tilgang til {company_name} i regnmed"),
        text,
        reply_to: company_email.filter(|e| !e.is_empty()),
        attachment: None,
        invoice_id: None,
        reminder_id: None,
        invitation_id: Some(invitation_id),
    })
}

/// One row per send — the id IS the queue's Nats-Msg-Id, so the log row
/// and the (deduplicated) queue message are the same event.
#[allow(clippy::too_many_arguments)]
pub async fn log_utsendelse(
    pool: &PgPool,
    id: Uuid,
    company_id: Uuid,
    invoice_id: Option<Uuid>,
    reminder_id: Option<Uuid>,
    invitation_id: Option<Uuid>,
    to_email: &str,
    subject: &str,
    sent_by: &str,
) -> Result<()> {
    if invoice_id.is_none() && reminder_id.is_none() && invitation_id.is_none() {
        bail!("utsendelse must reference an invoice, a reminder or an invitation");
    }
    sqlx::query(
        "insert into utsendelse
             (id, company_id, invoice_id, reminder_id, invitation_id, to_email, subject, sent_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(company_id)
    .bind(invoice_id)
    .bind(reminder_id)
    .bind(invitation_id)
    .bind(to_email)
    .bind(subject)
    .bind(sent_by)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug)]
pub struct UtsendelseRow {
    pub id: Uuid,
    pub reminder_id: Option<Uuid>,
    pub to_email: String,
    pub subject: String,
    pub sent_by: String,
    pub sent_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_utsendelser(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
) -> Result<Vec<UtsendelseRow>> {
    let rows = sqlx::query(
        "select id, reminder_id, to_email, subject, sent_by, created_at
         from utsendelse where company_id = $1 and invoice_id = $2
         order by created_at",
    )
    .bind(company_id)
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| UtsendelseRow {
            id: r.get("id"),
            reminder_id: r.get("reminder_id"),
            to_email: r.get("to_email"),
            subject: r.get("subject"),
            sent_by: r.get("sent_by"),
            sent_at: r.get("created_at"),
        })
        .collect())
}
