//! External anchoring of chain heads: the Merkle snapshot format.
//!
//! The voucher chain proves that history *within* the database is
//! consistent — but an adversary with full database access could rewrite
//! the entire chain from genesis and recompute every hash. Anchoring
//! closes that hole: every company's chain head (last sequence number and
//! last hash) becomes a leaf in a Merkle tree, and the single root is
//! published outside the database (transparency endpoint, RFC 3161
//! timestamp token). Once a root exists outside the adversary's control,
//! any rewrite of anchored history is provable: the voucher at the
//! anchored sequence no longer carries the anchored hash.
//!
//! One root covers every tenant, and reveals nothing about any of them —
//! per-company facts stay behind access control; the inclusion proof lets
//! an authorized revisor connect *their* company's head to the public
//! root without seeing anyone else's.
//!
//! # Format (v1 — frozen, like the voucher hash formats)
//!
//! - Leaf content: netstring fields `"regnmed-anchor-v1"`, company id
//!   (16 raw bytes), chain seq (decimal), chain head hash (32 raw bytes).
//! - Leaf hash: `SHA-256(0x00 || content)`; interior node:
//!   `SHA-256(0x01 || left || right)` — the domain-separation prefixes
//!   make a leaf unforgeable as a node and vice versa.
//! - Leaves are sorted by company id before building the tree, so the
//!   root is independent of query order.
//! - Odd node at any level is promoted unchanged to the next level
//!   (Certificate Transparency style — never duplicated).
//!
//! The golden test pins the v1 root digest; the format can only be
//! superseded by a v2, never edited.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// One company's chain head, as covered by the anchor root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorLeaf {
    pub company_id: Uuid,
    pub last_seq: i64,
    pub last_hash: [u8; 32],
}

/// Which side the sibling sits on when hashing up the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofStep {
    pub side: Side,
    pub sibling: [u8; 32],
}

/// Path from one leaf to the root: fold the leaf hash through the steps
/// with [`verify_inclusion`]. Publishing a proof reveals only sibling
/// hashes, never other companies' data.
pub type InclusionProof = Vec<ProofStep>;

pub fn leaf_hash(leaf: &AnchorLeaf) -> [u8; 32] {
    let mut buf = Vec::with_capacity(96);
    buf.push(0x00);
    push_field(&mut buf, b"regnmed-anchor-v1");
    push_field(&mut buf, leaf.company_id.as_bytes());
    push_field(&mut buf, leaf.last_seq.to_string().as_bytes());
    push_field(&mut buf, &leaf.last_hash);
    Sha256::digest(&buf).into()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(65);
    buf.push(0x01);
    buf.extend_from_slice(left);
    buf.extend_from_slice(right);
    Sha256::digest(&buf).into()
}

fn sorted(leaves: &[AnchorLeaf]) -> Vec<AnchorLeaf> {
    let mut leaves = leaves.to_vec();
    leaves.sort_by_key(|l| l.company_id);
    leaves
}

/// Merkle root over the leaves (sorted by company id internally).
/// `None` for an empty set — there is nothing to anchor.
pub fn merkle_root(leaves: &[AnchorLeaf]) -> Option<[u8; 32]> {
    let mut level: Vec<[u8; 32]> = sorted(leaves).iter().map(leaf_hash).collect();
    if level.is_empty() {
        return None;
    }
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [left, right] => node_hash(left, right),
                [odd] => *odd,
                _ => unreachable!(),
            })
            .collect();
    }
    Some(level[0])
}

/// Path from the given company's leaf to the root, or `None` if the
/// company is not among the leaves.
pub fn inclusion_proof(leaves: &[AnchorLeaf], company_id: Uuid) -> Option<InclusionProof> {
    let leaves = sorted(leaves);
    let mut index = leaves.iter().position(|l| l.company_id == company_id)?;
    let mut level: Vec<[u8; 32]> = leaves.iter().map(leaf_hash).collect();
    let mut proof = Vec::new();
    while level.len() > 1 {
        let sibling = if index % 2 == 0 { index + 1 } else { index - 1 };
        if sibling < level.len() {
            proof.push(ProofStep {
                side: if sibling < index {
                    Side::Left
                } else {
                    Side::Right
                },
                sibling: level[sibling],
            });
        }
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [left, right] => node_hash(left, right),
                [odd] => *odd,
                _ => unreachable!(),
            })
            .collect();
        index /= 2;
    }
    Some(proof)
}

/// True when the leaf folds through the proof to exactly the root.
pub fn verify_inclusion(leaf: &AnchorLeaf, proof: &InclusionProof, root: &[u8; 32]) -> bool {
    let mut hash = leaf_hash(leaf);
    for step in proof {
        hash = match step.side {
            Side::Left => node_hash(&step.sibling, &hash),
            Side::Right => node_hash(&hash, &step.sibling),
        };
    }
    &hash == root
}

fn push_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(bytes.len().to_string().as_bytes());
    buf.push(b':');
    buf.extend_from_slice(bytes);
    buf.push(b';');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(n: u128) -> Vec<AnchorLeaf> {
        (1..=n)
            .map(|i| AnchorLeaf {
                company_id: Uuid::from_u128(i),
                last_seq: i as i64 * 10,
                last_hash: [i as u8; 32],
            })
            .collect()
    }

    /// Pins the v1 anchor format forever: if this digest changes, roots
    /// already witnessed externally stop verifying — the format can only
    /// be superseded, never edited.
    #[test]
    fn golden_root_never_changes() {
        let root = merkle_root(&leaves(3)).unwrap();
        assert_eq!(
            root.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "79c9ecf01bda78fd05002e8f715d08e7d39ba1990adc6601ef79d6ac4fb9eec1"
        );
    }

    #[test]
    fn root_is_independent_of_input_order() {
        let mut shuffled = leaves(5);
        shuffled.reverse();
        assert_eq!(merkle_root(&leaves(5)), merkle_root(&shuffled));
    }

    #[test]
    fn empty_set_has_no_root_and_single_leaf_roots_as_its_hash() {
        assert_eq!(merkle_root(&[]), None);
        let one = leaves(1);
        assert_eq!(merkle_root(&one), Some(leaf_hash(&one[0])));
    }

    #[test]
    fn inclusion_proofs_verify_for_every_leaf_at_every_size() {
        for n in 1..=8u128 {
            let set = leaves(n);
            let root = merkle_root(&set).unwrap();
            for leaf in &set {
                let proof = inclusion_proof(&set, leaf.company_id).unwrap();
                assert!(
                    verify_inclusion(leaf, &proof, &root),
                    "size {n}, company {}",
                    leaf.company_id
                );
            }
        }
    }

    #[test]
    fn a_tampered_leaf_fails_its_proof() {
        let set = leaves(4);
        let root = merkle_root(&set).unwrap();
        let proof = inclusion_proof(&set, set[1].company_id).unwrap();
        let mut tampered = set[1].clone();
        tampered.last_seq += 1;
        assert!(!verify_inclusion(&tampered, &proof, &root));
        let mut rewritten = set[1].clone();
        rewritten.last_hash = [0xAB; 32];
        assert!(!verify_inclusion(&rewritten, &proof, &root));
        // A proof for one company never verifies another company's leaf.
        assert!(!verify_inclusion(&set[2], &proof, &root));
    }

    #[test]
    fn unknown_company_has_no_proof() {
        assert_eq!(inclusion_proof(&leaves(3), Uuid::from_u128(99)), None);
    }
}
