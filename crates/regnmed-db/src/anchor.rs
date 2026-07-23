//! Anchor snapshots: freeze every chain head under one Merkle root, and
//! verify anchored history against the live chains.
//!
//! Snapshot creation reads all chain heads in a single statement (one
//! MVCC snapshot — consistent without locking posting), computes the
//! root with `regnmed-core::anchor` (format v1, frozen), and stores
//! snapshot + leaves append-only. Publication of the root — the part
//! that actually defeats a DBA — happens outside this module: the public
//! `/anchors` endpoint and the RFC 3161 witness (regnmed-gov::tsa).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regnmed_core::anchor::{AnchorLeaf, InclusionProof, inclusion_proof, merkle_root};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct AnchorSnapshot {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub root_hash: [u8; 32],
    pub leaf_count: i32,
}

#[derive(Debug)]
pub struct WitnessRow {
    pub method: String,
    pub reference: String,
    pub witnessed_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct SnapshotRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub root_hash: [u8; 32],
    pub leaf_count: i32,
    pub witnesses: Vec<WitnessRow>,
}

#[derive(Debug)]
pub struct CompanyAnchor {
    pub snapshot_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub root_hash: [u8; 32],
    pub last_seq: i64,
    pub last_hash: [u8; 32],
    pub proof: InclusionProof,
    pub witnesses: Vec<WitnessRow>,
}

#[derive(Debug, Default)]
pub struct AnchorCheck {
    pub snapshots_checked: i64,
    pub problems: Vec<String>,
}

/// Creates a new snapshot over every company with at least one voucher.
/// `Ok(None)` when there is nothing to anchor yet.
pub async fn create_anchor_snapshot(pool: &PgPool) -> Result<Option<AnchorSnapshot>> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(
        "select company_id, last_seq, last_hash from chain_head where last_seq > 0
         order by company_id",
    )
    .fetch_all(&mut *tx)
    .await?;
    let leaves: Vec<AnchorLeaf> = rows
        .iter()
        .map(|r| {
            Ok(AnchorLeaf {
                company_id: r.get("company_id"),
                last_seq: r.get("last_seq"),
                last_hash: to_hash32(r.get("last_hash"))?,
            })
        })
        .collect::<Result<_>>()?;
    let Some(root) = merkle_root(&leaves) else {
        return Ok(None);
    };

    let id = Uuid::now_v7();
    let created_at: DateTime<Utc> = sqlx::query_scalar(
        "insert into anchor_snapshot (id, root_hash, leaf_count) values ($1, $2, $3)
         returning created_at",
    )
    .bind(id)
    .bind(root.as_slice())
    .bind(leaves.len() as i32)
    .fetch_one(&mut *tx)
    .await?;
    for leaf in &leaves {
        sqlx::query(
            "insert into anchor_leaf (snapshot_id, company_id, last_seq, last_hash)
             values ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(leaf.company_id)
        .bind(leaf.last_seq)
        .bind(leaf.last_hash.as_slice())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Some(AnchorSnapshot {
        id,
        created_at,
        root_hash: root,
        leaf_count: leaves.len() as i32,
    }))
}

/// Records a successful external publication of a snapshot's root.
pub async fn add_anchor_witness(
    pool: &PgPool,
    snapshot_id: Uuid,
    method: &str,
    reference: &str,
    proof: &[u8],
) -> Result<()> {
    sqlx::query(
        "insert into anchor_witness (id, snapshot_id, method, reference, proof)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(snapshot_id)
    .bind(method)
    .bind(reference)
    .bind(proof)
    .execute(pool)
    .await?;
    Ok(())
}

/// Latest snapshots, newest first — the transparency feed: roots and
/// witness metadata only, no company data.
pub async fn latest_anchor_snapshots(pool: &PgPool, limit: i64) -> Result<Vec<SnapshotRow>> {
    let rows = sqlx::query(
        "select id, created_at, root_hash, leaf_count from anchor_snapshot
         order by created_at desc, id desc limit $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut snapshots = Vec::with_capacity(rows.len());
    for row in rows {
        let id: Uuid = row.get("id");
        snapshots.push(SnapshotRow {
            id,
            created_at: row.get("created_at"),
            root_hash: to_hash32(row.get("root_hash"))?,
            leaf_count: row.get("leaf_count"),
            witnesses: witnesses_for(pool, id).await?,
        });
    }
    Ok(snapshots)
}

/// The snapshots covering one company, each with the anchored head and
/// the inclusion proof connecting it to the published root — everything
/// a revisor needs to verify independently.
pub async fn company_anchors(pool: &PgPool, company_id: Uuid) -> Result<Vec<CompanyAnchor>> {
    let rows = sqlx::query(
        "select s.id, s.created_at, s.root_hash, l.last_seq, l.last_hash
         from anchor_leaf l join anchor_snapshot s on s.id = l.snapshot_id
         where l.company_id = $1
         order by s.created_at desc, s.id desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut anchors = Vec::with_capacity(rows.len());
    for row in rows {
        let snapshot_id: Uuid = row.get("id");
        let leaves = snapshot_leaves(pool, snapshot_id).await?;
        let proof = inclusion_proof(&leaves, company_id)
            .context("anchored company missing from its own snapshot's leaves")?;
        anchors.push(CompanyAnchor {
            snapshot_id,
            created_at: row.get("created_at"),
            root_hash: to_hash32(row.get("root_hash"))?,
            last_seq: row.get("last_seq"),
            last_hash: to_hash32(row.get("last_hash"))?,
            proof,
            witnesses: witnesses_for(pool, snapshot_id).await?,
        });
    }
    Ok(anchors)
}

/// Checks every anchored head for one company against the live chain:
/// the voucher at the anchored sequence must still carry the anchored
/// hash, and the snapshot's stored root must still recompute from its
/// leaves. Any mismatch is evidence of rewritten history.
pub async fn verify_company_anchors(pool: &PgPool, company_id: Uuid) -> Result<AnchorCheck> {
    let rows = sqlx::query(
        "select s.id, s.created_at, s.root_hash, l.last_seq, l.last_hash
         from anchor_leaf l join anchor_snapshot s on s.id = l.snapshot_id
         where l.company_id = $1
         order by s.created_at, s.id",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut check = AnchorCheck::default();
    for row in rows {
        check.snapshots_checked += 1;
        let snapshot_id: Uuid = row.get("id");
        let created_at: DateTime<Utc> = row.get("created_at");
        let anchored_seq: i64 = row.get("last_seq");
        let anchored_hash = to_hash32(row.get("last_hash"))?;
        let stored_root = to_hash32(row.get("root_hash"))?;

        let live_hash: Option<Vec<u8>> =
            sqlx::query_scalar("select hash from voucher where company_id = $1 and chain_seq = $2")
                .bind(company_id)
                .bind(anchored_seq)
                .fetch_optional(pool)
                .await?;
        match live_hash.map(to_hash32).transpose()? {
            None => check.problems.push(format!(
                "snapshot {snapshot_id} ({created_at}): anchored voucher at seq \
                 {anchored_seq} no longer exists — history has been truncated"
            )),
            Some(hash) if hash != anchored_hash => check.problems.push(format!(
                "snapshot {snapshot_id} ({created_at}): voucher at seq {anchored_seq} \
                 no longer matches the anchored hash — history has been rewritten"
            )),
            Some(_) => {}
        }

        let leaves = snapshot_leaves(pool, snapshot_id).await?;
        if merkle_root(&leaves) != Some(stored_root) {
            check.problems.push(format!(
                "snapshot {snapshot_id} ({created_at}): stored root does not recompute \
                 from its leaves — the anchor rows themselves have been tampered with"
            ));
        }
    }
    Ok(check)
}

async fn snapshot_leaves(pool: &PgPool, snapshot_id: Uuid) -> Result<Vec<AnchorLeaf>> {
    let rows = sqlx::query(
        "select company_id, last_seq, last_hash from anchor_leaf where snapshot_id = $1",
    )
    .bind(snapshot_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(AnchorLeaf {
                company_id: r.get("company_id"),
                last_seq: r.get("last_seq"),
                last_hash: to_hash32(r.get("last_hash"))?,
            })
        })
        .collect()
}

async fn witnesses_for(pool: &PgPool, snapshot_id: Uuid) -> Result<Vec<WitnessRow>> {
    let rows = sqlx::query(
        "select method, reference, witnessed_at from anchor_witness
         where snapshot_id = $1 order by witnessed_at",
    )
    .bind(snapshot_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| WitnessRow {
            method: r.get("method"),
            reference: r.get("reference"),
            witnessed_at: r.get("witnessed_at"),
        })
        .collect())
}

fn to_hash32(bytes: Vec<u8>) -> Result<[u8; 32]> {
    bytes.try_into().ok().context("stored hash is not 32 bytes")
}
