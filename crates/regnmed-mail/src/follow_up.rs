//! The mail step of `regnmed abonnement-oppfolging` (docs/abonnement.md
//! §5.3), here rather than in the CLI binary so it can be tested against
//! a real stream: which documents go out is the decision #223 changed.

use anyhow::Result;
use regnmed_db::job::{self, Kind};
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
    send_logged_as(pool, js, Uuid::now_v7(), company_id, payload, sent_by).await
}

/// [`send_logged`] with the caller's id, so a job that is taken over repeats
/// the same queue message instead of creating a second one.
pub async fn send_logged_as(
    pool: &PgPool,
    js: &jetstream::Context,
    id: Uuid,
    company_id: Uuid,
    payload: regnmed_db::EmailPayload,
    sent_by: &str,
) -> Result<()> {
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

/// The mail rail as a job needs it (docs/efaktura.md §3.1).
pub fn job_mail(pool: PgPool, js: jetstream::Context) -> regnmed_db::peppol_worker::SendMail {
    std::sync::Arc::new(move |id, company_id, payload, sent_by| {
        let (pool, js) = (pool.clone(), js.clone());
        Box::pin(async move { send_logged_as(&pool, &js, id, company_id, payload, &sent_by).await })
    })
}

/// Queues every subscription invoice and credit note that has not gone
/// out (paid, credited and open alike). A worker chooses EHF or e-mail,
/// with the customer company's own address for e-mail. One result per
/// document: its job, or why it could not be queued.
pub async fn queue_unsent_invoices(
    pool: &PgPool,
    js: &jetstream::Context,
    drift: Uuid,
) -> Result<Vec<(UnsentInvoice, Result<Uuid>)>> {
    let mut outcomes = Vec::new();
    for f in unsent_invoices(pool, drift).await? {
        let queued = async {
            regnmed_db::peppol_worker::check_sendable(
                pool,
                drift,
                f.invoice_id,
                f.email.as_deref(),
            )
            .await?;
            let job_id = job::create_deduped(
                pool,
                drift,
                Kind::InvoiceDispatch,
                &f.invoice_id.to_string(),
                &serde_json::json!({ "invoice_id": f.invoice_id, "email": f.email }),
                PERFORMED_BY,
                None,
            )
            .await?;
            // The row is the record: a lost nudge is found by the sweep.
            if let Err(e) = crate::jobqueue::nudge(js, job_id).await {
                eprintln!("the job queue rejected the nudge for {job_id}: {e:#}");
            }
            Ok(job_id)
        }
        .await;
        outcomes.push((f, queued));
    }
    Ok(outcomes)
}
