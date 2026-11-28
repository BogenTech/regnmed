//! Payments to self-employed people without a fixed place of business
//! (third-party report, formerly RF-1321; migration 0112,
//! docs/selvstendig.md, #363).
//!
//! Three things:
//!
//! - **The assessment** (`contractor_assessment`): the customer's own
//!   judgement, per supplier. Skatteetaten leaves it to the payer, so
//!   regnmed never infers it. Only a supplier marked yes is reported;
//!   suppliers nobody has assessed are listed as a warning, never silently
//!   in or out.
//! - **The figures**, read from the supplier ledger: what each supplier
//!   invoiced in the year excluding VAT, net of credit notes (skfvf.
//!   § 7-10-24 (1)). Payments are not counted. See [`report`].
//! - **The submission record**: what is sent is stored before it is sent,
//!   with the key it is always sent with (the 0076 rule).

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::contractor_payments::{
    self as core, Contact, Delivery, Recipient, RecipientId, VoucherShare,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// `kildesystem` in every delivery.
const SOURCE_SYSTEM: &str = "regnmed";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Assessment {
    pub self_employed: bool,
    /// Only for a supplier without an orgnr.
    pub nin: Option<String>,
    pub decided_by: String,
    pub created_at: DateTime<Utc>,
}

/// Records the customer's judgement on one supplier. A fødselsnummer (or
/// D-nummer) is taken only for a supplier marked yes that has no orgnr:
/// the rettledning says the orgnr is used when there is one.
pub async fn assess(
    pool: &PgPool,
    company_id: Uuid,
    party_id: Uuid,
    self_employed: bool,
    nin: Option<&str>,
    by: &str,
) -> Result<()> {
    let row = sqlx::query("select kind, orgnr from party where id = $1 and company_id = $2")
        .bind(party_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("ukjent leverandør")?;
    ensure!(
        row.get::<String, _>("kind") == "leverandor",
        "bare leverandører vurderes som selvstendig næringsdrivende"
    );
    let nin = nin.map(str::trim).filter(|n| !n.is_empty());
    if let Some(n) = nin {
        ensure!(
            self_employed,
            "fødselsnummer lagres bare for en leverandør som er selvstendig næringsdrivende"
        );
        ensure!(
            row.get::<Option<String>, _>("orgnr").is_none(),
            "leverandøren har organisasjonsnummer, og det er det som rapporteres"
        );
        ensure!(
            regnmed_core::nin::is_valid(n),
            "«{n}» er ikke et gyldig fødselsnummer eller D-nummer"
        );
    }
    sqlx::query(
        "insert into contractor_assessment
             (id, company_id, party_id, self_employed, nin, decided_by)
         values ($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(party_id)
    .bind(self_employed)
    .bind(nin)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every judgement on the supplier, newest first: the first one applies.
pub async fn assessments(
    pool: &PgPool,
    company_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<Assessment>> {
    let rows = sqlx::query(
        "select self_employed, nin, decided_by, created_at from contractor_assessment
         where company_id = $1 and party_id = $2 order by created_at desc, id desc",
    )
    .bind(company_id)
    .bind(party_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Assessment {
            self_employed: r.get("self_employed"),
            nin: r.get("nin"),
            decided_by: r.get("decided_by"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// One supplier with invoices in the year.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Line {
    pub party_id: Uuid,
    pub party_no: String,
    pub name: String,
    pub orgnumber: Option<String>,
    /// Never serialized: the portal shows the birth date, the download
    /// carries the number.
    #[serde(skip)]
    pub nin: Option<String>,
    /// Invoiced excluding VAT, net of credit notes, øre.
    pub invoiced_ore: i64,
}

impl Line {
    fn id(&self) -> Option<RecipientId> {
        match (&self.orgnumber, &self.nin) {
            (Some(o), _) => Some(RecipientId::Orgnumber(o.clone())),
            (None, Some(n)) => Some(RecipientId::Nin(n.clone())),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Withdrawn {
    pub name: String,
    /// May be a fødselsnummer, so never serialized.
    #[serde(skip)]
    pub id: RecipientId,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    pub income_year: i32,
    /// Marked self-employed, with a net to report.
    pub recipients: Vec<Line>,
    /// Records an earlier delivery made that the ledger no longer
    /// supports (marked no since, or credited to nothing): sent again with
    /// 0, which is how Skatteetaten deletes one.
    pub withdrawn: Vec<Withdrawn>,
    /// Suppliers with invoices in the year nobody has assessed. A warning:
    /// the decision on 2026-11-28 was to warn, not to block.
    pub unassessed: Vec<Line>,
    /// What stops the report from being delivered.
    pub hindrances: Vec<String>,
}

/// Every supplier's invoiced amount in the year excluding VAT, from the
/// supplier ledger (docs/selvstendig.md, «Tallene»):
///
/// - An entry on a supplier-ledger account is an invoice when it credits
///   the supplier and a credit note when it debits it, unless money moved
///   the other way in the same voucher (an account 19xx): then it is a
///   payment or a refund, and is not counted.
/// - The VAT is the voucher's own on 2700–2719, shared pro rata if the
///   voucher holds several suppliers.
async fn invoiced(pool: &PgPool, company_id: Uuid, year: i32) -> Result<Vec<(Uuid, i64)>> {
    let start = NaiveDate::from_ymd_opt(year, 1, 1).context("ugyldig år")?;
    let end = NaiveDate::from_ymd_opt(year, 12, 31).context("ugyldig år")?;
    let rows = sqlx::query(
        "with sup as (
             select e.voucher_id, e.party_id, e.amount_ore
             from entry e
             join voucher v on v.id = e.voucher_id
             join account a on a.id = e.account_id
             where v.company_id = $1 and v.voucher_date between $2 and $3
               and a.reskontro_kind = 'leverandor'
         ), vo as (
             select e.voucher_id,
                    coalesce(sum(e.amount_ore) filter (where a.number like '19%'), 0) as money,
                    coalesce(sum(e.amount_ore)
                             filter (where a.number between '2700' and '2719'), 0) as vat
             from entry e
             join account a on a.id = e.account_id
             where e.voucher_id in (select voucher_id from sup)
             group by e.voucher_id
         )
         select s.voucher_id, s.party_id, -sum(s.amount_ore)::bigint as gross, vo.vat::bigint as vat
         from sup s join vo on vo.voucher_id = s.voucher_id
         where (s.amount_ore < 0 and vo.money <= 0) or (s.amount_ore > 0 and vo.money >= 0)
         group by s.voucher_id, s.party_id, vo.vat",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;
    let shares: Vec<VoucherShare<Uuid>> = rows
        .iter()
        .map(|r| VoucherShare {
            voucher: r.get("voucher_id"),
            party: r.get("party_id"),
            gross_ore: r.get("gross"),
            voucher_vat_ore: r.get("vat"),
        })
        .collect();
    Ok(core::invoiced_excl_vat(&shares))
}

/// The year's report as the ledger and the assessments stand now.
pub async fn report(pool: &PgPool, company_id: Uuid, year: i32) -> Result<Report> {
    let invoiced = invoiced(pool, company_id, year).await?;
    let ids: Vec<Uuid> = invoiced.iter().map(|(p, _)| *p).collect();
    let rows = sqlx::query(
        "select p.id, p.party_no, p.name, p.orgnr, a.self_employed, a.nin
         from party p
         left join lateral (
             select self_employed, nin from contractor_assessment c
             where c.company_id = p.company_id and c.party_id = p.id
             order by c.created_at desc, c.id desc limit 1
         ) a on true
         where p.company_id = $1 and p.id = any($2)",
    )
    .bind(company_id)
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut report = Report {
        income_year: year,
        recipients: Vec::new(),
        withdrawn: Vec::new(),
        unassessed: Vec::new(),
        hindrances: Vec::new(),
    };
    for (party, ore) in invoiced {
        let r = rows
            .iter()
            .find(|r| r.get::<Uuid, _>("id") == party)
            .context("leverandøren finnes ikke")?;
        let line = Line {
            party_id: party,
            party_no: r.get("party_no"),
            name: r.get("name"),
            orgnumber: r.get("orgnr"),
            nin: r.get("nin"),
            invoiced_ore: ore,
        };
        match r.get::<Option<bool>, _>("self_employed") {
            None if ore > 0 => report.unassessed.push(line),
            // Nothing is reported at a net of 0 or below, and an amount
            // that rounds to 0 kroner is no record either.
            Some(true) if core::kroner(ore) > 0 => {
                if line.id().is_none() {
                    report.hindrances.push(format!(
                        "{} ({}) mangler organisasjonsnummer og fødselsnummer",
                        line.name, line.party_no
                    ));
                }
                report.recipients.push(line);
            }
            _ => {}
        }
    }
    let by_name = |a: &Line, b: &Line| a.name.cmp(&b.name).then(a.party_no.cmp(&b.party_no));
    report.recipients.sort_by(by_name);
    report.unassessed.sort_by(by_name);

    // What the newest delivery reported that is no longer here.
    if let Some(doc) = newest_delivered_document(pool, company_id, year).await? {
        let now: Vec<RecipientId> = report.recipients.iter().filter_map(Line::id).collect();
        for (id, name, amount) in records_in(&doc)? {
            if amount > 0 && !now.contains(&id) {
                report.withdrawn.push(Withdrawn { name, id });
            }
        }
    }
    if report.recipients.is_empty() && report.withdrawn.is_empty() {
        report.hindrances.push(format!(
            "ingen selvstendig næringsdrivende å rapportere for {year}"
        ));
    }
    Ok(report)
}

/// (identifier, name, amount in kroner) of every record in a delivered
/// document.
fn records_in(document: &str) -> Result<Vec<(RecipientId, String, i64)>> {
    let v: serde_json::Value = serde_json::from_str(document)?;
    let mut out = Vec::new();
    for o in v["leveranse"]["oppgave"].as_array().into_iter().flatten() {
        let owner = &o["oppgaveeier"];
        let id = match (
            owner["organisasjonsnummer"].as_str(),
            owner["foedselsnummer"].as_str(),
        ) {
            (Some(org), _) => RecipientId::Orgnumber(org.into()),
            (None, Some(n)) => RecipientId::Nin(n.into()),
            _ => bail!("en levert oppgave mangler identifikator"),
        };
        out.push((
            id,
            owner["navn"].as_str().unwrap_or_default().to_string(),
            o["betaltBeloep"].as_i64().unwrap_or_default(),
        ));
    }
    Ok(out)
}

/// The JSON for the year with this delivery reference. Refuses, naming
/// every hindrance, a report that cannot be delivered.
pub async fn document(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    reference: &str,
) -> Result<String> {
    let report = report(pool, company_id, year).await?;
    ensure!(
        report.hindrances.is_empty(),
        "oppgaven over betalinger til selvstendig næringsdrivende kan ikke leveres: {}",
        report.hindrances.join("; ")
    );
    let settings = crate::company_settings(pool, company_id).await?;
    let mut recipients: Vec<Recipient> = report
        .recipients
        .iter()
        .map(|l| Recipient {
            id: l.id().expect("no hindrance, so every line has an id"),
            name: l.name.clone(),
            paid_ore: l.invoiced_ore,
        })
        .collect();
    for w in &report.withdrawn {
        recipients.push(Recipient {
            id: w.id.clone(),
            name: w.name.clone(),
            paid_ore: 0,
        });
    }
    Ok(core::render(&Delivery {
        income_year: year,
        payer_orgnumber: settings.orgnumber,
        reference: reference.to_string(),
        source_system: SOURCE_SYSTEM.into(),
        // The company, not the person: a retry by someone else must render
        // the same bytes.
        contact: Contact {
            name: Some(settings.name),
            phone: None,
            email: settings.email,
        },
        recipients,
    })?)
}

async fn newest_delivered_document(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "select s.document from contractor_report_submission s
         where s.company_id = $1 and s.income_year = $2
           and exists (select 1 from contractor_report_submission_event e
                       where e.submission_id = s.id and e.kind = 'receipt')
         order by s.created_at desc limit 1",
    )
    .bind(company_id)
    .bind(year)
    .fetch_optional(pool)
    .await?)
}

// --- Submission --------------------------------------------------------------

pub struct Rail {
    pub tokens: regnmed_gov::maskinporten::TokenProvider,
    pub client: regnmed_gov::contractor_payments::ContractorPaymentsClient,
}

impl Rail {
    /// None without MASKINPORTEN_CLIENT_ID: the rail is off, not broken.
    pub fn from_env() -> Result<Option<Self>> {
        if std::env::var("MASKINPORTEN_CLIENT_ID").is_err() {
            return Ok(None);
        }
        let mut config = regnmed_gov::maskinporten::MaskinportenConfig::from_env()?;
        config.scopes = regnmed_gov::contractor_payments::SCOPES.into();
        Ok(Some(Self {
            tokens: regnmed_gov::maskinporten::TokenProvider::new(config),
            client: regnmed_gov::contractor_payments::ContractorPaymentsClient::from_env(),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Submission {
    pub id: Uuid,
    #[serde(rename = "inntektsar")]
    pub income_year: i32,
    #[serde(rename = "antall_oppgaver")]
    pub records: usize,
    #[serde(rename = "opprettet_av")]
    pub created_by: String,
    #[serde(rename = "opprettet")]
    pub created_at: DateTime<Utc>,
    #[serde(skip)]
    pub document: String,
    #[serde(skip)]
    pub idempotency_key: Uuid,
    /// `ikke_levert`, `levert`, `godkjent`, `avvist` or `behandlet`.
    pub status: &'static str,
    #[serde(rename = "siste_feil")]
    pub last_error: Option<String>,
    pub forsendelse_id: Option<String>,
    #[serde(skip)]
    pub dialog_id: Option<String>,
    /// Skatteetaten's answer (`regnmed_gov::shareholder_return::Outcome`).
    #[serde(rename = "svar")]
    pub outcome: Option<serde_json::Value>,
}

/// The year's submissions, newest first.
pub async fn submissions(pool: &PgPool, company_id: Uuid, year: i32) -> Result<Vec<Submission>> {
    let rows = sqlx::query(
        "select s.id, s.income_year, s.document, s.idempotency_key, s.created_by, s.created_at,
                r.dialog_id, r.forsendelse_id, o.detail as outcome,
                (select f.detail->>'error' from contractor_report_submission_event f
                  where f.submission_id = s.id and f.kind = 'failed'
                  order by f.created_at desc limit 1) as last_error
         from contractor_report_submission s
         left join contractor_report_submission_event r
                on r.submission_id = s.id and r.kind = 'receipt'
         left join contractor_report_submission_event o
                on o.submission_id = s.id and o.kind = 'outcome'
         where s.company_id = $1 and s.income_year = $2
         order by s.created_at desc",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            let document: String = r.get("document");
            let forsendelse_id: Option<String> = r.get("forsendelse_id");
            let outcome: Option<serde_json::Value> = r.get("outcome");
            let status = match (&forsendelse_id, &outcome) {
                (None, _) => "ikke_levert",
                (Some(_), None) => "levert",
                (Some(_), Some(o)) => match o["kind"].as_str() {
                    Some("Acceptance") => "godkjent",
                    Some("Rejection") => "avvist",
                    _ => "behandlet",
                },
            };
            Ok(Submission {
                id: r.get("id"),
                income_year: r.get("income_year"),
                records: records_in(&document)?.len(),
                created_by: r.get("created_by"),
                created_at: r.get("created_at"),
                document,
                idempotency_key: r.get("idempotency_key"),
                status,
                last_error: r.get("last_error"),
                forsendelse_id,
                dialog_id: r.get("dialog_id"),
                outcome,
            })
        })
        .collect()
}

/// The delivery to send for `year` now: the newest undelivered one when
/// the report still renders the same (a retry: same bytes, same key),
/// otherwise a new one. A report identical to the newest delivered one is
/// refused: there is nothing to correct. Anything else after a delivery is
/// a correction, and Skatteetaten replaces record by record.
async fn prepare(pool: &PgPool, company_id: Uuid, year: i32, by: &str) -> Result<Submission> {
    let mut tx = pool.begin().await?;
    // Two clicks at once must not store two deliveries.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("contractor_payments:{company_id}:{year}"))
        .execute(&mut *tx)
        .await?;
    let existing = submissions(pool, company_id, year).await?;
    if let Some(s) = existing.first() {
        // The stored id is the reference inside the stored document, so
        // re-rendering with it shows whether anything changed.
        let same = document(pool, company_id, year, &s.id.to_string()).await? == s.document;
        match (s.status, same) {
            ("ikke_levert", true) => return Ok(s.clone()),
            ("ikke_levert", false) => {}
            (_, true) => bail!(
                "oppgaven for {year} er allerede levert slik den står nå. Rett leverandørene \
                 eller bilagene først om noe er feil"
            ),
            (_, false) => {}
        }
    }
    let id = Uuid::now_v7();
    let doc = document(pool, company_id, year, &id.to_string()).await?;
    sqlx::query(
        "insert into contractor_report_submission
             (id, company_id, income_year, document, idempotency_key, created_by)
         values ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(company_id)
    .bind(year)
    .bind(&doc)
    .bind(Uuid::new_v4())
    .bind(by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    submissions(pool, company_id, year)
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .context("the delivery just stored")
}

/// Files the year's report with Skatteetaten: stored first, sent second.
/// Refused unless the company has accepted the terms in force.
pub async fn submit(
    pool: &PgPool,
    company_id: Uuid,
    rail: &Rail,
    year: i32,
    by: &str,
) -> Result<Submission> {
    ensure!(
        crate::terms::may_file(&crate::terms::acceptances(pool, company_id).await?),
        "selskapet har ikke godtatt gjeldende kundevilkår for kall mot myndighetene, så \
         oppgaven sendes ikke. En administrator godtar dem under Administrasjon → \
         Firmaopplysninger."
    );
    let s = prepare(pool, company_id, year, by).await?;
    let orgnumber = crate::company_settings(pool, company_id).await?.orgnumber;
    match rail
        .client
        .submit(
            &rail.tokens,
            &orgnumber,
            year,
            &s.document,
            s.idempotency_key,
        )
        .await
    {
        Ok(receipt) => {
            record(
                pool,
                s.id,
                "receipt",
                Some((&receipt.dialog_id, &receipt.forsendelse_id)),
                None,
                by,
            )
            .await?
        }
        Err(e) => {
            let e = format!("{e:#}");
            record(
                pool,
                s.id,
                "failed",
                None,
                Some(serde_json::json!({ "error": e })),
                by,
            )
            .await?;
            bail!(
                "Skatteetaten tok ikke imot oppgaven: {e}. Den er lagret og sendes uendret \
                 (samme idempotensnøkkel) neste gang du sender, så lenge tallene ikke er \
                 endret — den leveres ikke to ganger."
            );
        }
    }
    submissions(pool, company_id, year)
        .await?
        .into_iter()
        .find(|x| x.id == s.id)
        .context("the delivery just sent")
}

/// Fetches Skatteetaten's answer for a delivered submission that waits for
/// one, and stores it when it exists.
pub async fn fetch_outcome(
    pool: &PgPool,
    company_id: Uuid,
    rail: &Rail,
    submission_id: Uuid,
    by: &str,
) -> Result<()> {
    let year: i32 = sqlx::query_scalar(
        "select income_year from contractor_report_submission where id = $1 and company_id = $2",
    )
    .bind(submission_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent innsending")?;
    let s = submissions(pool, company_id, year)
        .await?
        .into_iter()
        .find(|s| s.id == submission_id)
        .context("ukjent innsending")?;
    let (Some(dialog), Some(forsendelse), None) = (&s.dialog_id, &s.forsendelse_id, &s.outcome)
    else {
        return Ok(());
    };
    let orgnumber = crate::company_settings(pool, company_id).await?.orgnumber;
    if let Some(o) = rail
        .client
        .outcome(&rail.tokens, &orgnumber, dialog, forsendelse)
        .await?
    {
        record(
            pool,
            s.id,
            "outcome",
            None,
            Some(serde_json::to_value(o)?),
            by,
        )
        .await?;
    }
    Ok(())
}

async fn record(
    pool: &PgPool,
    submission_id: Uuid,
    kind: &str,
    receipt: Option<(&str, &str)>,
    detail: Option<serde_json::Value>,
    by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into contractor_report_submission_event
             (id, submission_id, kind, dialog_id, forsendelse_id, detail, created_by)
         values ($1,$2,$3,$4,$5,$6,$7)
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(submission_id)
    .bind(kind)
    .bind(receipt.map(|r| r.0))
    .bind(receipt.map(|r| r.1))
    .bind(detail)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}
