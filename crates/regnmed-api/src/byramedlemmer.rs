//! Byrå membership administration (#78, docs/marketplace.md):
//!
//! - GET    /firms/{fid}/access                 members of the firm
//! - PUT    /firms/{fid}/access/{person_id}     change role
//! - DELETE /firms/{fid}/access/{person_id}     deactivate
//! - POST   /firms/{fid}/access/{person_id}/restore
//! - GET    /firms/{fid}/access/history         who let whom in
//! - GET    /firms/{fid}/invitations            open invitations
//! - POST   /firms/{fid}/invitations            invite an e-mail address
//! - DELETE /firms/{fid}/invitations/{iid}      revoke
//! - POST   /firms/{fid}/invitations/{iid}/resend
//!
//! Everything requires an ACTIVE ADMIN of the firm — a firm member
//! reaches every client of the firm, so membership is portfolio access.
//! Non-admins (and strangers) get 404, never 403: the firm's existence
//! is not theirs to probe.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::mailq::{OutboundMail, publish};

pub(crate) async fn require_firm_admin(
    state: &AppState,
    person_id: Uuid,
    firm_id: Uuid,
) -> Result<(), ApiError> {
    if !regnmed_db::byramedlemmer::is_firm_admin(&state.pool, person_id, firm_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

pub async fn list_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    let medlemmer = regnmed_db::byramedlemmer::list_medlemmer(&state.pool, firm_id).await?;
    Ok(Json(json!({
        "medlemmer": medlemmer.iter().map(|m| json!({
            "person_id": m.person_id,
            "navn": m.navn,
            "epost": m.epost,
            "rolle": m.rolle,
            "aktiv": m.aktiv,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct RolleRequest {
    rolle: String,
}

pub async fn set_role(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, person_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RolleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    regnmed_db::byramedlemmer::sett_rolle(
        &state.pool,
        firm_id,
        person_id,
        &request.rolle,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn revoke_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    regnmed_db::byramedlemmer::sett_aktiv(&state.pool, firm_id, person_id, false, person.person_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn restore_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    regnmed_db::byramedlemmer::sett_aktiv(&state.pool, firm_id, person_id, true, person.person_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn access_history(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    let rader = regnmed_db::byramedlemmer::tilgangshistorikk(&state.pool, firm_id).await?;
    Ok(Json(json!({
        "endringer": rader.iter().map(|e| json!({
            "navn": e.navn,
            "endring": e.endring,
            "fra_rolle": e.fra_rolle,
            "til_rolle": e.til_rolle,
            "utfort_av": e.utfort_av,
            "kilde": e.kilde,
            "tidspunkt": e.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn list_invitations(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    let rader = regnmed_db::byramedlemmer::list_invitasjoner(&state.pool, firm_id).await?;
    Ok(Json(json!({
        "invitasjoner": rader.iter().map(|i| json!({
            "id": i.id,
            "epost": i.epost,
            "rolle": i.rolle,
            "invitert_av": i.invitert_av,
            "tidspunkt": i.created_at.to_rfc3339(),
            "sist_sendt": i.sist_sendt.map(|t| t.to_rfc3339()),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct InviteRequest {
    epost: String,
    rolle: String,
}

/// Invites an e-mail address into the firm. As on the company side, the
/// response never says whether the address already has a user with us,
/// and a mail-queue outage never fails the invitation — the invitation
/// IS the grant, the mail only announces it.
pub async fn invite(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
    Json(request): Json<InviteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    let id = regnmed_db::byramedlemmer::inviter(
        &state.pool,
        firm_id,
        &request.epost,
        &request.rolle,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let feil = try_send_firm_invitation(&state, &person, firm_id, id)
        .await
        .err();
    Ok(Json(json!({
        "invitasjon_id": id,
        "epost_sendt": feil.is_none(),
        "epost_grunn": feil,
    })))
}

pub async fn revoke_invitation(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, invitasjon_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    regnmed_db::byramedlemmer::tilbakekall_invitasjon(
        &state.pool,
        firm_id,
        invitasjon_id,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Queues the firm-invitation mail and logs it in the shared utsendelse
/// log; returns why it did not go if it did not (see
/// `utsendelse::try_send_invitation` — same contract).
async fn try_send_firm_invitation(
    state: &AppState,
    person: &AuthPerson,
    firm_id: Uuid,
    invitasjon_id: Uuid,
) -> Result<(), String> {
    let Some(js) = &state.mailq else {
        return Err("e-postutsendelse er ikke konfigurert (NATS_URL)".into());
    };
    let payload = regnmed_db::byramedlemmer::firm_invitation_email_payload(
        &state.pool,
        firm_id,
        invitasjon_id,
        state.portal_base.as_deref(),
    )
    .await
    .map_err(|e| format!("{e:#}"))?;

    let id = Uuid::now_v7();
    publish(js, &OutboundMail::from_payload(id, &payload))
        .await
        .map_err(|e| format!("kunne ikke legge i utsendelseskøen: {e:#}"))?;
    let sent_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::byramedlemmer::log_firm_utsendelse(
        &state.pool,
        id,
        invitasjon_id,
        &payload.to,
        &payload.subject,
        sent_by,
    )
    .await
    .map_err(|e| format!("{e:#}"))?;
    Ok(())
}

pub async fn resend_invitation(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, invitasjon_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_admin(&state, person.person_id, firm_id).await?;
    try_send_firm_invitation(&state, &person, firm_id, invitasjon_id)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(json!({ "epost_sendt": true })))
}
