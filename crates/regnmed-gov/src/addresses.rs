//! Address suggestions from Kartverket's open address register
//! (ws.geonorge.no/adresser, no key). Used by the trip form: the portal
//! asks regnmed, regnmed asks Kartverket — the CSP stays `connect-src
//! 'self'` and the user's IP never reaches the third party. A
//! suggestion is a convenience, never a requirement: the caller treats
//! any failure as "no suggestions".

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub struct AddressClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct Response {
    adresser: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    adressetekst: String,
    postnummer: Option<String>,
    poststed: Option<String>,
    representasjonspunkt: Option<Point>,
}

/// A point in EUROREF89 geographic (EPSG:4258) — GPS-compatible degrees.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
}

/// The first hit's point, if it has one.
fn parse_point(body: &str) -> Result<Option<Point>> {
    let response: Response = serde_json::from_str(body).context("parsing Kartverket response")?;
    Ok(response
        .adresser
        .into_iter()
        .find_map(|h| h.representasjonspunkt))
}

/// "Storgata 1, 0155 OSLO" — how the register writes it, one per hit.
fn parse(body: &str) -> Result<Vec<String>> {
    let response: Response = serde_json::from_str(body).context("parsing Kartverket response")?;
    Ok(response
        .adresser
        .into_iter()
        .map(|h| match (h.postnummer, h.poststed) {
            (Some(nr), Some(sted)) => format!("{}, {nr} {sted}", h.adressetekst),
            _ => h.adressetekst,
        })
        .collect())
}

impl AddressClient {
    pub fn from_env() -> Self {
        Self {
            base_url: std::env::var("ADDRESS_API_URL")
                .unwrap_or_else(|_| "https://ws.geonorge.no/adresser/v1".into()),
            http: reqwest::Client::builder()
                // Suggestions that arrive after the user typed on are noise.
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .expect("static client config"),
        }
    }

    async fn get(&self, query: &[(&str, &str)]) -> Result<String> {
        let response = self
            .http
            .get(format!("{}/sok", self.base_url.trim_end_matches('/')))
            .query(query)
            .send()
            .await
            .context("Kartverket unreachable")?;
        if !response.status().is_success() {
            bail!("Kartverket returned {}", response.status());
        }
        Ok(response.text().await?)
    }

    pub async fn search(&self, text: &str) -> Result<Vec<String>> {
        parse(
            &self
                .get(&[("sok", text), ("fuzzy", "true"), ("treffPerSide", "8")])
                .await?,
        )
    }

    /// Where an address is. The field may hold "Name, street, postcode
    /// town" (a company picked from the suggestions) or a c/o line, so a
    /// miss drops the leading part and tries again — the street and town
    /// are always last. None when nothing is left to try.
    pub async fn locate(&self, text: &str) -> Result<Option<Point>> {
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        for start in 0..parts.len() {
            let candidate = parts[start..].join(", ");
            let body = self
                .get(&[
                    ("sok", &candidate),
                    ("treffPerSide", "1"),
                    ("utkoordsys", "4258"),
                ])
                .await?;
            if let Some(point) = parse_point(&body)? {
                return Ok(Some(point));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_read_as_one_line_each() {
        let body = r#"{"metadata":{"totaltAntallTreff":2},"adresser":[
            {"adressetekst":"Storgata 1","postnummer":"0155","poststed":"OSLO","kommunenavn":"OSLO"},
            {"adressetekst":"Brygga","postnummer":null,"poststed":null}]}"#;
        assert_eq!(
            parse(body).unwrap(),
            vec!["Storgata 1, 0155 OSLO", "Brygga"]
        );
    }

    #[test]
    fn the_first_hit_with_a_point_locates_it() {
        let body = r#"{"adresser":[{"adressetekst":"Bragernes torg 1",
            "representasjonspunkt":{"epsg":"EPSG:4258","lat":59.74328,"lon":10.20324}}]}"#;
        assert_eq!(
            parse_point(body).unwrap(),
            Some(Point {
                lat: 59.74328,
                lon: 10.20324
            })
        );
        assert_eq!(parse_point(r#"{"adresser":[]}"#).unwrap(), None);
    }
}
