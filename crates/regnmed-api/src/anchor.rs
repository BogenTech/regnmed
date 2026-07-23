//! Anchoring endpoints — the "don't trust us — verify" surface.
//!
//! - GET /anchors — **public, no token**: the transparency feed of anchor
//!   roots and their external witnesses. It exposes only root hashes,
//!   timestamps and leaf counts — no company data — and being public is
//!   the point: every independent copy of a root (a revisor's notebook,
//!   a monitoring job) is one more witness a database rewrite cannot
//!   reach.
//! - GET /companies/{id}/anchors — the snapshots covering one company,
//!   each with the anchored head and the Merkle inclusion proof
//!   connecting it to the public root (any access level; 404 hides
//!   existence from outsiders).
//! - GET /companies/{id}/anchors/verify — full independent check: chain
//!   from genesis, attachments, and every anchored head against the live
//!   chain. Built for revisorer; read access suffices — verification
//!   never mutates anything.

use axum::Json;
use axum::extract::{Path, State};
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn require_access(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<String, ApiError> {
    regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)
}

fn witness_json(witnesses: &[regnmed_db::WitnessRow]) -> Vec<serde_json::Value> {
    witnesses
        .iter()
        .map(|w| {
            json!({
                "method": w.method,
                "reference": w.reference,
                "witnessed_at": w.witnessed_at.to_rfc3339(),
            })
        })
        .collect()
}

/// Public transparency feed — deliberately unauthenticated.
pub async fn list_anchors(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let snapshots = regnmed_db::latest_anchor_snapshots(&state.pool, 20).await?;
    Ok(Json(json!({
        "snapshots": snapshots.iter().map(|s| json!({
            "snapshot_id": s.id,
            "created_at": s.created_at.to_rfc3339(),
            "root_hash": hex::encode(s.root_hash),
            "leaf_count": s.leaf_count,
            "witnesses": witness_json(&s.witnesses),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn company_anchors(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    let anchors = regnmed_db::company_anchors(&state.pool, company_id).await?;
    Ok(Json(json!({
        "anchors": anchors.iter().map(|a| json!({
            "snapshot_id": a.snapshot_id,
            "created_at": a.created_at.to_rfc3339(),
            "root_hash": hex::encode(a.root_hash),
            "last_seq": a.last_seq,
            "last_hash": hex::encode(a.last_hash),
            "proof": a.proof.iter().map(|step| json!({
                "side": match step.side {
                    regnmed_core::anchor::Side::Left => "left",
                    regnmed_core::anchor::Side::Right => "right",
                },
                "sibling": hex::encode(step.sibling),
            })).collect::<Vec<_>>(),
            "witnesses": witness_json(&a.witnesses),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn verify_anchors(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id).await?;
    let chain = regnmed_db::verify_chain(&state.pool, company_id).await;
    let attachments = regnmed_db::verify_attachments(&state.pool, company_id).await;
    let anchors = regnmed_db::verify_company_anchors(&state.pool, company_id).await?;

    let mut problems: Vec<String> = Vec::new();
    let vouchers_checked = match &chain {
        Ok(report) => report.vouchers_checked,
        Err(e) => {
            problems.push(format!("kjedeverifisering feilet: {e}"));
            0
        }
    };
    let attachments_checked = match &attachments {
        Ok(count) => *count,
        Err(e) => {
            problems.push(format!("vedleggsverifisering feilet: {e}"));
            0
        }
    };
    problems.extend(anchors.problems.iter().cloned());

    Ok(Json(json!({
        "ok": problems.is_empty(),
        "vouchers_checked": vouchers_checked,
        "attachments_checked": attachments_checked,
        "anchors_checked": anchors.snapshots_checked,
        "problems": problems,
    })))
}
