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
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use regnmed_core::mva::{Direction, Termin, direction};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};
use crate::tilgang::{Rett, krev};

/// 404 (not 403) when the person has no path to the company: a caller
/// without access must not learn that the company exists.

/// Resolves the company's ordning for the year and validates the
/// periode number against it (docs/mva.md, #51).
async fn ordning_termin(
    state: &AppState,
    company_id: Uuid,
    year: i32,
    termin: u8,
) -> Result<(regnmed_core::mva::Terminordning, Termin), ApiError> {
    let jan1 = NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| ApiError::BadRequest("invalid year".into()))?;
    let ordning = regnmed_db::terminordning_on(&state.pool, company_id, jan1).await?;
    let termin = ordning.ny_periode(year, termin).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "termin must be 1-{} under ordningen {}",
            ordning.antall_perioder(),
            ordning.as_str()
        ))
    })?;
    Ok((ordning, termin))
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let (ordning, termin) = ordning_termin(&state, company_id, query.year, query.termin).await?;

    let lines = regnmed_db::mva_spesifikasjon(
        &state.pool,
        company_id,
        ordning.start(termin),
        ordning.end(termin),
    )
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
        "ordning": ordning.as_str(),
        "antall_perioder": ordning.antall_perioder(),
        "label": ordning.label(termin),
        "frist": ordning.frist(termin).to_string(),
        "start": ordning.start(termin).to_string(),
        "end": ordning.end(termin).to_string(),
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let (ordning, termin) = ordning_termin(&state, company_id, query.year, query.termin).await?;

    let orgnr: String = sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(company_id)
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let spes = regnmed_db::mva_spesifikasjon(
        &state.pool,
        company_id,
        ordning.start(termin),
        ordning.end(termin),
    )
    .await?;
    if spes.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "no VAT postings in {} — nothing to report",
            ordning.label(termin)
        )));
    }

    let referanse = format!("regnmed-{}-{}-{}", orgnr, termin.year, termin.number);
    let melding = regnmed_core::mvamelding::build(
        &orgnr,
        termin,
        ordning,
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;

    let (start, end) = match (query.year, query.from, query.to) {
        // year= betyr regnskapsåret; definisjonen ligger i
        // regnmed-core::regnskapsar (docs/regelverk.md, #52).
        (Some(year), None, None) => regnmed_core::regnskapsar::regnskapsar_periode(year)
            .ok_or_else(|| ApiError::BadRequest("invalid year".into()))?,
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
    file_response(Vis::Attachment, filename, "application/xml", xml)
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
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
pub struct ProsjektRapportQuery {
    year: Option<i32>,
    prosjekt: Option<String>,
}

/// Prosjektlønnsomhet (#71, docs/rapporter.md): does the project make
/// money? Pure composition of what already exists — dimension-filtered
/// SUM queries folded with `regnskap::lonnsomhet` (presentation signs
/// in one place), the timesheet summary's billed/unbilled split, and
/// the kunde link from #80. No new stored state.
pub async fn prosjektlonnsomhet(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ProsjektRapportQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let year = match query.year {
        Some(y) => y,
        None => {
            let today: NaiveDate = sqlx::query_scalar("select current_date")
                .fetch_one(&state.pool)
                .await
                .map_err(anyhow::Error::from)?;
            regnmed_core::regnskapsar::regnskapsar(today)
        }
    };
    let (from, to) = regnmed_core::regnskapsar::regnskapsar_periode(year)
        .ok_or_else(|| ApiError::BadRequest(format!("ugyldig år {year}")))?;

    let dims = regnmed_db::list_dimensions(&state.pool, company_id, person.person_id).await?;
    let timer = regnmed_db::timesheet_summary(&state.pool, company_id, from, to).await?;
    let timer_for = |code: &str| timer.iter().find(|t| t.prosjekt.as_deref() == Some(code));
    let timer_json = |t: Option<&regnmed_db::timesheet::ProsjektSum>| match t {
        Some(t) => json!({
            "minutter": t.minutter,
            "fakturerte_minutter": t.fakturerte_minutter,
            "fakturert_ore": t.fakturert_ore,
            "ufakturert_ore": t.ufakturert_ore,
        }),
        None => json!({
            "minutter": 0, "fakturerte_minutter": 0,
            "fakturert_ore": 0, "ufakturert_ore": 0,
        }),
    };

    if let Some(code) = &query.prosjekt {
        let dim = dims
            .iter()
            .find(|d| d.kind == "prosjekt" && &d.code == code)
            .ok_or_else(|| ApiError::BadRequest(format!("ingen prosjekt med kode {code}")))?;
        let lines =
            regnmed_db::saldo_lines(&state.pool, company_id, Some(from), to, None, Some(code))
                .await?;
        let l = regnmed_core::regnskap::lonnsomhet(&lines);
        let r = regnmed_core::regnskap::resultat(&lines);
        return Ok(Json(json!({
            "year": year,
            "prosjekt": dim.code,
            "name": dim.name,
            "active": dim.active,
            "kunde": dim.kunde,
            "kunde_navn": dim.kunde_navn,
            "inntekter_ore": l.inntekter_ore,
            "kostnader_ore": l.kostnader_ore,
            "resultat_ore": l.resultat_ore(),
            "timer": timer_json(timer_for(code)),
            "seksjoner": r.seksjoner.iter().map(seksjon_json).collect::<Vec<_>>(),
        })));
    }

    let saldo = regnmed_db::prosjekt_saldo_lines(&state.pool, company_id, from, to).await?;
    let rows = dims
        .iter()
        .filter(|d| d.kind == "prosjekt")
        .map(|d| {
            let lines: Vec<regnmed_core::regnskap::SaldoLine> = saldo
                .iter()
                .filter(|(p, _)| p == &d.code)
                .map(|(_, l)| regnmed_core::regnskap::SaldoLine {
                    number: l.number.clone(),
                    name: l.name.clone(),
                    saldo_ore: l.saldo_ore,
                })
                .collect();
            let l = regnmed_core::regnskap::lonnsomhet(&lines);
            json!({
                "prosjekt": d.code,
                "name": d.name,
                "active": d.active,
                "kunde": d.kunde,
                "kunde_navn": d.kunde_navn,
                "inntekter_ore": l.inntekter_ore,
                "kostnader_ore": l.kostnader_ore,
                "resultat_ore": l.resultat_ore(),
                "timer": timer_json(timer_for(&d.code)),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "year": year, "prosjekter": rows })))
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
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
        return Ok(file_response(
            Vis::Attachment,
            &filename,
            "text/plain",
            regnmed_core::revisjon::render_text(&report),
        ));
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

/// The company's mva-terminordning (docs/mva.md, #51): current
/// ordning, this year's perioder with frister, and the registered
/// history. To-måneder is the default and needs no row.
pub async fn terminordning(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let today: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let ordning = regnmed_db::terminordning_on(&state.pool, company_id, today).await?;
    let year = chrono::Datelike::year(&today);
    let perioder: Vec<serde_json::Value> = (1..=ordning.antall_perioder())
        .filter_map(|n| ordning.ny_periode(year, n))
        .map(|t| {
            json!({
                "termin": t.number,
                "label": ordning.label(t),
                "start": ordning.start(t).to_string(),
                "end": ordning.end(t).to_string(),
                "frist": ordning.frist(t).to_string(),
            })
        })
        .collect();
    let history = regnmed_db::list_terminordninger(&state.pool, company_id).await?;
    Ok(Json(json!({
        "ordning": ordning.as_str(),
        "antall_perioder": ordning.antall_perioder(),
        "year": year,
        "perioder": perioder,
        "history": history.iter().map(|h| json!({
            "valid_from": h.valid_from.to_string(),
            "ordning": h.ordning,
            "note": h.note,
            "created_by": h.created_by,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct SetOrdningRequest {
    /// to-maneder | arlig | primaernaering
    ordning: String,
    valid_from: NaiveDate,
    /// Reference to Skatteetatens vedtak — the ordning is GRANTED,
    /// never inferred.
    note: Option<String>,
}

pub async fn set_terminordning(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<SetOrdningRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MvaOrdningAdmin).await?;
    let ordning = regnmed_core::mva::Terminordning::parse(&request.ordning).ok_or_else(|| {
        ApiError::BadRequest("ordning must be to-maneder, arlig or primaernaering".into())
    })?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::set_terminordning(
        &state.pool,
        company_id,
        request.valid_from,
        ordning,
        request.note.as_deref(),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        json!({ "ordning": ordning.as_str(), "valid_from": request.valid_from.to_string() }),
    ))
}

#[derive(Deserialize, Default)]
pub struct NokkeltallQuery {
    /// The month columns and the year-to-date figures apply to this year;
    /// defaults to the current one. Liquidity and deadlines are always NOW.
    year: Option<i32>,
}

/// Key figures for the overview (docs/rapporter.md, #36): plain queries
/// over numbers we already have — result, liquidity picture and upcoming
/// mva deadlines under the company's terminordning.
pub async fn nokkeltall(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<NokkeltallQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let today: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let year = query.year.unwrap_or(chrono::Datelike::year(&today));
    let tall = regnmed_db::nokkeltall(&state.pool, company_id, year, today).await?;

    // Mva owed for the current period (net computed so far) and the next
    // deadlines, under the company's ordning.
    let ordning = regnmed_db::terminordning_on(&state.pool, company_id, today).await?;
    let naa = ordning.periode_of(today);
    let spes = regnmed_db::mva_spesifikasjon(
        &state.pool,
        company_id,
        ordning.start(naa),
        ordning.end(naa).min(today),
    )
    .await?;
    let utgaende: i64 = spes
        .iter()
        .filter(|l| regnmed_core::mva::direction(&l.code) == regnmed_core::mva::Direction::Utgaende)
        .map(|l| -l.avgift_ore)
        .sum();
    let inngaende: i64 = spes
        .iter()
        .filter(|l| {
            regnmed_core::mva::direction(&l.code) == regnmed_core::mva::Direction::Inngaende
        })
        .map(|l| l.avgift_ore)
        .sum();
    let mva_netto = utgaende - inngaende;

    let this_year = chrono::Datelike::year(&today);
    let mut frister = Vec::new();
    for y in [this_year, this_year + 1] {
        for n in 1..=ordning.antall_perioder() {
            if let Some(periode) = ordning.ny_periode(y, n) {
                let frist = ordning.frist(periode);
                if frist >= today && frister.len() < 2 {
                    frister.push(serde_json::json!({
                        "type": "mva",
                        "label": ordning.label(periode),
                        "frist": frist.to_string(),
                    }));
                }
            }
        }
    }

    let disponibelt =
        tall.bank_ore + tall.kundefordringer_ore - tall.leverandorgjeld_ore - mva_netto.max(0);
    Ok(Json(json!({
        "year": tall.year,
        "resultat_hittil_ore": tall.resultat_hittil_ore,
        "resultat_fjor_ore": tall.resultat_fjor_ore,
        "maaneder": tall.maaneder,
        "likviditet": {
            "bank_ore": tall.bank_ore,
            "kundefordringer_ore": tall.kundefordringer_ore,
            "leverandorgjeld_ore": tall.leverandorgjeld_ore,
            "mva_netto_ore": mva_netto,
            "disponibelt_ore": disponibelt,
        },
        "frister": frister,
    })))
}
