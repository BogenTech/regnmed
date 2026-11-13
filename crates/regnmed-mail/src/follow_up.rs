//! The mail step of `regnmed abonnement-oppfolging` (docs/abonnement.md
//! §5.3), here rather than in the CLI binary so it can be tested against
//! a real stream: which documents go out is the decision #223 changed.

use anyhow::{Context as _, Result};
use regnmed_db::subscription_follow_up::{PERFORMED_BY, UnsentInvoice, unsent_invoices};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{OutboundMail, jetstream, publish};

/// Publishes one mail and records it in the insert-only dispatch log. The
/// log id is the stream's Nats-Msg-Id, so a retry cannot double-deliver.
pub async fn send_logged(
    pool: &PgPool,
    js: &jetstream::Context,
    company_id: Uuid,
    payload: regnmed_db::EmailPayload,
    sent_by: &str,
) -> Result<()> {
    let id = Uuid::now_v7();
    publish(js, &OutboundMail::from_payload(id, &payload)).await?;
    regnmed_db::log_dispatch(
        pool,
        id,
        company_id,
        payload.invoice_id,
        payload.reminder_id,
        payload.invitation_id,
        &payload.to,
        &payload.subject,
        sent_by,
    )
    .await
}

/// Sends every subscription invoice and credit note that never went out
/// (paid, credited and open alike) to the customer company's own address.
/// One result per document: the address it went to, or why it did not go.
pub async fn send_unsent_invoices(
    pool: &PgPool,
    js: &jetstream::Context,
    drift: Uuid,
) -> Result<Vec<(UnsentInvoice, Result<String>)>> {
    let mut outcomes = Vec::new();
    for f in unsent_invoices(pool, drift).await? {
        let sent = async {
            let email = f
                .email
                .clone()
                .context("the company has no e-mail address")?;
            let payload =
                regnmed_db::invoice_email_payload(pool, drift, f.invoice_id, Some(&email)).await?;
            send_logged(pool, js, drift, payload, PERFORMED_BY).await?;
            Ok(email)
        }
        .await;
        outcomes.push((f, sent));
    }
    Ok(outcomes)
}
