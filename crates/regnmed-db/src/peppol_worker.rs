//! What `regnmed work` does for an e-invoice (docs/efaktura.md §3.1).
//!
//! Every provider call in the product goes through here. The API only
//! creates these jobs.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use regnmed_core::peppol::{Channel, DirectoryAnswer, EventKind, choose_channel, delivery_event};
use regnmed_core::peppol::{own_event_key, provider_event_key};
use regnmed_gov::recommand::{
    CREDIT_NOTE_DOCTYPE, DeliveryReport, INVOICE_DOCTYPE, Recommand, SendOutcome,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::job::{Claimed, Runtime};
use crate::peppol::{self, BeginAttempt, ResendPolicy};

/// Publishes a mail under the given dispatch id and logs it. This crate has no NATS dependency, so the
/// process that owns the connection supplies it.
pub type SendMail = Arc<
    dyn Fn(
            Uuid,
            Uuid,
            crate::EmailPayload,
            String,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Serialize, Deserialize)]
pub struct DispatchInput {
    pub invoice_id: Uuid,
    #[serde(default)]
    pub email: Option<String>,
    /// A human's explicit resend of an unresolved attempt.
    #[serde(default)]
    pub resend_unresolved: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LookupInput {
    pub invoice_id: Uuid,
}

struct Facts {
    invoice_no: i64,
    invoice_date: NaiveDate,
    is_credit_note: bool,
    buyer_orgnumber: Option<String>,
    buyer_reference: Option<String>,
    voucher_id: Uuid,
}

struct Route {
    channel: Channel,
    receiver: Option<String>,
    reason: String,
}

impl Facts {
    fn doctype(&self) -> &'static str {
        if self.is_credit_note {
            CREDIT_NOTE_DOCTYPE
        } else {
            INVOICE_DOCTYPE
        }
    }

    fn subject(&self) -> String {
        let kind = if self.is_credit_note {
            "Kreditnota"
        } else {
            "Faktura"
        };
        format!("{kind} {}", self.invoice_no)
    }
}

async fn facts(pool: &PgPool, company_id: Uuid, invoice_id: Uuid) -> Result<Facts> {
    let row = sqlx::query(
        "select i.invoice_no, i.invoice_date, i.voucher_id,
                (i.credits_invoice_id is not null) as is_credit_note,
                nullif(trim(p.orgnr), '') as buyer_orgnumber,
                nullif(trim(i.buyer_reference), '') as buyer_reference
         from invoice i join party p on p.id = i.party_id
         where i.id = $1 and i.company_id = $2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice")?;
    Ok(Facts {
        invoice_no: row.get("invoice_no"),
        invoice_date: row.get("invoice_date"),
        is_credit_note: row.get("is_credit_note"),
        buyer_orgnumber: row.get("buyer_orgnumber"),
        buyer_reference: row.get("buyer_reference"),
        voucher_id: row.get("voucher_id"),
    })
}

fn duty_applies(invoice_date: NaiveDate) -> bool {
    invoice_date >= NaiveDate::from_ymd_opt(2027, 1, 1).expect("valid date")
}

async fn directory(runtime: &Runtime, receiver: &str, doctype: &str) -> DirectoryAnswer {
    if let Some(answer) = runtime.directory_answer {
        return answer;
    }
    let bare = doctype
        .strip_prefix("busdox-docid-qns::")
        .unwrap_or(doctype);
    regnmed_gov::peppol_directory::lookup(receiver, bare).await
}

async fn route(runtime: &Runtime, facts: &Facts) -> Route {
    let Some(orgnumber) = &facts.buyer_orgnumber else {
        return Route {
            channel: Channel::Email,
            receiver: None,
            reason: "Kjøperen har ikke organisasjonsnummer.".into(),
        };
    };
    let receiver = format!("0192:{orgnumber}");
    let doctype = facts.doctype();

    let Some(provider) = &runtime.peppol else {
        if !duty_applies(facts.invoice_date) {
            return Route {
                channel: Channel::Email,
                receiver: None,
                reason: "EHF-utsendelse er ikke slått på ennå.".into(),
            };
        }
        // Without a provider we can still refuse an unlawful PDF.
        let channel = choose_channel(
            Some(false),
            Some(directory(runtime, &receiver, doctype).await),
        );
        let reason = if channel == Channel::Email {
            "Kjøperen tar ikke imot dette dokumentet som EHF."
        } else {
            "Kjøperen kan motta EHF, men EHF-utsendelse er ikke slått på."
        };
        return Route {
            channel,
            receiver: Some(receiver),
            reason: reason.into(),
        };
    };

    let supported = provider.supports_document(&receiver, doctype).await.ok();
    let answer = if supported == Some(false) {
        Some(directory(runtime, &receiver, doctype).await)
    } else {
        None
    };
    let channel = choose_channel(supported, answer);
    let reason = match channel {
        Channel::Peppol => "Kjøperen tar imot EHF.",
        Channel::Email => "Kjøperen tar ikke imot dette dokumentet som EHF.",
        Channel::Hold => {
            "Vi fikk ikke sikkert svar på om kjøperen tar imot EHF. Fakturaen er ikke sendt. Prøv igjen senere."
        }
    };
    Route {
        channel,
        receiver: Some(receiver),
        reason: reason.into(),
    }
}

/// Without BT-10 the EHF carries our customer number. It passes validation,
/// but a buyer that routes on its reference, as the public sector does,
/// cannot place the invoice (docs/efaktura.md §3.4).
fn reference_warning(channel: Channel, buyer_reference: Option<&str>) -> Option<&'static str> {
    (channel == Channel::Peppol && buyer_reference.is_none()).then_some(
        "Fakturaen mangler «Deres referanse». Offentlige kjøpere kan avvise eller ikke finne den.",
    )
}

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Peppol => "peppol",
        Channel::Email => "epost",
        Channel::Hold => "ukjent",
    }
}

/// Refuses another company's or an unknown invoice.
pub async fn check_invoice(pool: &PgPool, company_id: Uuid, invoice_id: Uuid) -> Result<()> {
    facts(pool, company_id, invoice_id).await.map(|_| ())
}

/// What the API can refuse at once, without asking any provider.
pub async fn check_sendable(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    email: Option<&str>,
) -> Result<()> {
    let facts = facts(pool, company_id, invoice_id).await?;
    let email = email.map(str::trim).filter(|e| !e.is_empty());
    if let Some(email) = email {
        let valid = email.split('@').count() == 2
            && !email.starts_with('@')
            && !email.ends_with('@')
            && !email.chars().any(char::is_whitespace);
        if !valid {
            bail!("ugyldig e-postadresse");
        }
    } else if facts.buyer_orgnumber.is_none() {
        // Without an org number the only channel is e-mail.
        let stored: Option<String> = sqlx::query_scalar(
            "select nullif(trim(p.email), '') from invoice i
             join party p on p.id = i.party_id where i.id = $1",
        )
        .bind(invoice_id)
        .fetch_one(pool)
        .await?;
        if stored.is_none() {
            bail!("kunden mangler e-postadresse — sett den på parten eller oppgi en");
        }
    }
    Ok(())
}

const ALREADY_DISPATCHED: &str = "Fakturaen har allerede en EHF-utsendelse.";

pub(crate) async fn run_lookup(pool: &PgPool, job: &Claimed, runtime: &Runtime) -> Result<Value> {
    let input: LookupInput = crate::job::json_input(pool, job.id).await?;
    let facts = facts(pool, job.company_id, input.invoice_id).await?;
    // The send continues an existing dispatch, so the dialog must say so.
    if let Some(existing) = peppol::latest_dispatch(pool, job.company_id, input.invoice_id).await? {
        return Ok(json!({
            "kanal": channel_name(Channel::Peppol),
            "grunn": ALREADY_DISPATCHED,
            "mottaker": existing.receiver,
            "advarsel": reference_warning(Channel::Peppol, facts.buyer_reference.as_deref()),
        }));
    }
    let route = route(runtime, &facts).await;
    Ok(json!({
        "kanal": channel_name(route.channel),
        "grunn": route.reason,
        "mottaker": route.receiver,
        "advarsel": reference_warning(route.channel, facts.buyer_reference.as_deref()),
    }))
}

pub(crate) async fn run_dispatch(pool: &PgPool, job: &Claimed, runtime: &Runtime) -> Result<Value> {
    let input: DispatchInput = crate::job::json_input(pool, job.id).await?;
    let facts = facts(pool, job.company_id, input.invoice_id).await?;

    // A takeover of a job whose mail already went out is finished.
    if crate::dispatch_exists(pool, job.id).await? {
        return Ok(json!({ "kanal": "epost", "grunn": "E-posten var allerede sendt." }));
    }

    // A takeover or a repeated click must see an earlier attempt before a
    // fresh lookup can route the same invoice down another channel.
    if let Some(existing) = peppol::latest_dispatch(pool, job.company_id, input.invoice_id).await? {
        let receiver = existing.receiver.clone();
        return send_peppol(
            pool,
            job,
            runtime,
            &input,
            &facts,
            Some(existing),
            receiver,
            ALREADY_DISPATCHED,
        )
        .await;
    }

    let route = route(runtime, &facts).await;
    match route.channel {
        Channel::Hold => bail!("{}", route.reason),
        Channel::Email => {
            let send = runtime
                .mail
                .as_ref()
                .context("e-postutsendelse er ikke konfigurert (NATS_URL)")?;
            let payload = crate::invoice_email_payload(
                pool,
                job.company_id,
                input.invoice_id,
                input.email.as_deref(),
            )
            .await?;
            let (to, subject) = (payload.to.clone(), payload.subject.clone());
            // The job id is the Nats-Msg-Id: a takeover inside the stream's
            // duplicate window cannot deliver a second mail.
            send(job.id, job.company_id, payload, job.created_by.clone()).await?;
            Ok(json!({ "kanal": "epost", "to": to, "subject": subject, "grunn": route.reason }))
        }
        Channel::Peppol => {
            let receiver = route.receiver.context("a Peppol route has a receiver")?;
            send_peppol(
                pool,
                job,
                runtime,
                &input,
                &facts,
                None,
                receiver,
                &route.reason,
            )
            .await
        }
    }
}

async fn verified_sender(pool: &PgPool, provider: &Recommand, company_id: Uuid) -> Result<String> {
    let Some(sender) = peppol::load_sender(pool, company_id).await? else {
        bail!("Selskapet må aktiveres for EHF før fakturaen kan sendes.");
    };
    if sender.is_verified {
        return Ok(sender.provider_company_id);
    }
    // The webhook can be lost, so ask before telling a user to verify again.
    if provider.is_verified(&sender.provider_company_id).await? {
        peppol::set_sender_verified(pool, company_id, true).await?;
        return Ok(sender.provider_company_id);
    }
    bail!("En som kan signere for selskapet må fullføre verifiseringen før EHF kan sendes.");
}

/// The XML to send. Invoices from before #222 have none stored, so their
/// one rendering is stored here and reused by every later attempt.
async fn xml_attachment(
    pool: &PgPool,
    job: &Claimed,
    invoice_id: Uuid,
    facts: &Facts,
) -> Result<Uuid> {
    if let Some(id) =
        crate::settings::issued_attachment_id(pool, job.company_id, invoice_id, "application/xml")
            .await?
    {
        return Ok(id);
    }
    let xml = crate::invoice_ehf(pool, job.company_id, invoice_id).await?;
    let meta = crate::add_attachment(
        pool,
        job.company_id,
        facts.voucher_id,
        &format!("ehf-{}.xml", facts.invoice_no),
        "application/xml",
        xml.as_bytes(),
        &job.created_by,
    )
    .await?;
    Ok(meta.id)
}

fn dispatch_result(
    dispatch: &peppol::PeppolDispatch,
    reason: &str,
    attempt: i32,
    status: EventKind,
    detail: &str,
) -> Value {
    json!({
        "kanal": "peppol",
        "mottaker": dispatch.receiver,
        "utsendelse_id": dispatch.id,
        "forsok": attempt,
        "status": status.as_str(),
        "detalj": detail,
        "grunn": reason,
    })
}

#[allow(clippy::too_many_arguments)]
async fn send_peppol(
    pool: &PgPool,
    job: &Claimed,
    runtime: &Runtime,
    input: &DispatchInput,
    facts: &Facts,
    existing: Option<peppol::PeppolDispatch>,
    receiver: String,
    reason: &str,
) -> Result<Value> {
    // What already happened is answered first: it needs no provider, and a
    // revoked verification must not hide that an invoice was delivered.
    if let Some(dispatch) = &existing
        && let Some((attempt, status)) = peppol::latest_status(pool, dispatch.id).await?
    {
        let may_resend = input.resend_unresolved
            && matches!(status, EventKind::Unresolved | EventKind::DeliveryFailed);
        if status == EventKind::SendAttempt {
            // A previous worker may have died after its POST.
            own_event(pool, dispatch.id, attempt, EventKind::Unresolved, "").await?;
            return Ok(dispatch_result(
                dispatch,
                reason,
                attempt,
                EventKind::Unresolved,
                "",
            ));
        }
        if !status.is_safe_to_retry() && !may_resend {
            return Ok(dispatch_result(dispatch, reason, attempt, status, ""));
        }
    }
    let provider = runtime
        .peppol
        .as_deref()
        .context("EHF-utsendelse er ikke konfigurert")?;
    let sender = verified_sender(pool, provider, job.company_id).await?;
    let dispatch = match existing {
        Some(dispatch) => dispatch,
        None => {
            let attachment = xml_attachment(pool, job, input.invoice_id, facts).await?;
            peppol::find_or_create_dispatch(
                pool,
                job.company_id,
                input.invoice_id,
                &receiver,
                attachment,
                &facts.subject(),
                &job.created_by,
            )
            .await?
        }
    };
    let (_, bytes) = crate::get_attachment(pool, job.company_id, dispatch.attachment_id).await?;
    let xml = String::from_utf8(bytes).context("stored EHF is not UTF-8")?;
    let result = |attempt: i32, status: EventKind, detail: &str| {
        dispatch_result(&dispatch, reason, attempt, status, detail)
    };

    let mut policy = ResendPolicy::SafeOnly;
    if input.resend_unresolved
        && let Some((attempt, status @ (EventKind::Unresolved | EventKind::DeliveryFailed))) =
            peppol::latest_status(pool, dispatch.id).await?
    {
        let hash: [u8; 32] = Sha256::digest(xml.as_bytes()).into();
        let from = peppol::first_attempt_at(pool, dispatch.id)
            .await?
            .context("an unresolved dispatch has an attempt")?;
        let linked = peppol::linked_documents(pool, dispatch.id).await?;
        let found: Vec<_> = provider
            .find_outgoing_by_sha256(&sender, from, &hash)
            .await?
            .into_iter()
            .filter(|id| !linked.contains(id))
            .collect();
        let has_document = peppol::attempt_has_document(pool, dispatch.id, attempt).await?;
        match found.as_slice() {
            [] => policy = ResendPolicy::AllowAgedUnresolved,
            [document] if !has_document => {
                adopt_document(pool, provider, dispatch.id, attempt, document).await?;
                let (attempt, status) = peppol::latest_status(pool, dispatch.id)
                    .await?
                    .context("the adopted attempt has a status")?;
                return Ok(result(attempt, status, "Dokumentet var allerede sendt."));
            }
            many => {
                return Ok(result(
                    attempt,
                    status,
                    &format!("Fant {} mulige kopier. Kontakt support.", many.len()),
                ));
            }
        }
    }

    let attempt = match peppol::begin_attempt(pool, dispatch.id, policy).await? {
        BeginAttempt::Started(attempt) => attempt,
        // A previous worker may have died after its POST.
        BeginAttempt::Blocked {
            attempt,
            status: EventKind::SendAttempt,
        } => {
            own_event(pool, dispatch.id, attempt, EventKind::Unresolved, "").await?;
            return Ok(result(attempt, EventKind::Unresolved, ""));
        }
        BeginAttempt::Blocked { attempt, status } => return Ok(result(attempt, status, "")),
    };

    match provider
        .send_xml(&sender, &dispatch.receiver, &xml, facts.doctype())
        .await
    {
        SendOutcome::Accepted {
            document_id,
            delivery,
        } => {
            peppol::link_provider_document(pool, &document_id, dispatch.id, attempt).await?;
            provider_event(
                pool,
                dispatch.id,
                attempt,
                &document_id,
                EventKind::Accepted,
            )
            .await?;
            apply_delivery(pool, dispatch.id, attempt, &document_id, &delivery).await?;
            let (_, status) = peppol::latest_status(pool, dispatch.id)
                .await?
                .context("the attempt has a status")?;
            Ok(result(attempt, status, ""))
        }
        SendOutcome::Rejected(detail) => {
            own_event(pool, dispatch.id, attempt, EventKind::Rejected, &detail).await?;
            Ok(result(attempt, EventKind::Rejected, &detail))
        }
        SendOutcome::NotSent(detail) => {
            own_event(pool, dispatch.id, attempt, EventKind::NotSent, &detail).await?;
            Ok(result(attempt, EventKind::NotSent, &detail))
        }
        SendOutcome::Unresolved(detail) => {
            own_event(pool, dispatch.id, attempt, EventKind::Unresolved, &detail).await?;
            Ok(result(attempt, EventKind::Unresolved, &detail))
        }
    }
}

async fn own_event(
    pool: &PgPool,
    dispatch_id: Uuid,
    attempt: i32,
    kind: EventKind,
    detail: &str,
) -> Result<()> {
    peppol::record_event(
        pool,
        dispatch_id,
        attempt,
        kind,
        &own_event_key(dispatch_id, attempt, kind),
        detail,
    )
    .await?;
    Ok(())
}

async fn provider_event(
    pool: &PgPool,
    dispatch_id: Uuid,
    attempt: i32,
    document_id: &str,
    kind: EventKind,
) -> Result<()> {
    peppol::record_event(
        pool,
        dispatch_id,
        attempt,
        kind,
        &provider_event_key(document_id, kind),
        "",
    )
    .await?;
    Ok(())
}

async fn apply_delivery(
    pool: &PgPool,
    dispatch_id: Uuid,
    attempt: i32,
    document_id: &str,
    delivery: &DeliveryReport,
) -> Result<()> {
    let Some(kind) = delivery_event(
        delivery.status.as_deref(),
        delivery.failure_category.as_deref(),
    ) else {
        return Ok(());
    };
    peppol::record_event(
        pool,
        dispatch_id,
        attempt,
        kind,
        &provider_event_key(document_id, kind),
        &delivery.failure_message,
    )
    .await?;
    Ok(())
}

async fn adopt_document(
    pool: &PgPool,
    provider: &Recommand,
    dispatch_id: Uuid,
    attempt: i32,
    document_id: &str,
) -> Result<()> {
    peppol::link_provider_document(pool, document_id, dispatch_id, attempt).await?;
    provider_event(pool, dispatch_id, attempt, document_id, EventKind::Accepted).await?;
    let document = provider.document(document_id).await?;
    apply_delivery(pool, dispatch_id, attempt, document_id, &document.delivery).await
}

/// Registers the company as a send-only participant, or reports where its
/// verification stands. Repeating it never creates a second sender.
pub(crate) async fn run_sender_setup(
    pool: &PgPool,
    job: &Claimed,
    runtime: &Runtime,
) -> Result<Value> {
    let provider = runtime
        .peppol
        .as_deref()
        .context("EHF-utsendelse er ikke konfigurert")?;
    let (provider_id, mut is_verified, mut url) = match peppol::load_sender(pool, job.company_id)
        .await?
    {
        Some(sender) => (sender.provider_company_id, sender.is_verified, None),
        None => {
            let settings = crate::company_settings(pool, job.company_id).await?;
            let (street, postal_code, city) = crate::ehf::split_address(settings.address);
            let (Some(street), Some(postal_code), Some(city)) = (street, postal_code, city) else {
                bail!("Selskapet må ha adresse på formen «Gate 1, 0150 Oslo».");
            };
            // A crash after the provider call must not create a second one.
            let created = match provider.find_sender(&settings.orgnumber).await? {
                Some(sender) => sender,
                None => {
                    provider
                        .create_sender(
                            &settings.name,
                            &street,
                            &postal_code,
                            &city,
                            &settings.orgnumber,
                            None,
                        )
                        .await?
                }
            };
            peppol::insert_sender(
                pool,
                job.company_id,
                &created.provider_company_id,
                created.is_verified,
            )
            .await?;
            (
                created.provider_company_id,
                created.is_verified,
                created.verification_url,
            )
        }
    };
    if !is_verified {
        is_verified = provider.is_verified(&provider_id).await?;
        if is_verified {
            peppol::set_sender_verified(pool, job.company_id, true).await?;
        } else if url.is_none() {
            url = Some(provider.verification_url(&provider_id).await?);
        }
    }
    Ok(json!({
        "er_verifisert": is_verified,
        "verifiseringslenke": if is_verified { None } else { url },
    }))
}

/// What the queue should do with a provider event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAction {
    Ack,
    /// Temporary: deliver it again after a delay.
    Retry,
    /// It can never be processed.
    Drop,
}

const MISSING_LINK_DELIVERIES: i64 = 5;

/// One signed webhook event, as a worker pulls it from the queue. Commits
/// before answering `Ack`, and uses no provider call.
pub async fn handle_provider_event(
    pool: &PgPool,
    team_id: Option<&str>,
    body: &[u8],
    delivered: i64,
) -> EventAction {
    let Ok(event) = serde_json::from_slice::<Value>(body) else {
        eprintln!("peppol event: unreadable body");
        return EventAction::Drop;
    };
    let Some(kind) = event["eventType"].as_str() else {
        eprintln!("peppol event: no eventType");
        return EventAction::Drop;
    };
    if let Some(team) = team_id
        && event["teamId"].as_str() != Some(team)
    {
        eprintln!("peppol event: another team's {kind}, ignored");
        return EventAction::Ack;
    }
    let outcome = match kind {
        "document.sent" => {
            let delivery = DeliveryReport {
                status: event["deliveryStatus"].as_str().map(str::to_string),
                ..Default::default()
            };
            document_event(pool, &event, &delivery, delivered).await
        }
        "document.delivery_status_changed" if event["channel"] == "peppol" => {
            let delivery = DeliveryReport {
                status: event["status"].as_str().map(str::to_string),
                failure_category: event["failure"]["category"].as_str().map(str::to_string),
                failure_message: event["failure"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            };
            document_event(pool, &event, &delivery, delivered).await
        }
        "company.verification" => match event["companyId"].as_str() {
            Some(company) => {
                peppol::set_verified_by_provider_id(pool, company, event["status"] == "verified")
                    .await
                    .map(|()| EventAction::Ack)
            }
            None => Ok(EventAction::Drop),
        },
        // Receiving is phase 3; other event types are not ours to act on.
        _ => Ok(EventAction::Ack),
    };
    outcome.unwrap_or_else(|e| {
        eprintln!("peppol event {kind}: {e:#}");
        EventAction::Retry
    })
}

async fn document_event(
    pool: &PgPool,
    event: &Value,
    delivery: &DeliveryReport,
    delivered: i64,
) -> Result<EventAction> {
    let Some(document) = event["documentId"].as_str() else {
        return Ok(EventAction::Drop);
    };
    let Some((dispatch, attempt)) = peppol::linked_attempt(pool, document).await? else {
        // The webhook can beat the send response that writes the link.
        // After that it is not ours, or reconciliation will pick it up.
        return Ok(if delivered < MISSING_LINK_DELIVERIES {
            EventAction::Retry
        } else {
            EventAction::Ack
        });
    };
    provider_event(pool, dispatch, attempt, document, EventKind::Accepted).await?;
    apply_delivery(pool, dispatch, attempt, document, delivery).await?;
    Ok(EventAction::Ack)
}

/// Long enough that a live worker's POST (60 s timeout) has finished.
const STALE_ATTEMPT_SECS: i64 = 10 * 60;
/// How long an unresolved attempt is searched for automatically. A human
/// resend always searches again.
const UNRESOLVED_SEARCH_HOURS: i64 = 24;
const RECONCILE_BATCH: i64 = 50;
const RECONCILE_LOCK: i64 = 0x5045_5050_4f4c;

/// The webhook is a shortcut, this is the truth (docs/efaktura.md §3.3).
/// One replica at a time, and every write is idempotent. Returns how many
/// open dispatches it looked at.
pub async fn reconcile(pool: &PgPool, provider: &Recommand) -> Result<usize> {
    let mut lock = pool.acquire().await?;
    let locked: bool = sqlx::query_scalar("select pg_try_advisory_lock($1)")
        .bind(RECONCILE_LOCK)
        .fetch_one(&mut *lock)
        .await?;
    if !locked {
        return Ok(0);
    }
    let outcome = reconcile_batch(pool, provider).await;
    // The connection goes back to the pool, so the lock must not stay on it.
    sqlx::query("select pg_advisory_unlock($1)")
        .bind(RECONCILE_LOCK)
        .execute(&mut *lock)
        .await?;
    outcome
}

async fn reconcile_batch(pool: &PgPool, provider: &Recommand) -> Result<usize> {
    let now = chrono::Utc::now();
    let (mut after, mut seen) = (None, 0);
    loop {
        let page = peppol::open_dispatches(pool, after, RECONCILE_BATCH).await?;
        for dispatch in &page {
            // One failing dispatch must not stop the others.
            if let Err(e) = reconcile_one(pool, provider, dispatch, now).await {
                eprintln!("reconcile dispatch {}: {e:#}", dispatch.id);
            }
        }
        seen += page.len();
        if (page.len() as i64) < RECONCILE_BATCH {
            return Ok(seen);
        }
        after = page.last().map(|dispatch| dispatch.id);
    }
}

async fn reconcile_one(
    pool: &PgPool,
    provider: &Recommand,
    dispatch: &peppol::OpenDispatch,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    if let Some(document) = &dispatch.document_id {
        let current = provider.document(document).await?;
        return apply_delivery(
            pool,
            dispatch.id,
            dispatch.attempt,
            document,
            &current.delivery,
        )
        .await;
    }
    let age = now - dispatch.attempt_started;
    if dispatch.status == EventKind::SendAttempt {
        if age.num_seconds() >= STALE_ATTEMPT_SECS {
            own_event(
                pool,
                dispatch.id,
                dispatch.attempt,
                EventKind::Unresolved,
                "",
            )
            .await?;
        } else {
            return Ok(());
        }
    }
    if age.num_hours() >= UNRESOLVED_SEARCH_HOURS {
        return Ok(());
    }
    let Some(sender) = peppol::load_sender(pool, dispatch.company_id).await? else {
        return Ok(());
    };
    let (_, xml) = crate::get_attachment(pool, dispatch.company_id, dispatch.attachment_id).await?;
    let hash: [u8; 32] = Sha256::digest(&xml).into();
    let from = peppol::first_attempt_at(pool, dispatch.id)
        .await?
        .context("an open dispatch has an attempt")?;
    let linked = peppol::linked_documents(pool, dispatch.id).await?;
    let found: Vec<_> = provider
        .find_outgoing_by_sha256(&sender.provider_company_id, from, &hash)
        .await?
        .into_iter()
        .filter(|id| !linked.contains(id))
        .collect();
    match found.as_slice() {
        [] => Ok(()),
        [document] => adopt_document(pool, provider, dispatch.id, dispatch.attempt, document).await,
        many => {
            eprintln!(
                "reconcile dispatch {}: {} possible duplicates, needs a human",
                dispatch.id,
                many.len()
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_ehf_without_a_buyer_reference_is_warned_about() {
        assert!(reference_warning(Channel::Peppol, None).is_some());
        assert!(reference_warning(Channel::Peppol, Some("PO-1")).is_none());
        assert!(reference_warning(Channel::Email, None).is_none());
        assert!(reference_warning(Channel::Hold, None).is_none());
    }
}
