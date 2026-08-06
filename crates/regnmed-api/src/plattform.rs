//! Platform roles: systemadmin and support (docs/auth.md §8).
//!
//! The deliberately bounded exception to "no access path crosses a
//! company boundary". Everything a platform role can do lives under
//! `/platform/*` on its own sub-router; the company-scoped guard
//! (`tilgang::krev`) is untouched, and there is NO route from a platform
//! role into any company's ledger — no vouchers, no balances, no reports.
//!
//! The #57 safeguards are enforced structurally, not by convention:
//!
//! - **Logged**: the `vakt` middleware wraps the whole sub-router, so a
//!   `/platform` endpoint that forgets to log cannot exist. The log row
//!   is written BEFORE the handler runs — a refused call still shows the
//!   attempt — and synchronously: if the log insert fails, the call fails.
//! - **Visible**: `/companies/{id}/platform-access` (and the firm twin)
//!   serves the rows that concern that company to its own admins.
//! - **Time-limited**: `platform_member.valid_to` is NOT NULL; expiry and
//!   revocation are immediate (exclusive date, checked per request).
//!
//! No platform role, integration or token gets in here without an active
//! `platform_member` row; strangers get 404, like everywhere else.

use axum::extract::{Path, Query, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

/// What the middleware resolved: who is calling, and as what. Handlers
/// receive it as an `Extension` — its presence proves the call went
/// through the guard and was logged.
#[derive(Clone)]
pub struct PlattformKontekst {
    pub person_id: Uuid,
    pub rolle: String,
}

impl PlattformKontekst {
    fn er_systemadmin(&self) -> bool {
        self.rolle == "systemadmin"
    }

    /// Support may look and assign new memberships; everything else —
    /// customer registers, role changes, platform-user administration —
    /// is systemadmin territory.
    fn krev_systemadmin(&self) -> Result<(), ApiError> {
        if self.er_systemadmin() {
            Ok(())
        } else {
            Err(ApiError::Forbidden("SYSTEMADMIN"))
        }
    }
}

/// The `/platform` sub-router, merged into the main router. The state is
/// applied here so the middleware can carry it.
pub fn plattform_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/platform/members", get(list_members).post(grant_member))
        .route("/platform/members/{member_id}", delete(end_member))
        .route("/platform/overview", get(overview))
        .route("/platform/subscriptions", get(list_subscriptions))
        .route("/platform/settings", get(get_settings).put(put_settings))
        .route("/platform/companies", get(list_companies))
        .route("/platform/companies/{company_id}", get(company_detail))
        .route(
            "/platform/companies/{company_id}/settings",
            axum::routing::put(put_company_settings),
        )
        .route(
            "/platform/companies/{company_id}/members/{person_id}",
            delete(deactivate_member),
        )
        .route(
            "/platform/companies/{company_id}/members/{person_id}/restore",
            post(restore_member),
        )
        .route(
            "/platform/companies/{company_id}/subscription",
            post(start_coverage),
        )
        .route(
            "/platform/companies/{company_id}/subscription/end",
            post(end_coverage),
        )
        .route("/platform/firms", get(list_firms))
        .route("/platform/users", get(list_users))
        .route("/platform/customers", get(list_customers))
        .route(
            "/platform/users/{person_id}/companies/{company_id}",
            post(assign_company),
        )
        .route(
            "/platform/users/{person_id}/firms/{firm_id}",
            post(assign_firm),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, vakt))
}

/// Company/firm ids named in a `/platform/...` path, for the log row.
fn ider_i_sti(path: &str) -> (Option<Uuid>, Option<Uuid>) {
    let mut company = None;
    let mut firm = None;
    let mut segments = path.split('/').filter(|s| !s.is_empty()).peekable();
    while let Some(seg) = segments.next() {
        if let Some(next) = segments.peek() {
            match seg {
                "companies" => company = Uuid::parse_str(next).ok(),
                "firms" => firm = Uuid::parse_str(next).ok(),
                _ => {}
            }
        }
    }
    (company, firm)
}

/// The one seam every `/platform` call passes through: token → person →
/// active platform role → log row → handler. A person without an active
/// role gets 404 — whether `/platform` exists is not theirs to probe.
async fn vakt(State(state): State<AppState>, req: Request, next: Next) -> Response {
    use axum::extract::FromRequestParts;
    let (mut parts, body) = req.into_parts();
    let person = match AuthPerson::from_request_parts(&mut parts, &state).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    // Platform roles are held by humans. A machine token never gets one
    // (also refused at grant time) — refuse here too, so the invariant
    // does not depend on the database alone.
    if person.integration.is_some() {
        return ApiError::NotFound.into_response();
    }
    let rolle = match regnmed_db::active_platform_role(&state.pool, person.person_id).await {
        Ok(Some(r)) => r.rolle,
        Ok(None) => return ApiError::NotFound.into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    let path = parts.uri.path().to_string();
    let (company_id, firm_id) = ider_i_sti(&path);
    if let Err(e) = regnmed_db::log_platform_access(
        &state.pool,
        person.person_id,
        &rolle,
        parts.method.as_str(),
        &path,
        company_id,
        firm_id,
    )
    .await
    {
        // Synchronous and fatal on purpose: an unlogged platform call
        // must not happen, so the call does not happen.
        return ApiError::from(e).into_response();
    }
    parts.extensions.insert(PlattformKontekst {
        person_id: person.person_id,
        rolle,
    });
    next.run(Request::from_parts(parts, body)).await
}

#[derive(Deserialize)]
pub struct SokQuery {
    sok: Option<String>,
}

async fn list_members(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let medlemmer = regnmed_db::list_platform_members(&state.pool).await?;
    Ok(Json(json!({
        "medlemmer": medlemmer.iter().map(|m| json!({
            "id": m.id,
            "person_id": m.person_id,
            "navn": m.navn,
            "epost": m.epost,
            "rolle": m.rolle,
            "valid_from": m.valid_from.to_string(),
            "valid_to": m.valid_to.to_string(),
            "notat": m.notat,
            "aktiv": m.aktiv,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct GrantRequest {
    epost: String,
    rolle: String,
    valid_to: NaiveDate,
    notat: String,
}

async fn grant_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Json(body): Json<GrantRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let person_id = regnmed_db::person_by_email(&state.pool, &body.epost)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
        .ok_or_else(|| {
            ApiError::BadRequest(
                "ingen bruker med den adressen — personen må ha logget inn først".into(),
            )
        })?;
    let id = regnmed_db::grant_platform_role(
        &state.pool,
        person_id,
        &body.rolle,
        body.valid_to,
        &body.notat,
        Some(ctx.person_id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

async fn end_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path(member_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    regnmed_db::end_platform_member(&state.pool, member_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avsluttet": true })))
}

async fn list_companies(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let selskaper = regnmed_db::platform_list_companies(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "selskaper": selskaper.iter().map(|c| json!({
            "company_id": c.id,
            "orgnr": c.orgnr,
            "name": c.name,
        })).collect::<Vec<_>>(),
    })))
}

async fn list_firms(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let byraer = regnmed_db::platform_list_firms(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "byraer": byraer.iter().map(|f| json!({
            "firm_id": f.id,
            "orgnr": f.orgnr,
            "name": f.name,
            "kind": f.kind,
        })).collect::<Vec<_>>(),
    })))
}

async fn list_users(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let brukere = regnmed_db::platform_list_users(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "brukere": brukere.iter().map(|b| json!({
            "person_id": b.person_id,
            "navn": b.navn,
            "epost": b.epost,
            "kind": b.kind,
            "tilknytninger": b.tilknytninger.iter().map(|t| json!({
                "slag": t.slag,
                "id": t.id,
                "navn": t.navn,
                "orgnr": t.orgnr,
                "rolle": t.rolle,
                "aktiv": t.aktiv,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })))
}

/// Customer registers across companies — master data with the owning
/// company named, never balances. Systemadmin only: "it is only System
/// Admins that have access to ALL customers".
async fn list_customers(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let kunder = regnmed_db::platform_list_customers(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "kunder": kunder.iter().map(|k| json!({
            "party_id": k.party_id,
            "party_no": k.party_no,
            "navn": k.navn,
            "orgnr": k.orgnr,
            "epost": k.epost,
            "selskap": {
                "company_id": k.company_id,
                "navn": k.company_navn,
                "orgnr": k.company_orgnr,
            },
        })).collect::<Vec<_>>(),
    })))
}

/// Dashboard counts + the abonnement status distribution. Open to both
/// roles: the same aggregates support already sees as lists, nothing
/// per-company beyond what `/platform/companies` shows.
async fn overview(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tall = regnmed_db::platform_overview(&state.pool).await?;
    let abo = regnmed_db::platform_list_subscriptions(&state.pool).await?;
    let idag = chrono::Utc::now().date_naive();
    let mut fordeling = std::collections::BTreeMap::new();
    for a in &abo {
        let status =
            regnmed_core::abonnement::status(a.opprettet, a.dekket_i_dag, a.siste_slutt, idag);
        *fordeling.entry(status.slug()).or_insert(0i64) += 1;
    }
    Ok(Json(json!({
        "selskaper": tall.selskaper,
        "byraer": tall.byraer,
        "brukere": tall.brukere,
        "integrasjoner": tall.integrasjoner,
        "plattformbrukere": tall.plattformbrukere,
        "abonnement": fordeling,
    })))
}

/// Per-company abonnement status — regnmed's own billing relationship,
/// systemadmin only (the customer-register precedent). The status rule
/// runs here, in one place, over the facts the db fetched set-wise;
/// nothing is stored and no company ledger is read.
async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let abo = regnmed_db::platform_list_subscriptions(&state.pool).await?;
    let idag = chrono::Utc::now().date_naive();
    Ok(Json(json!({
        "abonnementer": abo.iter().map(|a| {
            use regnmed_core::abonnement::Status;
            let status =
                regnmed_core::abonnement::status(a.opprettet, a.dekket_i_dag, a.siste_slutt, idag);
            // The date that matters for THIS status — same shape as /me.
            let dato = match status {
                Status::Aktiv => a.valid_to,
                Status::Prove { til } => Some(til),
                Status::Frist { sperres } => Some(sperres),
                Status::Sperret { siden } => Some(siden),
            };
            json!({
                "company_id": a.company_id,
                "orgnr": a.orgnr,
                "name": a.name,
                "opprettet": a.opprettet.to_string(),
                "plan": a.plan,
                "status": status.slug(),
                "dato": dato.map(|d| d.to_string()),
            })
        }).collect::<Vec<_>>(),
    })))
}

// ---------------------------------------------------------------------
// The back office (docs/auth.md §8): systemadmin's tools for supporting
// customers — editing administrative master data, memberships and the
// abonnement relationship. Everything below runs behind the same `vakt`
// (logged, time-limited role) and reaches NO ledger.
// ---------------------------------------------------------------------

/// Icon styles the portal knows (ui/portal/src/lib/ikoner.js). Locked
/// globally by systemadmin — validated here so a typo cannot blank every
/// menu on the platform.
const IKONSTILER: [&str; 4] = ["linje", "kraftig", "emoji", "ingen"];

async fn get_settings(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ikonstil = regnmed_db::platform_setting(&state.pool, "ikonstil").await?;
    Ok(Json(
        json!({ "ikonstil": ikonstil.unwrap_or_else(|| "linje".into()) }),
    ))
}

#[derive(Deserialize)]
pub struct SettingsRequest {
    ikonstil: String,
}

async fn put_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Json(body): Json<SettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    if !IKONSTILER.contains(&body.ikonstil.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "ukjent ikonstil «{}» — gyldige: {}",
            body.ikonstil,
            IKONSTILER.join(", ")
        )));
    }
    regnmed_db::set_platform_setting(&state.pool, "ikonstil", &body.ikonstil, ctx.person_id)
        .await?;
    Ok(Json(json!({ "updated": true })))
}

/// One company, everything the back office needs on one page: master
/// data, memberships, open invitations and the abonnement relationship.
/// Support may look (same data as the lists plus what the company's own
/// admin sees about itself); editing is systemadmin, guarded per action.
async fn company_detail(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = regnmed_db::company_settings(&state.pool, company_id).await?;
    let medlemmer = regnmed_db::medlemmer::list_medlemmer(&state.pool, company_id).await?;
    let invitasjoner = regnmed_db::medlemmer::list_invitasjoner(&state.pool, company_id).await?;
    let status = regnmed_db::abonnement::status_for(&state.pool, company_id).await?;
    let dekning = regnmed_db::coverage_rows(&state.pool, company_id).await?;
    use regnmed_core::abonnement::Status;
    let dato = match status {
        Status::Aktiv => None,
        Status::Prove { til } => Some(til),
        Status::Frist { sperres } => Some(sperres),
        Status::Sperret { siden } => Some(siden),
    };
    Ok(Json(json!({
        "settings": {
            "name": s.name,
            "orgnr": s.orgnr,
            "address": s.address,
            "bank_account": s.bank_account,
            "orgform": s.orgform,
            "email": s.email,
        },
        "medlemmer": medlemmer.iter().map(|m| json!({
            "person_id": m.person_id,
            "navn": m.navn,
            "epost": m.epost,
            "rolle": m.rolle,
            "aktiv": m.aktiv,
            "via": m.via,
            "kan_endres": m.kan_endres,
        })).collect::<Vec<_>>(),
        "invitasjoner": invitasjoner.iter().map(|i| json!({
            "id": i.id,
            "epost": i.epost,
            "rolle": i.rolle,
            "sist_sendt": i.sist_sendt.map(|t| t.to_rfc3339()),
        })).collect::<Vec<_>>(),
        "abonnement": {
            "status": status.slug(),
            "dato": dato.map(|d| d.to_string()),
            "dekning": dekning.iter().map(|d| json!({
                "plan": d.plan,
                "valid_from": d.valid_from.to_string(),
                "valid_to": d.valid_to.map(|d| d.to_string()),
                "note": d.note,
                "created_by": d.created_by,
            })).collect::<Vec<_>>(),
        },
    })))
}

#[derive(Deserialize)]
pub struct CompanySettingsRequest {
    address: Option<String>,
    bank_account: Option<String>,
    orgform: Option<String>,
    email: Option<String>,
}

/// Edit a company's master data on its behalf (support cases). Same
/// storage and validation as the company's own PUT /settings; nothing
/// here touches the ledger or anything hashed.
async fn put_company_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CompanySettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    regnmed_db::update_company_settings(
        &state.pool,
        company_id,
        request.address.as_deref(),
        request.bank_account.as_deref(),
        request.orgform.as_deref(),
        request.email.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

/// Deactivates a membership with kilde='plattform' in the company's own
/// change log. The last-active-admin guard inside `sett_aktiv` holds
/// here too: the platform must not orphan a company either.
async fn deactivate_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    regnmed_db::medlemmer::sett_aktiv(
        &state.pool,
        company_id,
        person_id,
        false,
        ctx.person_id,
        "plattform",
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "deaktivert": true })))
}

async fn restore_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    regnmed_db::medlemmer::sett_aktiv(
        &state.pool,
        company_id,
        person_id,
        true,
        ctx.person_id,
        "plattform",
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "reaktivert": true })))
}

#[derive(Deserialize)]
pub struct CoverageRequest {
    plan: String,
    /// Mandatory reference: WHY coverage is opened by hand (support
    /// case, agreement, migration). `tegn` refuses an empty one.
    note: String,
}

/// Opens coverage manually — the support case where the card/invoice
/// machinery does not fit. The row carries the reason and who did it.
async fn start_coverage(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CoverageRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let idag = chrono::Utc::now().date_naive();
    let status = regnmed_db::abonnement::status_for(&state.pool, company_id).await?;
    if matches!(status, regnmed_core::abonnement::Status::Aktiv) {
        return Err(ApiError::BadRequest(
            "selskapet har allerede aktiv dekning".into(),
        ));
    }
    regnmed_db::abonnement::tegn(
        &state.pool,
        company_id,
        &request.plan,
        idag,
        None,
        &format!("plattform: {}", request.note),
        &format!("plattform:{}", ctx.person_id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "startet": true })))
}

/// Ends the open coverage today (exclusive, so today is still covered —
/// the shortest truthful coverage is one day). The ordinary frist runs
/// on top before anything is blocked; the row's own note says why it
/// existed, and this call is in the platform access log.
async fn end_coverage(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let idag = chrono::Utc::now().date_naive();
    regnmed_db::abonnement::avslutt(&state.pool, company_id, idag)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avsluttet": true })))
}

#[derive(Deserialize)]
pub struct AssignRequest {
    rolle: String,
}

async fn assign_company(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path((person_id, company_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::platform_assign_company(
        &state.pool,
        company_id,
        person_id,
        &body.rolle,
        ctx.person_id,
        ctx.er_systemadmin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "tildelt": true })))
}

async fn assign_firm(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path((person_id, firm_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::platform_assign_firm(
        &state.pool,
        firm_id,
        person_id,
        &body.rolle,
        ctx.person_id,
        ctx.er_systemadmin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "tildelt": true })))
}

fn innsyn_json(rader: &[regnmed_db::PlattformInnsyn]) -> serde_json::Value {
    json!({
        "innsyn": rader.iter().map(|r| json!({
            "navn": r.navn,
            "rolle": r.rolle,
            "method": r.method,
            "path": r.path,
            "tidspunkt": r.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })
}

/// What the platform did that concerns THIS company, read by the
/// company's own administrators. Lives on the main router behind the
/// ordinary company guard.
pub async fn company_platform_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let rader = regnmed_db::platform_access_for_company(&state.pool, company_id).await?;
    Ok(Json(innsyn_json(&rader)))
}

/// The byrå twin, for firm admins.
pub async fn firm_platform_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::byramedlemmer::require_firm_admin(&state, person.person_id, firm_id).await?;
    let rader = regnmed_db::platform_access_for_firm(&state.pool, firm_id).await?;
    Ok(Json(innsyn_json(&rader)))
}
