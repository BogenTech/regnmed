//! Platform roles: systemadmin and support (docs/auth.md §8).
//!
//! The deliberately bounded exception to "no access path crosses a
//! company boundary". Everything a platform role can do lives under
//! `/platform/*` on its own sub-router; the company-scoped guard
//! (`access::krev`) is untouched, and there is NO route from a platform
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
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Extension, Json, Router};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

/// What the middleware resolved: who is calling, and as what. Handlers
/// receive it as an `Extension` — its presence proves the call went
/// through the guard and was logged.
#[derive(Clone)]
pub struct PlatformContext {
    pub person_id: Uuid,
    pub role: String,
}

impl PlatformContext {
    fn er_systemadmin(&self) -> bool {
        self.role == "systemadmin"
    }

    /// Support may look and assign new memberships; everything else —
    /// customer registers, role changes, platform-user administration —
    /// is systemadmin territory.
    fn require_systemadmin(&self) -> Result<(), ApiError> {
        if self.er_systemadmin() {
            Ok(())
        } else {
            Err(ApiError::Forbidden("SYSTEMADMIN"))
        }
    }
}

/// The `/platform` sub-router, merged into the main router. The state is
/// applied here so the middleware can carry it.
pub fn platform_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/platform/members", get(list_members).post(grant_member))
        .route("/platform/members/{member_id}", delete(end_member))
        .route("/platform/overview", get(overview))
        .route("/platform/subscriptions", get(list_subscriptions))
        .route("/platform/settings", get(get_settings).put(put_settings))
        .route("/platform/companies/{company_id}/addons", post(grant_addon))
        .route(
            "/platform/companies/{company_id}/addons/{addon}/end",
            post(end_addon),
        )
        .route("/platform/plans", get(list_plans))
        .route("/platform/plans/{slug}", put(put_plan))
        .route("/platform/plans/{slug}/prices", post(post_price))
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
    let role = match regnmed_db::active_platform_role(&state.pool, person.person_id).await {
        Ok(Some(r)) => r.role,
        Ok(None) => return ApiError::NotFound.into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    let path = parts.uri.path().to_string();
    let (company_id, firm_id) = ider_i_sti(&path);
    if let Err(e) = regnmed_db::log_platform_access(
        &state.pool,
        person.person_id,
        &role,
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
    parts.extensions.insert(PlatformContext {
        person_id: person.person_id,
        role,
    });
    next.run(Request::from_parts(parts, body)).await
}

#[derive(Deserialize)]
pub struct SokQuery {
    sok: Option<String>,
}

async fn list_members(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    let members = regnmed_db::list_platform_members(&state.pool).await?;
    Ok(Json(json!({
        "medlemmer": members.iter().map(|m| json!({
            "id": m.id,
            "person_id": m.person_id,
            "navn": m.name,
            "epost": m.email,
            "rolle": m.role,
            "valid_from": m.valid_from.to_string(),
            "valid_to": m.valid_to.to_string(),
            "notat": m.note,
            "aktiv": m.active,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct GrantRequest {
    #[serde(rename = "epost")]
    email: String,
    #[serde(rename = "rolle")]
    role: String,
    valid_to: NaiveDate,
    #[serde(rename = "notat")]
    note: String,
}

async fn grant_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Json(body): Json<GrantRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    let person_id = regnmed_db::person_by_email(&state.pool, &body.email)
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
        &body.role,
        body.valid_to,
        &body.note,
        Some(ctx.person_id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

async fn end_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path(member_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::end_platform_member(&state.pool, member_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avsluttet": true })))
}

async fn list_companies(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlatformContext>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let selskaper = regnmed_db::platform_list_companies(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "selskaper": selskaper.iter().map(|c| json!({
            "company_id": c.id,
            "orgnr": c.orgnumber,
            "name": c.name,
        })).collect::<Vec<_>>(),
    })))
}

async fn list_firms(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlatformContext>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let byraer = regnmed_db::platform_list_firms(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "byraer": byraer.iter().map(|f| json!({
            "firm_id": f.id,
            "orgnr": f.orgnumber,
            "name": f.name,
            "kind": f.kind,
        })).collect::<Vec<_>>(),
    })))
}

async fn list_users(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlatformContext>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let brukere = regnmed_db::platform_list_users(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "brukere": brukere.iter().map(|b| json!({
            "person_id": b.person_id,
            "navn": b.name,
            "epost": b.email,
            "kind": b.kind,
            "tilknytninger": b.tilknytninger.iter().map(|t| json!({
                "slag": t.slag,
                "id": t.id,
                "navn": t.name,
                "orgnr": t.orgnumber,
                "rolle": t.role,
                "aktiv": t.active,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })))
}

/// Customer registers across companies — master data with the owning
/// company named, never balances. Systemadmin only: "it is only System
/// Admins that have access to ALL customers".
async fn list_customers(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    let customers = regnmed_db::platform_list_customers(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "kunder": customers.iter().map(|k| json!({
            "party_id": k.party_id,
            "party_no": k.party_no,
            "navn": k.name,
            "orgnr": k.orgnumber,
            "epost": k.email,
            "selskap": {
                "company_id": k.company_id,
                "navn": k.company_name,
                "orgnr": k.company_orgnumber,
            },
        })).collect::<Vec<_>>(),
    })))
}

/// Dashboard counts + the abonnement status distribution. Open to both
/// roles: the same aggregates support already sees as lists, nothing
/// per-company beyond what `/platform/companies` shows.
async fn overview(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlatformContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let figures = regnmed_db::platform_overview(&state.pool).await?;
    let abo = regnmed_db::platform_list_subscriptions(&state.pool).await?;
    let today = chrono::Utc::now().date_naive();
    let mut distribution = std::collections::BTreeMap::new();
    for a in &abo {
        let status =
            regnmed_core::subscription::status(a.created, a.covered_today, a.last_end, today);
        *distribution.entry(status.slug()).or_insert(0i64) += 1;
    }
    Ok(Json(json!({
        "selskaper": figures.selskaper,
        "byraer": figures.byraer,
        "brukere": figures.brukere,
        "integrasjoner": figures.integrasjoner,
        "plattformbrukere": figures.plattformbrukere,
        "abonnement": distribution,
    })))
}

/// Per-company abonnement status — regnmed's own billing relationship,
/// systemadmin only (the customer-register precedent). The status rule
/// runs here, in one place, over the facts the db fetched set-wise;
/// nothing is stored and no company ledger is read.
async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    let abo = regnmed_db::platform_list_subscriptions(&state.pool).await?;
    let today = chrono::Utc::now().date_naive();
    Ok(Json(json!({
        "abonnementer": abo.iter().map(|a| {
            use regnmed_core::subscription::Status;
            let status =
                regnmed_core::subscription::status(a.created, a.covered_today, a.last_end, today);
            // The date that matters for THIS status — same shape as /me.
            let date = match status {
                Status::Active => a.valid_to,
                Status::Trial { to } => Some(to),
                Status::Grace { blocked_from } => Some(blocked_from),
                Status::Blocked { since } => Some(since),
            };
            json!({
                "company_id": a.company_id,
                "orgnr": a.orgnumber,
                "name": a.name,
                "opprettet": a.created.to_string(),
                "plan": a.plan,
                "status": status.slug(),
                "dato": date.map(|d| d.to_string()),
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
    Extension(_ctx): Extension<PlatformContext>,
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
    Extension(ctx): Extension<PlatformContext>,
    Json(body): Json<SettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
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
    Extension(_ctx): Extension<PlatformContext>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = regnmed_db::company_settings(&state.pool, company_id).await?;
    let members = regnmed_db::members::list_members(&state.pool, company_id).await?;
    let invitations = regnmed_db::members::list_invitations(&state.pool, company_id).await?;
    let status = regnmed_db::subscription::status_for(&state.pool, company_id).await?;
    let dekning = regnmed_db::coverage_rows(&state.pool, company_id).await?;
    use regnmed_core::subscription::Status;
    let date = match status {
        Status::Active => None,
        Status::Trial { to } => Some(to),
        Status::Grace { blocked_from } => Some(blocked_from),
        Status::Blocked { since } => Some(since),
    };
    // Add-ons: what the company has, and what exists that it does not have.
    let held = regnmed_db::bank_feed::addon_rows(&state.pool, company_id).await?;
    let addons: Vec<_> = regnmed_db::plans::list_plans_with(&state.pool, false)
        .await?
        .into_iter()
        .filter(|p| p.kind == "addon")
        .collect();
    let addon_name = |slug: &str| {
        addons
            .iter()
            .find(|p| p.slug == slug)
            .map_or_else(|| slug.to_string(), |p| p.name.clone())
    };
    Ok(Json(json!({
        "tillegg": held.iter().map(|(addon, from, note)| json!({
            "addon": addon, "navn": addon_name(addon), "fra": from.to_string(), "note": note,
        })).collect::<Vec<_>>(),
        "tillegg_mulige": addons.iter()
            .filter(|p| !held.iter().any(|(a, _, _)| a == &p.slug))
            .map(|p| json!({ "addon": p.slug, "navn": p.name }))
            .collect::<Vec<_>>(),
        "settings": {
            "name": s.name,
            "orgnr": s.orgnumber,
            "address": s.address,
            "bank_account": s.bank_account,
            "orgform": s.org_form,
            "email": s.email,
        },
        "medlemmer": members.iter().map(|m| json!({
            "person_id": m.person_id,
            "navn": m.name,
            "epost": m.email,
            "rolle": m.role,
            "aktiv": m.active,
            "via": m.via,
            "kan_endres": m.kan_endres,
        })).collect::<Vec<_>>(),
        "invitasjoner": invitations.iter().map(|i| json!({
            "id": i.id,
            "epost": i.email,
            "rolle": i.role,
            "sist_sendt": i.sist_sendt.map(|t| t.to_rfc3339()),
        })).collect::<Vec<_>>(),
        "abonnement": {
            "status": status.slug(),
            "dato": date.map(|d| d.to_string()),
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
    #[serde(rename = "orgform")]
    org_form: Option<String>,
    email: Option<String>,
}

/// Edit a company's master data on its behalf (support cases). Same
/// storage and validation as the company's own PUT /settings; nothing
/// here touches the ledger or anything hashed.
async fn put_company_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CompanySettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::update_company_settings(
        &state.pool,
        company_id,
        request.address.as_deref(),
        request.bank_account.as_deref(),
        request.org_form.as_deref(),
        request.email.as_deref(),
        None,
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
    Extension(ctx): Extension<PlatformContext>,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::members::set_active(
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
    Extension(ctx): Extension<PlatformContext>,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::members::set_active(
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
    /// case, agreement, migration). `open_coverage` refuses an empty one.
    note: String,
}

/// Opens coverage manually — the support case where the card/invoice
/// machinery does not fit. The row carries the reason and who did it.
async fn start_coverage(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CoverageRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    let today = chrono::Utc::now().date_naive();
    let status = regnmed_db::subscription::status_for(&state.pool, company_id).await?;
    if matches!(status, regnmed_core::subscription::Status::Active) {
        return Err(ApiError::BadRequest(
            "selskapet har allerede aktiv dekning".into(),
        ));
    }
    regnmed_db::subscription::open_coverage(
        &state.pool,
        company_id,
        &request.plan,
        today,
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
    Extension(ctx): Extension<PlatformContext>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.require_systemadmin()?;
    let today = chrono::Utc::now().date_naive();
    regnmed_db::subscription::close_coverage(&state.pool, company_id, today)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avsluttet": true })))
}

#[derive(Deserialize)]
pub struct AssignRequest {
    #[serde(rename = "rolle")]
    role: String,
}

async fn assign_company(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path((person_id, company_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::platform_assign_company(
        &state.pool,
        company_id,
        person_id,
        &body.role,
        ctx.person_id,
        ctx.er_systemadmin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "tildelt": true })))
}

async fn assign_firm(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path((person_id, firm_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::platform_assign_firm(
        &state.pool,
        firm_id,
        person_id,
        &body.role,
        ctx.person_id,
        ctx.er_systemadmin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "tildelt": true })))
}

fn access_log_json(rader: &[regnmed_db::PlatformAccessLog]) -> serde_json::Value {
    json!({
        "innsyn": rader.iter().map(|r| json!({
            "navn": r.name,
            "rolle": r.role,
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
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    let rader = regnmed_db::platform_access_for_company(&state.pool, company_id).await?;
    Ok(Json(access_log_json(&rader)))
}

/// The byrå twin, for firm admins.
pub async fn firm_platform_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::firm_members::require_firm_admin(&state, person.person_id, firm_id).await?;
    let rader = regnmed_db::platform_access_for_firm(&state.pool, firm_id).await?;
    Ok(Json(access_log_json(&rader)))
}

/// Packages and add-ons with their price history (migration 0093). Support
/// may look; changing what is sold is systemadmin territory, like the other
/// customer-facing registers.
async fn list_plans(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlatformContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let plans = regnmed_db::plans::list_plans(&state.pool).await?;
    Ok(Json(json!({ "plans": plans })))
}

fn plan_kind() -> String {
    "plan".into()
}
fn yes() -> bool {
    true
}

#[derive(Deserialize)]
pub struct PlanRequest {
    name: String,
    #[serde(default = "plan_kind")]
    kind: String,
    #[serde(default = "yes")]
    available: bool,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    featured: bool,
    #[serde(default)]
    listed: bool,
    voucher_limit: Option<i32>,
    employee_limit: Option<i32>,
    integration_limit: Option<i32>,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    sort: i32,
}

/// A new insert-only version of a plan, or a new plan for a new slug.
async fn put_plan(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path(slug): Path<String>,
    Json(body): Json<PlanRequest>,
) -> Result<StatusCode, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::plans::validate_slug(&slug).map_err(ApiError::BadRequest)?;
    let input = regnmed_db::plans::PlanInput {
        name: body.name,
        kind: body.kind,
        available: body.available,
        audience: body.audience,
        featured: body.featured,
        listed: body.listed,
        voucher_limit: body.voucher_limit,
        employee_limit: body.employee_limit,
        integration_limit: body.integration_limit,
        features: body.features,
        sort: body.sort,
    };
    regnmed_db::plans::validate_input(&input).map_err(ApiError::BadRequest)?;
    if let Some(kind) = regnmed_db::plans::existing_kind(&state.pool, &slug).await?
        && kind != input.kind
    {
        return Err(ApiError::BadRequest(
            "typen til en plan kan ikke endres; opprett en ny plan i stedet".into(),
        ));
    }
    regnmed_db::plans::save_plan(&state.pool, &slug, &input, ctx.person_id).await?;
    crate::subscription::invalidate_pricing_cache();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PriceRequest {
    interval: String,
    /// Øre, an integer: a `year` row is the whole year's amount.
    price_ore: i64,
    valid_from: NaiveDate,
    source: String,
    /// Must be true when companies are on the plan today: the price reaches them.
    #[serde(default)]
    confirm_existing: bool,
}

async fn post_price(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path(slug): Path<String>,
    Json(body): Json<PriceRequest>,
) -> Result<StatusCode, ApiError> {
    ctx.require_systemadmin()?;
    use regnmed_db::plans::PriceRefused as R;
    let today = chrono::Utc::now().date_naive();
    match regnmed_db::plans::add_price(
        &state.pool,
        &slug,
        &body.interval,
        body.price_ore,
        body.valid_from,
        today,
        &body.source,
        body.confirm_existing,
    )
    .await?
    {
        Ok(()) => {
            crate::subscription::invalidate_pricing_cache();
            Ok(StatusCode::CREATED)
        }
        Err(R::InUse(n)) => Err(ApiError::BadRequest(format!(
            "{n} selskap er på denne planen, og en ny pris endrer hva de betaler. Bekreft for å fortsette."
        ))),
        Err(why) => Err(ApiError::BadRequest(
            match why {
                R::UnknownPlan => "ukjent plan",
                R::BadInterval => "intervallet må være month eller year",
                R::Negative => "prisen kan ikke være negativ",
                R::NoSource => "prisen må ha en kilde (hvorfor den settes)",
                R::InThePast => "prisen kan ikke gjelde bakover i tid; velg i dag eller senere",
                R::Duplicate => {
                    "det finnes allerede en pris for denne planen, dette intervallet og denne dagen"
                }
                R::InUse(_) => unreachable!("answered above"),
            }
            .to_string(),
        )),
    }
}

#[derive(Deserialize)]
pub struct AddonRequest {
    addon: String,
    /// The agreement or pilot the grant rests on; stands on the row.
    note: String,
}

/// Gives a company an add-on (tilleggstjeneste). Until buying one in the
/// portal exists, this is the only way a company gets it — so it is
/// systemadmin territory and carries a reference, like manual coverage.
async fn grant_addon(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<AddonRequest>,
) -> Result<StatusCode, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::bank_feed::grant_addon(
        &state.pool,
        company_id,
        body.addon.trim(),
        &body.note,
        &format!("plattform:{}", ctx.person_id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(StatusCode::CREATED)
}

async fn end_addon(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlatformContext>,
    Path((company_id, addon)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiError> {
    ctx.require_systemadmin()?;
    regnmed_db::bank_feed::end_addon(&state.pool, company_id, &addon)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(StatusCode::NO_CONTENT)
}
