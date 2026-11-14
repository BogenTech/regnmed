//! Driving distance and the route line between two points, from
//! OpenRouteService (api.openrouteservice.org). Configured by
//! `ORS_API_KEY` — a key per installation, kept in the environment and
//! never in the repo (docs/secrets.md). Without it there is no client and
//! the trip form simply asks for the kilometres, as before.
//!
//! The distance is a SUGGESTION: the claimant may have driven a detour
//! with good reason, and the claimed km is theirs to state. What this
//! gives is a cross-check and a picture of the route.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::addresses::Point;

pub struct RouteClient {
    base_url: String,
    key: String,
    http: reqwest::Client,
}

#[derive(Debug, PartialEq)]
pub struct Route {
    pub metres: f64,
    /// [lat, lon] pairs along the way — Leaflet's order, not GeoJSON's.
    pub line: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    properties: Properties,
    geometry: Geometry,
}

#[derive(Deserialize)]
struct Properties {
    summary: Summary,
}

#[derive(Deserialize)]
struct Summary {
    #[serde(default)]
    distance: f64,
}

#[derive(Deserialize)]
struct Geometry {
    coordinates: Vec<[f64; 2]>,
}

/// The first route of a GeoJSON directions answer. GeoJSON is [lon, lat].
fn parse(body: &str) -> Result<Route> {
    let collection: Collection =
        serde_json::from_str(body).context("parsing OpenRouteService response")?;
    let feature = collection
        .features
        .into_iter()
        .next()
        .context("OpenRouteService found no route")?;
    Ok(Route {
        metres: feature.properties.summary.distance,
        line: feature
            .geometry
            .coordinates
            .into_iter()
            .map(|[lon, lat]| [lat, lon])
            .collect(),
    })
}

impl RouteClient {
    /// None when no key is configured.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("ORS_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())?;
        Some(Self {
            base_url: std::env::var("ORS_API_URL")
                .unwrap_or_else(|_| "https://api.openrouteservice.org".into()),
            key,
            http: crate::http(),
        })
    }

    pub async fn drive(&self, from: Point, to: Point) -> Result<Route> {
        let response = self
            .http
            .post(format!(
                "{}/v2/directions/driving-car/geojson",
                self.base_url.trim_end_matches('/')
            ))
            .header("authorization", &self.key)
            .timeout(std::time::Duration::from_secs(8))
            .json(&serde_json::json!({
                "coordinates": [[from.lon, from.lat], [to.lon, to.lat]],
                "geometry_simplify": true,
            }))
            .send()
            .await
            .context("OpenRouteService unreachable")?;
        if !response.status().is_success() {
            bail!("OpenRouteService returned {}", response.status());
        }
        parse(&response.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The documented GeoJSON shape (openrouteservice.org/dev, directions
    // v2), cut to what we read. Not yet checked against a live answer.
    #[test]
    fn the_first_route_reads_as_metres_and_lat_lon() {
        let body = r#"{"type":"FeatureCollection","features":[{"type":"Feature",
            "properties":{"summary":{"distance":41873.2,"duration":2412.5}},
            "geometry":{"type":"LineString","coordinates":[[10.7522,59.9139],[10.2032,59.7433]]}}]}"#;
        assert_eq!(
            parse(body).unwrap(),
            Route {
                metres: 41873.2,
                line: vec![[59.9139, 10.7522], [59.7433, 10.2032]]
            }
        );
        assert!(parse(r#"{"features":[]}"#).is_err());
    }
}
