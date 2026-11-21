//! Finanstilsynets virksomhetsregister: does an orgnr hold an active
//! autorisasjon as regnskapsførerselskap or revisjonsselskap?
//!
//! Uses the register's public API (`/registry/v2`, published OpenAPI at
//! `/registry/swagger/v2/swagger.json`, docs/marketplace.md). It lists current
//! licences only and has no test environment, so synthetic Tenor companies are
//! simply not in it. **Without a configured URL the check fails closed** —
//! nobody becomes a verified firm because a register was unreachable.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResults {
    legal_entities: Option<Vec<SearchHit>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchHit {
    legal_entity_id: i64,
    organisation_number: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegalEntities {
    legal_entities: Option<Vec<LegalEntity>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegalEntity {
    legal_entity_id: i64,
    organisation_number: Option<String>,
    licences: Option<Vec<Licence>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Licence {
    licensed_entity: Option<EntityReference>,
    licence_type: Option<LicenceType>,
    service_provider_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntityReference {
    legal_entity_id: i64,
}

#[derive(Debug, Deserialize)]
struct LicenceType {
    code: String,
}

/// The register's licence codes that let a business offer this service.
struct Accepted {
    firm: &'static [&'static str],
    sole_proprietor: &'static str,
}

fn accepted(kind: &str) -> Result<Accepted> {
    Ok(match kind {
        "regnskap" => Accepted {
            firm: &["REGS", "REGNSN"],
            sole_proprietor: "REGP",
        },
        "revisjon" => Accepted {
            firm: &["REVS"],
            sole_proprietor: "REVP",
        },
        other => bail!("unknown autorisasjon kind '{other}'"),
    })
}

/// A firm must hold the licence itself. A sole proprietorship is listed with
/// its owner's personal licence. An agent or branch of a licensee is neither.
fn holds(entity: &LegalEntity, accepted: &Accepted) -> bool {
    entity.licences.iter().flatten().any(|licence| {
        let Some(code) = licence.licence_type.as_ref().map(|t| t.code.as_str()) else {
            return false;
        };
        match licence.service_provider_type.as_deref() {
            Some("Licensed") => {
                accepted.firm.contains(&code)
                    && licence
                        .licensed_entity
                        .as_ref()
                        .is_some_and(|e| e.legal_entity_id == entity.legal_entity_id)
            }
            Some("SoleProprietorship") => code == accepted.sole_proprietor,
            _ => false,
        }
    })
}

pub struct FinanstilsynetClient {
    base_url: Option<String>,
    http: reqwest::Client,
}

impl FinanstilsynetClient {
    pub fn from_env() -> Self {
        Self::new(std::env::var("FINANSTILSYNET_API_URL").ok())
    }

    fn new(base_url: Option<String>) -> Self {
        Self {
            base_url,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("a TLS backend"),
        }
    }

    async fn fetch<T: DeserializeOwned>(&self, url: &str, query: &[(&str, String)]) -> Result<T> {
        let response = self
            .http
            .get(url)
            .query(query)
            .header("accept", "application/json")
            .send()
            .await
            .context("Finanstilsynets register unreachable")?;
        if !response.status().is_success() {
            bail!("Finanstilsynets register returned {}", response.status());
        }
        response
            .json()
            .await
            .context("parsing Finanstilsynet response")
    }

    /// True when the orgnr holds an active autorisasjon of the kind
    /// ('regnskap' or 'revisjon'). Fails closed: no configured register,
    /// unreachable register, or unknown orgnr all mean "not verified".
    pub async fn has_authorisation(&self, orgnumber: &str, kind: &str) -> Result<bool> {
        let accepted = accepted(kind)?;
        let Some(base) = &self.base_url else {
            bail!(
                "FINANSTILSYNET_API_URL is not configured — autorisasjon \
                 cannot be verified (see docs/marketplace.md)"
            );
        };
        let base = base.trim_end_matches('/');
        let search: SearchResults = self
            .fetch(
                &format!("{base}/legal-entities/search"),
                &[("query", orgnumber.to_string()), ("pageSize", "50".into())],
            )
            .await?;
        // The search is free text, so only an exact orgnr counts as a hit.
        let ids: Vec<(&str, String)> = search
            .legal_entities
            .unwrap_or_default()
            .into_iter()
            .filter(|hit| hit.organisation_number.as_deref() == Some(orgnumber))
            .map(|hit| ("ids", hit.legal_entity_id.to_string()))
            .collect();
        if ids.is_empty() {
            return Ok(false);
        }
        let found: LegalEntities = self.fetch(&format!("{base}/legal-entities"), &ids).await?;
        Ok(found
            .legal_entities
            .unwrap_or_default()
            .iter()
            .filter(|entity| entity.organisation_number.as_deref() == Some(orgnumber))
            .any(|entity| holds(entity, &accepted)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real `/legal-entities` answer, trimmed to what we read.
    fn entity(licensee: i64, code: &str, provider: &str) -> LegalEntity {
        serde_json::from_value(serde_json::json!({
            "legalEntityId": 7,
            "legalEntityType": "Company",
            "organisationNumber": "991937099",
            "name": "REGNSKAP AS",
            "licences": [{
                "licensedEntity": {"legalEntityId": licensee, "legalEntityName": "X"},
                "serviceProviderId": 1,
                "licenceType": {"code": code, "name": {"norwegian": "X", "english": "X"}},
                "licenceClassification": null,
                "registeredDate": "2007-12-10T00:00:00",
                "serviceProviderType": provider,
                "remarks": null
            }]
        }))
        .unwrap()
    }

    fn counts(e: &LegalEntity, kind: &str) -> bool {
        holds(e, &accepted(kind).unwrap())
    }

    #[test]
    fn a_firm_needs_its_own_licence_of_the_right_kind() {
        assert!(counts(&entity(7, "REGS", "Licensed"), "regnskap"));
        assert!(counts(&entity(7, "REGNSN", "Licensed"), "regnskap"));
        assert!(counts(&entity(7, "REVS", "Licensed"), "revisjon"));
        assert!(!counts(&entity(7, "REGS", "Licensed"), "revisjon"));
        assert!(!counts(&entity(7, "REVTHIRD", "Licensed"), "revisjon"));
        assert!(!counts(&entity(9, "REGS", "Licensed"), "regnskap"));
    }

    #[test]
    fn a_sole_proprietorship_counts_through_its_owner() {
        assert!(counts(&entity(9, "REGP", "SoleProprietorship"), "regnskap"));
        assert!(counts(&entity(9, "REVP", "SoleProprietorship"), "revisjon"));
        assert!(!counts(&entity(9, "REGP", "Licensed"), "regnskap"));
    }

    #[test]
    fn an_agent_or_branch_of_a_licensee_does_not_count() {
        assert!(!counts(&entity(7, "REGS", "Agent"), "regnskap"));
        assert!(!counts(&entity(7, "REGS", "Branch"), "regnskap"));
    }

    #[tokio::test]
    async fn no_register_means_not_verified() {
        let client = FinanstilsynetClient::new(None);
        assert!(
            client
                .has_authorisation("991937099", "regnskap")
                .await
                .is_err()
        );
    }

    /// Against the real register:
    /// `cargo nextest run -p regnmed-gov --run-ignored only finanstilsynet`.
    #[tokio::test]
    #[ignore = "calls Finanstilsynet's live public API"]
    async fn the_live_register_answers_as_this_adapter_expects() {
        let client =
            FinanstilsynetClient::new(Some("https://api.finanstilsynet.no/registry/v2".into()));
        for (orgnumber, kind, expected) in [
            ("991937099", "regnskap", true),
            ("991937099", "revisjon", false),
            ("927515911", "revisjon", true),
            ("935115086", "regnskap", false),
        ] {
            let answer = client.has_authorisation(orgnumber, kind).await.unwrap();
            assert_eq!(answer, expected, "{orgnumber} {kind}");
        }
    }
}
