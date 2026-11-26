//! The e-mail notice of new customer terms (docs/kundevilkar.md §14.1,
//! docs/vilkar-teknisk.md), sent by `regnmed terms-notice`. Here rather
//! than in the CLI binary so it can be tested against a real stream.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use regnmed_db::terms::{NoticeDue, VERSION, effective_date, notices_due, record_notice};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{OutboundMail, jetstream, publish};

/// Tells every company bound by an earlier version about the current one,
/// once. One result per company: sent, or why not. A company with no
/// address is reported and tried again on the next run.
pub async fn send_terms_notices(
    pool: &PgPool,
    js: &jetstream::Context,
    portal_base: &str,
    today: NaiveDate,
) -> Result<Vec<(NoticeDue, Result<String>)>> {
    let from: NaiveDate = effective_date()
        .parse()
        .context("the current terms have no effective date in docs/kundevilkar/")?;
    let url = format!("{}/vilkar/{}", portal_base.trim_end_matches('/'), *VERSION);
    let mut outcomes = Vec::new();
    for due in notices_due(pool, &VERSION).await? {
        let sent = async {
            let to = due.recipient.clone().context(
                "no e-mail address: neither the company nor the person who accepted has one",
            )?;
            let (subject, text) = notice_text(&due.company_name, from, today, &url);
            let id = Uuid::now_v7();
            publish(
                js,
                &OutboundMail {
                    id,
                    to: to.clone(),
                    subject,
                    text,
                    reply_to: None,
                    attachments: Vec::new(),
                },
            )
            .await?;
            record_notice(pool, id, due.company_id, &VERSION, &to).await?;
            Ok(to)
        }
        .await;
        outcomes.push((due, sent));
    }
    Ok(outcomes)
}

/// Subject and body. A version already in force says so instead of naming
/// a deadline that has passed.
fn notice_text(company: &str, from: NaiveDate, today: NaiveDate, url: &str) -> (String, String) {
    let date = from.format("%d.%m.%Y");
    let consequence = if from > today {
        format!(
            "Er de ikke godtatt innen {date}, kan regnmed fra den dagen ikke sende a-melding og \
             andre oppgaver til myndighetene på selskapets vegne før de er det."
        )
    } else {
        "Til de er godtatt, kan regnmed ikke sende a-melding og andre oppgaver til \
         myndighetene på selskapets vegne."
            .to_string()
    };
    (
        format!("Nye kundevilkår for regnmed fra {date}"),
        format!(
            "Hei,\n\n\
             Kundevilkårene for regnmed er endret. {company} har godtatt en tidligere versjon, \
             og den nye gjelder fra {date}.\n\n\
             Les de nye vilkårene: {url}\n\n\
             En administrator i {company} godtar dem i regnmed under Administrasjon → \
             Firmaopplysninger. {consequence} Bokføringen og tilgangen til selskapets \
             opplysninger berøres ikke.\n\n\
             Dette varselet sendes etter kundevilkårene punkt 14.1.\n\n\
             Hilsen\nregnmed (BogenTech AS)\n"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_a_deadline_only_while_it_lies_ahead() {
        let from = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap();
        let before = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
        let (subject, text) = notice_text("Eksempel AS", from, before, "https://x/vilkar/abc");
        assert_eq!(subject, "Nye kundevilkår for regnmed fra 01.03.2027");
        assert!(text.contains("innen 01.03.2027"), "{text}");
        assert!(text.contains("https://x/vilkar/abc"));
        assert!(text.contains("Eksempel AS har godtatt"));

        let (_, text) = notice_text("Eksempel AS", from, from, "https://x/vilkar/abc");
        assert!(!text.contains("innen"), "{text}");
        assert!(text.contains("Til de er godtatt"));
    }

    /// A company bound by an earlier version is told once, at its own
    /// address or else the accepting person's; one without either is
    /// reported; one that already accepted, or never accepted, is not
    /// told. Needs DATABASE_URL and nats-server on PATH (skips without).
    #[tokio::test]
    async fn each_bound_company_is_told_once() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let Some(nats) = crate::testnats::start().await else {
            return;
        };
        let pool = regnmed_db::connect(&url).await.unwrap();
        // The oldest archived draft: a version every test company may hold.
        const OLD: &str = "f2d730ceb33dde33c066d7cc74bfb9d0c725e3958aea3e0711fab11da3a885cd";
        let orgnumber = || format!("{:09}", Uuid::new_v4().as_u128() % 1_000_000_000);
        let person = |email: Option<&'static str>| {
            let pool = pool.clone();
            async move {
                regnmed_db::ensure_person(
                    &pool,
                    &format!("terms|{}", Uuid::new_v4()),
                    Some("Admin"),
                    email,
                )
                .await
                .unwrap()
            }
        };
        let company = |name: &'static str,
                       email: Option<&'static str>,
                       sha: Option<&'static str>,
                       who: Uuid| {
            let pool = pool.clone();
            async move {
                let id = regnmed_db::create_company(&pool, &orgnumber(), name)
                    .await
                    .unwrap();
                sqlx::query("update company set email = $2 where id = $1")
                    .bind(id)
                    .bind(email)
                    .execute(&pool)
                    .await
                    .unwrap();
                if let Some(sha) = sha {
                    regnmed_db::terms::accept(&pool, id, who, sha, "Admin")
                        .await
                        .unwrap();
                }
                id
            }
        };
        let with_mail = person(Some("admin@varsel.example")).await;
        let without = person(None).await;
        let own = company(
            "Egen adresse AS",
            Some("post@varsel.example"),
            Some(OLD),
            without,
        )
        .await;
        let fallback = company("Uten adresse AS", None, Some(OLD), with_mail).await;
        let nobody = company("Ingen adresse AS", None, Some(OLD), without).await;
        let current = company(
            "Allerede godtatt AS",
            Some("x@varsel.example"),
            Some(OLD),
            without,
        )
        .await;
        regnmed_db::terms::accept(&pool, current, without, &VERSION, "Admin")
            .await
            .unwrap();
        let never = company("Aldri godtatt AS", Some("y@varsel.example"), None, without).await;

        let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let run = || send_terms_notices(&pool, &nats.js, "https://regnmed.example/", today);
        let outcome = |all: &[(NoticeDue, Result<String>)], id: Uuid| {
            all.iter()
                .find(|(d, _)| d.company_id == id)
                .map(|(_, r)| r.as_ref().map(String::clone).map_err(|e| format!("{e:#}")))
        };
        let first = run().await.unwrap();
        assert_eq!(outcome(&first, own), Some(Ok("post@varsel.example".into())));
        assert_eq!(
            outcome(&first, fallback),
            Some(Ok("admin@varsel.example".into()))
        );
        assert!(
            outcome(&first, nobody)
                .unwrap()
                .unwrap_err()
                .contains("no e-mail address")
        );
        assert_eq!(outcome(&first, current), None);
        assert_eq!(outcome(&first, never), None);

        // Told once; the one without an address is tried again.
        let second = run().await.unwrap();
        assert_eq!(outcome(&second, own), None);
        assert_eq!(outcome(&second, fallback), None);
        assert!(outcome(&second, nobody).unwrap().is_err());

        let recipients: Vec<String> = sqlx::query_scalar(
            "select recipient from terms_notice where company_id = any($1) order by recipient",
        )
        .bind(vec![own, fallback, nobody])
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(recipients, ["admin@varsel.example", "post@varsel.example"]);
    }
}
