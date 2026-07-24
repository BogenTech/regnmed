//! Report endpoints — the web is the product; the CLI wraps the same
//! crate functions for ops. Every report a user or workflow can trigger
//! is exposed here, guarded per company through the engagement model:
//! any access level (admin/bokforing/les — revisor included) may read
//! reports, since reports never mutate the ledger.
//!
//! Routes (all require a Bearer token):
//! - GET /companies/{id}/reports/mva?year=&termin=      → JSON spesifikasjon
//! - GET /companies/{id}/reports/mva-melding?year=&termin= → XML download
//! - GET /companies/{id}/reports/saft?year= (or from=&to=) → XML download

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use regnmed_core::mva::{Direction, Termin, direction};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

/// 404 (not 403) when the person has no path to the company: a caller
/// without access must not learn that the company exists.
async fn require_access(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<String, ApiError> {
    regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)
}

fn termin_of(year: i32, termin: u8) -> Result<Termin, ApiError> {
    Termin::new(year, termin).ok_or_else(|| ApiError::BadRequest("termin must be 1-6".into()))
}

#[derive(Deserialize)]
pub struct TerminQuery {
    year: i32,
    termin: u8,
}

pub async fn mva_report(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TerminQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    let termin = termin_of(query.year, query.termin)?;

    let lines =
        regnmed_db::mva_spesifikasjon(&state.pool, company_id, termin.start(), termin.end())
            .await?;

    let utgaende: i64 = lines
        .iter()
        .filter(|l| direction(&l.code) == Direction::Utgaende)
        .map(|l| -l.avgift_ore)
        .sum();
    let inngaende: i64 = lines
        .iter()
        .filter(|l| direction(&l.code) == Direction::Inngaende)
        .map(|l| l.avgift_ore)
        .sum();

    Ok(Json(json!({
        "year": termin.year,
        "termin": termin.number,
        "start": termin.start().to_string(),
        "end": termin.end().to_string(),
        "lines": lines.iter().map(|l| json!({
            "code": l.code,
            "description": l.description,
            "rate_bp": l.rate_bp,
            "grunnlag_ore": l.grunnlag_ore,
            "avgift_ore": l.avgift_ore,
        })).collect::<Vec<_>>(),
        "utgaende_ore": utgaende,
        "inngaende_ore": inngaende,
        "netto_ore": utgaende - inngaende,
    })))
}

pub async fn mva_melding(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TerminQuery>,
) -> Result<Response, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    let termin = termin_of(query.year, query.termin)?;

    let orgnr: String = sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(company_id)
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let spes = regnmed_db::mva_spesifikasjon(&state.pool, company_id, termin.start(), termin.end())
        .await?;
    if spes.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "no VAT postings in {termin} — nothing to report"
        )));
    }

    let referanse = format!("regnmed-{}-{}-{}", orgnr, termin.year, termin.number);
    let melding = regnmed_core::mvamelding::build(
        &orgnr,
        termin,
        &referanse,
        env!("CARGO_PKG_VERSION"),
        &spes,
    );
    let filename = format!(
        "mva-melding_{}_{}-termin{}.xml",
        orgnr, termin.year, termin.number
    );
    Ok(xml_download(
        regnmed_core::mvamelding::render(&melding),
        &filename,
    ))
}

#[derive(Deserialize)]
pub struct SaftQuery {
    year: Option<i32>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    /// Norwegian SAF-T requires a contact person in the header; defaults
    /// to the authenticated person's name.
    contact_first: Option<String>,
    contact_last: Option<String>,
}

pub async fn saft_export(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<SaftQuery>,
) -> Result<Response, ApiError> {
    require_access(&state, person.person_id, company_id).await?;

    let (start, end) = match (query.year, query.from, query.to) {
        (Some(year), None, None) => (
            NaiveDate::from_ymd_opt(year, 1, 1)
                .ok_or_else(|| ApiError::BadRequest("invalid year".into()))?,
            NaiveDate::from_ymd_opt(year, 12, 31)
                .ok_or_else(|| ApiError::BadRequest("invalid year".into()))?,
        ),
        (None, Some(from), Some(to)) if from <= to => (from, to),
        _ => {
            return Err(ApiError::BadRequest(
                "pass year=, or from= and to= (from before to)".into(),
            ));
        }
    };

    // The exporting person is the natural header contact.
    let (first, last) = match (&query.contact_first, &query.contact_last) {
        (Some(first), Some(last)) => (first.clone(), last.clone()),
        _ => person
            .name
            .as_deref()
            .and_then(|n| n.trim().rsplit_once(' '))
            .map(|(first, last)| (first.to_string(), last.to_string()))
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "no full name on the token; pass contact_first= and contact_last=".into(),
                )
            })?,
    };

    let input =
        regnmed_db::load_saft_input(&state.pool, company_id, start, end, &first, &last).await?;
    let filename = format!(
        "SAF-T Financial_{}_{}.xml",
        input.orgnr,
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    );
    let xml = regnmed_core::saft::render(&input).map_err(ApiError::BadRequest)?;
    Ok(xml_download(xml, &filename))
}

fn xml_download(xml: String, filename: &str) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/xml; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        xml,
    )
        .into_response()
}

// ---- Lovpålagte spesifikasjoner (bokføringsforskriften §3-1) ----

#[derive(Deserialize)]
pub struct PeriodQuery {
    from: NaiveDate,
    to: NaiveDate,
    account: Option<String>,
    /// Dimension filters (resultat per avdeling/prosjekt).
    avdeling: Option<String>,
    prosjekt: Option<String>,
}

fn check_period(from: NaiveDate, to: NaiveDate) -> Result<(), ApiError> {
    if from > to {
        return Err(ApiError::BadRequest("from must not be after to".into()));
    }
    Ok(())
}

pub async fn saldobalanse(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<PeriodQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    check_period(query.from, query.to)?;
    let rows = regnmed_db::saldobalanse(&state.pool, company_id, query.from, query.to).await?;
    Ok(Json(json!({
        "accounts": rows.iter().map(|r| json!({
            "number": r.number,
            "name": r.name,
            "inngaende_ore": r.inngaende_ore,
            "debet_ore": r.debet_ore,
            "kredit_ore": r.kredit_ore,
            "utgaende_ore": r.utgaende_ore,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn kontospesifikasjon(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<PeriodQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    check_period(query.from, query.to)?;
    let posts = regnmed_db::kontospesifikasjon(
        &state.pool,
        company_id,
        query.account.as_deref(),
        query.from,
        query.to,
    )
    .await?;
    Ok(Json(json!({
        "posts": posts.iter().map(|p| json!({
            "account": p.number,
            "account_name": p.account_name,
            "bilag": format!("{}-{}-{}", p.journal_code, p.fiscal_year, p.voucher_number),
            "date": p.voucher_date.to_string(),
            "description": p.description,
            "amount_ore": p.amount_ore,
            "saldo_ore": p.saldo_ore,
            "party_no": p.party_no,
            "avdeling": p.avdeling,
            "prosjekt": p.prosjekt,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn bokforingsspesifikasjon(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<PeriodQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    check_period(query.from, query.to)?;
    let vouchers =
        regnmed_db::bokforingsspesifikasjon(&state.pool, company_id, query.from, query.to).await?;
    Ok(Json(json!({
        "vouchers": vouchers.iter().map(|v| json!({
            "bilag": format!("{}-{}-{}", v.journal_code, v.fiscal_year, v.voucher_number),
            "date": v.voucher_date.to_string(),
            "description": v.description,
            "lines": v.lines.iter().map(|l| json!({
                "line_no": l.line_no,
                "account": l.account_number,
                "account_name": l.account_name,
                "amount_ore": l.amount_ore,
                "vat_code": l.vat_code,
                "description": l.description,
                "party_no": l.party_no,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })))
}

fn seksjon_json(s: &regnmed_core::regnskap::Seksjon) -> serde_json::Value {
    json!({
        "heading": s.heading,
        "sum_ore": s.sum_ore,
        "lines": s.lines.iter().map(|l| json!({
            "number": l.number,
            "name": l.name,
            "saldo_ore": l.saldo_ore,
        })).collect::<Vec<_>>(),
    })
}

pub async fn resultat(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<PeriodQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    check_period(query.from, query.to)?;
    let lines = regnmed_db::saldo_lines(
        &state.pool,
        company_id,
        Some(query.from),
        query.to,
        query.avdeling.as_deref(),
        query.prosjekt.as_deref(),
    )
    .await?;
    let r = regnmed_core::regnskap::resultat(&lines);
    Ok(Json(json!({
        "seksjoner": r.seksjoner.iter().map(seksjon_json).collect::<Vec<_>>(),
        "driftsresultat_ore": r.driftsresultat_ore,
        "arsresultat_ore": r.arsresultat_ore,
        "avdeling": query.avdeling,
        "prosjekt": query.prosjekt,
    })))
}

#[derive(Deserialize)]
pub struct DateQuery {
    date: NaiveDate,
}

pub async fn balanse(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<DateQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    let lines =
        regnmed_db::saldo_lines(&state.pool, company_id, None, query.date, None, None).await?;
    let b = regnmed_core::regnskap::balanse(&lines);
    Ok(Json(json!({
        "eiendeler": seksjon_json(&b.eiendeler),
        "egenkapital_gjeld": seksjon_json(&b.egenkapital_gjeld),
        "udisponert_resultat_ore": b.udisponert_resultat_ore,
        "differanse_ore": b.differanse_ore(),
    })))
}

// ---- Revisorens verifikasjonsrapport (issue #24) ----

#[derive(Deserialize)]
pub struct RevisjonQuery {
    format: Option<String>,
}

/// Every guarantee checked against the live ledger, in one document.
/// Any access level may generate it — the revisor (engagement 'revisjon'
/// → 'les') is exactly who it is for. `?format=tekst` downloads the
/// deterministic plain-text rendering for the revisor's own archive.
pub async fn revisjon(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RevisjonQuery>,
) -> Result<Response, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    let generated_by = person.name.as_deref().unwrap_or(&person.sub);
    let report = regnmed_db::build_revisjon_report(
        &state.pool,
        company_id,
        generated_by,
        env!("CARGO_PKG_VERSION"),
    )
    .await?;

    if query.format.as_deref() == Some("tekst") {
        let filename = format!("verifikasjonsrapport_{}.txt", report.orgnr);
        return Ok((
            [
                (
                    header::CONTENT_TYPE,
                    "text/plain; charset=utf-8".to_string(),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                ),
            ],
            regnmed_core::revisjon::render_text(&report),
        )
            .into_response());
    }

    Ok(Json(json!({
        "orgnr": report.orgnr,
        "selskap": report.selskap,
        "generert": report.generert,
        "generert_av": report.generert_av,
        "programversjon": report.programversjon,
        "kjede_sekvens": report.kjede_sekvens,
        "kjede_hode": report.kjede_hode_hex,
        "alle_ok": report.alle_ok(),
        "kontroller": report.kontroller.iter().map(|k| json!({
            "navn": k.navn,
            "ok": k.ok,
            "detalj": k.detalj,
        })).collect::<Vec<_>>(),
        "ankere": report.ankere.iter().map(|a| json!({
            "tidspunkt": a.tidspunkt,
            "root": a.root_hex,
            "siste_sekvens": a.siste_sekvens,
            "vitner": a.vitner,
        })).collect::<Vec<_>>(),
    }))
    .into_response())
}
