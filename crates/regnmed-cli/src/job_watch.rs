//! `regnmed job-watch`: judge every CronJob in this namespace and record
//! one healthy/unhealthy bit for the monitor outside the cluster
//! (docs/deploy.md). Names and reasons go to this pod's log only.
//!
//! An error here records nothing on purpose: the verdict then goes stale,
//! and a stale verdict reads as unhealthy.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::PgPool;

const SERVICE_ACCOUNT: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
const API_SERVER: &str = "https://kubernetes.default.svc";
/// Its own health is the freshness of the verdict it writes.
const SELF: &str = "job-watch";
const GRACE_HOURS: i64 = 6;

struct Watched {
    name: String,
    schedule: String,
    created: DateTime<Utc>,
    last_success: Option<DateTime<Utc>>,
    last_failure: Option<DateTime<Utc>>,
}

pub async fn run(pool: &PgPool) -> Result<()> {
    let namespace = std::fs::read_to_string(format!("{SERVICE_ACCOUNT}/namespace"))?;
    let token = std::fs::read_to_string(format!("{SERVICE_ACCOUNT}/token"))?;
    let ca = std::fs::read(format!("{SERVICE_ACCOUNT}/ca.crt"))?;
    let client = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(&ca)?)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let list = |kind: &'static str| {
        let request = client
            .get(format!(
                "{API_SERVER}/apis/batch/v1/namespaces/{}/{kind}",
                namespace.trim()
            ))
            .bearer_auth(token.trim());
        async move {
            let response = request.send().await?.error_for_status()?;
            anyhow::Ok(response.json::<Value>().await?)
        }
    };

    let watched = watched(&list("cronjobs").await?, &list("jobs").await?)?;
    let now = Utc::now();
    let problems: Vec<String> = watched.iter().filter_map(|w| problem(w, now)).collect();
    for problem in &problems {
        eprintln!("UNHEALTHY {problem}");
    }
    regnmed_db::record_job_health(pool, problems.is_empty()).await?;
    println!(
        "{} CronJobs watched, {} unhealthy",
        watched.len(),
        problems.len()
    );
    Ok(())
}

fn watched(cronjobs: &Value, jobs: &Value) -> Result<Vec<Watched>> {
    let mut last_failure: HashMap<&str, DateTime<Utc>> = HashMap::new();
    for job in jobs["items"].as_array().context("no job list")? {
        let owner = job["metadata"]["ownerReferences"]
            .as_array()
            .and_then(|o| o.iter().find(|o| o["kind"] == "CronJob"))
            .and_then(|o| o["name"].as_str());
        let failed = job["status"]["conditions"]
            .as_array()
            .and_then(|c| {
                c.iter()
                    .find(|c| c["type"] == "Failed" && c["status"] == "True")
            })
            .and_then(|c| time(&c["lastTransitionTime"]));
        if let (Some(owner), Some(failed)) = (owner, failed) {
            let newest = last_failure.entry(owner).or_insert(failed);
            *newest = failed.max(*newest);
        }
    }

    let mut watched = Vec::new();
    for cronjob in cronjobs["items"].as_array().context("no CronJob list")? {
        let name = cronjob["metadata"]["name"].as_str().context("no name")?;
        if name == SELF {
            continue;
        }
        watched.push(Watched {
            name: name.to_string(),
            schedule: cronjob["spec"]["schedule"]
                .as_str()
                .context("no schedule")?
                .to_string(),
            created: time(&cronjob["metadata"]["creationTimestamp"]).context("no created time")?,
            last_success: time(&cronjob["status"]["lastSuccessfulTime"]),
            last_failure: last_failure.get(name).copied(),
        });
    }
    // An empty list is a broken view of the cluster, never a healthy one.
    anyhow::ensure!(!watched.is_empty(), "no CronJobs to watch");
    Ok(watched)
}

fn time(value: &Value) -> Option<DateTime<Utc>> {
    value.as_str()?.parse().ok()
}

fn problem(job: &Watched, now: DateTime<Utc>) -> Option<String> {
    if let Some(failed) = job.last_failure
        && job.last_success.is_none_or(|ok| failed > ok)
    {
        return Some(format!("{}: last run failed at {failed}", job.name));
    }
    let Some(max_age) = max_age(&job.schedule) else {
        return Some(format!(
            "{}: cannot judge schedule {:?}",
            job.name, job.schedule
        ));
    };
    let since = job.last_success.unwrap_or(job.created);
    (now - since > max_age).then(|| format!("{}: no success since {since}", job.name))
}

/// The longest a healthy job can go between successes. Coarse by design: a
/// schedule between two shapes gets the longer bound, which only delays
/// the alert, and a shape not understood is reported rather than skipped.
fn max_age(schedule: &str) -> Option<Duration> {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    let [_, hour, day_of_month, "*", day_of_week] = fields[..] else {
        return None;
    };
    let period = match (hour, day_of_month, day_of_week) {
        ("*", "*", "*") => Duration::hours(1),
        (_, "*", "*") => Duration::days(1),
        (_, "*", _) => Duration::days(7),
        (_, _, "*") => Duration::days(31),
        _ => return None,
    };
    Some(period + Duration::hours(GRACE_HOURS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn job(schedule: &str, success: Option<&str>, failure: Option<&str>) -> Watched {
        Watched {
            name: "backup-verify".into(),
            schedule: schedule.into(),
            created: at("2026-11-01T00:00:00Z"),
            last_success: success.map(at),
            last_failure: failure.map(at),
        }
    }

    #[test]
    fn every_deployed_schedule_shape_has_a_bound() {
        for (schedule, hours) in [
            ("17 * * * *", 7),
            ("30 1 * * *", 30),
            ("0 4 * * 0", 174),
            ("0 7 1 * *", 750),
        ] {
            assert_eq!(
                max_age(schedule),
                Some(Duration::hours(hours)),
                "{schedule}"
            );
        }
        assert_eq!(max_age("0 0 1 1 *"), None);
        assert_eq!(max_age("@daily"), None);
    }

    #[test]
    fn a_failure_counts_until_a_newer_success() {
        let now = at("2026-11-22T05:00:00Z");
        let weekly = "0 4 * * 0";
        let failed = job(
            weekly,
            Some("2026-11-21T04:00:00Z"),
            Some("2026-11-22T04:00:00Z"),
        );
        assert!(problem(&failed, now).unwrap().contains("failed"));
        let recovered = job(
            weekly,
            Some("2026-11-22T04:30:00Z"),
            Some("2026-11-22T04:00:00Z"),
        );
        assert_eq!(problem(&recovered, now), None);
        let never_succeeded = job(weekly, None, Some("2026-11-22T04:00:00Z"));
        assert!(problem(&never_succeeded, now).is_some());
    }

    #[test]
    fn a_job_that_stops_running_is_caught_without_any_failure() {
        let daily = "30 1 * * *";
        let last = Some("2026-11-21T01:30:00Z");
        assert_eq!(
            problem(&job(daily, last, None), at("2026-11-22T07:00:00Z")),
            None
        );
        assert!(
            problem(&job(daily, last, None), at("2026-11-22T08:00:00Z"))
                .unwrap()
                .contains("no success")
        );
        // A job that never ran is measured from when it was created.
        assert!(problem(&job(daily, None, None), at("2026-11-02T07:00:00Z")).is_some());
        assert_eq!(
            problem(&job(daily, None, None), at("2026-11-01T12:00:00Z")),
            None
        );
    }

    #[test]
    fn a_failed_job_is_attributed_to_its_cronjob_and_the_watcher_skips_itself() {
        let cronjob = |name: &str| {
            json!({
                "metadata": {"name": name, "creationTimestamp": "2026-11-01T00:00:00Z"},
                "spec": {"schedule": "0 4 * * 0"},
                "status": {"lastSuccessfulTime": "2026-11-08T04:00:00Z"},
            })
        };
        let cronjobs = json!({"items": [cronjob("backup-verify"), cronjob(SELF)]});
        let jobs = json!({"items": [{
            "metadata": {"ownerReferences": [{"kind": "CronJob", "name": "backup-verify"}]},
            "status": {"conditions": [
                {"type": "FailureTarget", "status": "True", "lastTransitionTime": "2026-11-15T04:00:00Z"},
                {"type": "Failed", "status": "True", "lastTransitionTime": "2026-11-15T04:00:01Z"},
            ]},
        }]});
        let watched = watched(&cronjobs, &jobs).unwrap();
        assert_eq!(watched.len(), 1);
        assert_eq!(watched[0].last_failure, Some(at("2026-11-15T04:00:01Z")));
        assert!(super::watched(&json!({"items": [cronjob(SELF)]}), &jobs).is_err());
    }
}
