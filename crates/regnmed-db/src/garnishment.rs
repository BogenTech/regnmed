//! Trekkpålegg registered by hand (docs/trekkpalegg.md §6, #339, migration
//! 0110), and the parts of the payroll run every order goes through: which
//! orders apply on a pay date, the account the trekk is credited to and the
//! payee as a supplier.
//!
//! Orders fetched from Skatteetaten (#334) are not built. They are meant to
//! reach the run as more [`Effective`] rows; the rule that a fetched order
//! replaces a hand entry with the same KID belongs to #334.

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::garnishment::{self as core, Draft, Kind, Size};
use regnmed_core::hash::sha256;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

/// The account trekk goes to when the company has chosen none (§2 step 5).
pub const DEFAULT_ACCOUNT: &str = "2610";
const ACCOUNT_NAME: &str = "Utleggstrekk";

// ---- the letter ----------------------------------------------------------

/// Stores the letter an order came in. Returns its id, which a
/// registration then names. PDF or image only.
pub async fn upload_letter(
    pool: &PgPool,
    company_id: Uuid,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
    uploaded_by: &str,
) -> Result<Uuid> {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    ensure!(
        content_type == "application/pdf" || content_type.starts_with("image/"),
        "brevet må være en PDF eller et bilde"
    );
    ensure!(!bytes.is_empty(), "brevet er tomt");
    ensure!(!filename.trim().is_empty(), "oppgi filnavnet");
    let id = Uuid::now_v7();
    let in_row = crate::blob::blobs().store(bytes).await?;
    sqlx::query(
        "insert into garnishment_letter
             (id, company_id, filename, content_type, byte_size, sha256, content, uploaded_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(company_id)
    .bind(filename.trim())
    .bind(&content_type)
    .bind(bytes.len() as i32)
    .bind(sha256(bytes).as_slice())
    .bind(in_row)
    .bind(uploaded_by)
    .execute(pool)
    .await?;
    Ok(id)
}

/// The letter's name, type and bytes, checked against the stored hash.
pub async fn letter(
    pool: &PgPool,
    company_id: Uuid,
    letter_id: Uuid,
) -> Result<(String, String, Vec<u8>)> {
    let row = sqlx::query(
        "select filename, content_type, sha256, content from garnishment_letter
         where id = $1 and company_id = $2",
    )
    .bind(letter_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent brev")?;
    let hash: Vec<u8> = row.get("sha256");
    let bytes = crate::blob::blobs().load(row.get("content"), &hash).await?;
    Ok((row.get("filename"), row.get("content_type"), bytes))
}

// ---- orders and versions -------------------------------------------------

#[derive(Debug, Clone)]
pub struct Version {
    pub id: Uuid,
    pub version: i32,
    pub size: Size,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub kid: Option<String>,
    pub payee_orgnumber: Option<String>,
    pub payee_account: Option<String>,
    pub due_days: Option<i32>,
    pub letter_id: Uuid,
    pub letter_filename: String,
    pub stopped: bool,
    pub needs_attestation: bool,
    /// The newest attestation decision: "godkjent" or "avvist".
    pub attestation: Option<String>,
    pub attested_by: Option<String>,
    pub note: Option<String>,
    pub created_by_person: Uuid,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

impl Version {
    /// Whether this version may be withheld: a stop always counts; an
    /// ordinary version once attested, or when none was required.
    fn in_force(&self) -> bool {
        self.stopped || !self.needs_attestation || self.attestation.as_deref() == Some("godkjent")
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub kind: Kind,
    pub created_at: DateTime<Utc>,
    /// Newest first.
    pub versions: Vec<Version>,
}

impl Order {
    /// The version that governs, and the newer ones still waiting for (or
    /// refused at) attestation. `None` when stopped or never attested.
    fn governing(&self) -> (Option<&Version>, Vec<&Version>) {
        let mut waiting = Vec::new();
        for v in &self.versions {
            if v.in_force() {
                return ((!v.stopped).then_some(v), waiting);
            }
            waiting.push(v);
        }
        (None, waiting)
    }

    /// For the portal: «aktiv», «stoppet», «venter på attestering»,
    /// «avvist ved attestering».
    pub fn status(&self) -> &'static str {
        match self.versions.first() {
            Some(v) if v.stopped => "stoppet",
            Some(v) if v.in_force() => "aktiv",
            Some(v) if v.attestation.as_deref() == Some("avvist") => "avvist ved attestering",
            _ => "venter på attestering",
        }
    }
}

/// Every hand-registered order with all its versions.
pub async fn list(pool: &PgPool, company_id: Uuid) -> Result<Vec<Order>> {
    load(&mut *pool.acquire().await?, company_id).await
}

async fn load(conn: &mut PgConnection, company_id: Uuid) -> Result<Vec<Order>> {
    let rows = sqlx::query(
        "select g.id as order_id, g.employee_id, e.navn, g.kind, g.created_at as order_created,
                v.id, v.version, v.amount_ore, v.percent_bp, v.valid_from, v.valid_to, v.kid,
                v.payee_orgnumber, v.payee_account, v.due_days, v.letter_id, l.filename,
                v.stopped, v.needs_attestation, v.note, v.created_by_person, v.created_by,
                v.created_at, a.decision, a.decided_by
         from garnishment g
         join employee e on e.id = g.employee_id
         join garnishment_version v on v.garnishment_id = g.id
         join garnishment_letter l on l.id = v.letter_id
         left join lateral (
             select decision, decided_by from attestation
             where company_id = g.company_id and target_kind = 'garnishment_version'
               and target_id = v.id
             order by created_at desc, id desc limit 1
         ) a on true
         where g.company_id = $1
         order by g.created_at, g.id, v.version desc",
    )
    .bind(company_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut orders: Vec<Order> = Vec::new();
    for r in rows {
        let order_id: Uuid = r.get("order_id");
        if orders.last().is_none_or(|o| o.id != order_id) {
            let kind: String = r.get("kind");
            orders.push(Order {
                id: order_id,
                employee_id: r.get("employee_id"),
                employee_name: r.get("navn"),
                kind: Kind::from_slug(&kind).context("unknown garnishment kind")?,
                created_at: r.get("order_created"),
                versions: Vec::new(),
            });
        }
        let amount: Option<i64> = r.get("amount_ore");
        let percent: Option<i32> = r.get("percent_bp");
        orders
            .last_mut()
            .expect("pushed above")
            .versions
            .push(Version {
                id: r.get("id"),
                version: r.get("version"),
                size: match (amount, percent) {
                    (Some(a), _) => Size::AmountOre(a),
                    (None, Some(p)) => Size::PercentBp(p.into()),
                    (None, None) => bail!("garnishment version without a size"),
                },
                valid_from: r.get("valid_from"),
                valid_to: r.get("valid_to"),
                kid: r.get("kid"),
                payee_orgnumber: r.get("payee_orgnumber"),
                payee_account: r.get("payee_account"),
                due_days: r.get("due_days"),
                letter_id: r.get("letter_id"),
                letter_filename: r.get("filename"),
                stopped: r.get("stopped"),
                needs_attestation: r.get("needs_attestation"),
                attestation: r.get("decision"),
                attested_by: r.get("decided_by"),
                note: r.get("note"),
                created_by_person: r.get("created_by_person"),
                created_by: r.get("created_by"),
                created_at: r.get("created_at"),
            });
    }
    Ok(orders)
}

/// Who writes a version: the person (for the four-eyes check) and the
/// name shown in the trail.
pub struct Author<'a> {
    pub person_id: Uuid,
    pub name: &'a str,
}

/// What a new version says. On a change the type stays the order's.
pub struct VersionDraft {
    pub draft: Draft,
    /// Required on registration; on a change, `None` keeps the letter.
    pub letter_id: Option<Uuid>,
    pub note: Option<String>,
}

/// Registers a new order from a letter: version 1.
pub async fn register(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: Uuid,
    new: VersionDraft,
    by: &Author<'_>,
) -> Result<(Uuid, i32)> {
    let draft = core::validate(new.draft).map_err(anyhow::Error::msg)?;
    let letter_id = new.letter_id.context("last opp brevet pålegget kom i")?;
    let mut tx = pool.begin().await?;
    lock_company(&mut tx, company_id).await?;
    let employee: Option<String> =
        sqlx::query_scalar("select navn from employee where id = $1 and company_id = $2")
            .bind(employee_id)
            .bind(company_id)
            .fetch_optional(&mut *tx)
            .await?;
    ensure!(employee.is_some(), "ukjent ansatt");
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into garnishment (id, company_id, employee_id, kind, created_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(company_id)
    .bind(employee_id)
    .bind(draft.kind.slug())
    .bind(by.name)
    .execute(&mut *tx)
    .await?;
    insert_version(
        &mut tx, company_id, id, 1, &draft, letter_id, false, new.note, by,
    )
    .await?;
    tx.commit().await?;
    Ok((id, 1))
}

/// A new version with changed terms. Not after a stop: a stopped order is
/// history, and a new letter is a new registration.
pub async fn change(
    pool: &PgPool,
    company_id: Uuid,
    order_id: Uuid,
    new: VersionDraft,
    by: &Author<'_>,
) -> Result<i32> {
    let mut tx = pool.begin().await?;
    lock_company(&mut tx, company_id).await?;
    let order = one(&mut tx, company_id, order_id).await?;
    let latest = order.versions.first().context("order without versions")?;
    ensure!(
        !latest.stopped,
        "pålegget er stoppet — et nytt brev registreres som et nytt pålegg"
    );
    ensure!(
        new.draft.kind == order.kind,
        "typen kan ikke endres — stopp pålegget og registrer et nytt"
    );
    let draft = core::validate(new.draft).map_err(anyhow::Error::msg)?;
    let next = latest.version + 1;
    insert_version(
        &mut tx,
        company_id,
        order_id,
        next,
        &draft,
        new.letter_id.unwrap_or(latest.letter_id),
        false,
        new.note,
        by,
    )
    .await?;
    tx.commit().await?;
    Ok(next)
}

/// Stops an order: a version from which nothing is withheld. Needs no
/// attestation — stopping cannot take money from anyone.
pub async fn stop(
    pool: &PgPool,
    company_id: Uuid,
    order_id: Uuid,
    note: Option<String>,
    by: &Author<'_>,
) -> Result<i32> {
    let mut tx = pool.begin().await?;
    lock_company(&mut tx, company_id).await?;
    let order = one(&mut tx, company_id, order_id).await?;
    let latest = order.versions.first().context("order without versions")?;
    ensure!(!latest.stopped, "pålegget er allerede stoppet");
    let draft = Draft {
        kind: order.kind,
        size: latest.size,
        valid_from: latest.valid_from,
        valid_to: latest.valid_to,
        kid: latest.kid.clone(),
        payee_orgnumber: latest.payee_orgnumber.clone(),
        payee_account: latest.payee_account.clone(),
        due_days: latest.due_days,
    };
    let next = latest.version + 1;
    insert_version(
        &mut tx,
        company_id,
        order_id,
        next,
        &draft,
        latest.letter_id,
        true,
        note,
        by,
    )
    .await?;
    tx.commit().await?;
    Ok(next)
}

async fn one(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    order_id: Uuid,
) -> Result<Order> {
    load(&mut *tx, company_id)
        .await?
        .into_iter()
        .find(|o| o.id == order_id)
        .context("ukjent trekkpålegg")
}

/// Registrations and runs take the same lock, so a run's transaction sees
/// exactly the orders it was checked against (§3 point 2).
async fn lock_company(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
) -> Result<()> {
    sqlx::query("select 1 from company where id = $1 for no key update")
        .bind(company_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    order_id: Uuid,
    version: i32,
    d: &Draft,
    letter_id: Uuid,
    stopped: bool,
    note: Option<String>,
    by: &Author<'_>,
) -> Result<()> {
    let letter: Option<Uuid> =
        sqlx::query_scalar("select id from garnishment_letter where id = $1 and company_id = $2")
            .bind(letter_id)
            .bind(company_id)
            .fetch_optional(&mut **tx)
            .await?;
    ensure!(letter.is_some(), "ukjent brev");
    // A stop needs nobody's approval; anything else does when the company
    // has switched attestation on (docs/attestering.md).
    let needs_attestation = !stopped
        && crate::approval::current_policy(&mut **tx, company_id)
            .await?
            .is_some_and(|p| p.active);
    let (amount, percent) = match d.size {
        Size::AmountOre(a) => (Some(a), None),
        Size::PercentBp(p) => (None, Some(i32::try_from(p)?)),
    };
    sqlx::query(
        "insert into garnishment_version
             (id, garnishment_id, version, amount_ore, percent_bp, valid_from, valid_to, kid,
              payee_orgnumber, payee_account, due_days, letter_id, stopped, needs_attestation,
              note, created_by_person, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
    )
    .bind(Uuid::now_v7())
    .bind(order_id)
    .bind(version)
    .bind(amount)
    .bind(percent)
    .bind(d.valid_from)
    .bind(d.valid_to)
    .bind(&d.kid)
    .bind(&d.payee_orgnumber)
    .bind(&d.payee_account)
    .bind(d.due_days)
    .bind(letter_id)
    .bind(stopped)
    .bind(needs_attestation)
    .bind(note.as_deref().map(str::trim).filter(|n| !n.is_empty()))
    .bind(by.person_id)
    .bind(by.name)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Attests one version (docs/attestering.md): someone other than the person
/// who registered it, and the designated attestant when the policy names
/// one. Re-deciding appends; the newest decision governs.
pub async fn attest(
    pool: &PgPool,
    company_id: Uuid,
    order_id: Uuid,
    version: i32,
    approved: bool,
    note: Option<&str>,
    by: &Author<'_>,
) -> Result<()> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    ensure!(
        approved || note.is_some(),
        "en avvisning i attestering krever et notat"
    );
    if let Some(policy) = crate::approval::current_policy(pool, company_id).await?
        && policy.active
        && let Some(attestant) = policy.attestant_person_id
        && attestant != by.person_id
    {
        bail!("policyen utpeker en annen attestant for dette selskapet");
    }
    let row = sqlx::query(
        "select v.id, v.needs_attestation, v.created_by_person from garnishment_version v
         join garnishment g on g.id = v.garnishment_id
         where g.id = $1 and g.company_id = $2 and v.version = $3",
    )
    .bind(order_id)
    .bind(company_id)
    .bind(version)
    .fetch_optional(pool)
    .await?
    .context("ukjent versjon av trekkpålegget")?;
    ensure!(
        row.get::<bool, _>("needs_attestation"),
        "denne versjonen krever ikke attestering"
    );
    ensure!(
        row.get::<Uuid, _>("created_by_person") != by.person_id,
        "den som registrerte pålegget kan ikke attestere det selv"
    );
    sqlx::query(
        "insert into attestation (id, company_id, target_kind, target_id, decision, note,
                                  decided_by_person, decided_by)
         values ($1, $2, 'garnishment_version', $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(row.get::<Uuid, _>("id"))
    .bind(if approved { "godkjent" } else { "avvist" })
    .bind(note)
    .bind(by.person_id)
    .bind(by.name)
    .execute(pool)
    .await?;
    Ok(())
}

// ---- the payroll settings ------------------------------------------------

/// The payroll settings for utleggstrekk (§2 step 5, §6). Every row is the
/// whole set; the newest with `valid_from` on or before a date applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub valid_from: Option<NaiveDate>,
    pub account_number: Option<String>,
    pub tax_kid: Option<String>,
    pub tax_account: Option<String>,
    pub tax_payee_orgnumber: Option<String>,
    pub set_by: String,
    pub created_at: Option<DateTime<Utc>>,
}

fn settings_from_row(r: &sqlx::postgres::PgRow) -> Settings {
    Settings {
        valid_from: Some(r.get("valid_from")),
        account_number: r.get("account_number"),
        tax_kid: r.get("tax_kid"),
        tax_account: r.get("tax_account"),
        tax_payee_orgnumber: r.get("tax_payee_orgnumber"),
        set_by: r.get("set_by"),
        created_at: Some(r.get("created_at")),
    }
}

const SETTINGS_COLUMNS: &str =
    "valid_from, account_number, tax_kid, tax_account, tax_payee_orgnumber, set_by, created_at";

/// The settings in force on `date`.
pub async fn settings_on(
    conn: &mut PgConnection,
    company_id: Uuid,
    date: NaiveDate,
) -> Result<Option<Settings>> {
    let row = sqlx::query(&format!(
        "select {SETTINGS_COLUMNS} from payroll_garnishment_setting
         where company_id = $1 and valid_from <= $2
         order by valid_from desc, created_at desc, id desc limit 1"
    ))
    .bind(company_id)
    .bind(date)
    .fetch_optional(conn)
    .await?;
    Ok(row.as_ref().map(settings_from_row))
}

/// Every settings row, newest first: who chose which account, when.
pub async fn settings_history(pool: &PgPool, company_id: Uuid) -> Result<Vec<Settings>> {
    let rows = sqlx::query(&format!(
        "select {SETTINGS_COLUMNS} from payroll_garnishment_setting
         where company_id = $1 order by valid_from desc, created_at desc, id desc"
    ))
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(settings_from_row).collect())
}

/// Records a new settings row. The account must exist, be active and be
/// a supplier subledger; the skatt details are checked like an order's.
pub async fn set_settings(pool: &PgPool, company_id: Uuid, s: &Settings) -> Result<()> {
    let blank = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let account = blank(&s.account_number);
    let kid = blank(&s.tax_kid);
    let tax_account =
        blank(&s.tax_account).map(|a| regnmed_core::pain001::normalize_account_number(&a));
    let payee = blank(&s.tax_payee_orgnumber);
    if let Some(n) = &account {
        let ok: Option<bool> = sqlx::query_scalar(
            "select active and reskontro_kind is not distinct from 'leverandor' from account
             where company_id = $1 and number = $2",
        )
        .bind(company_id)
        .bind(n)
        .fetch_optional(pool)
        .await?;
        ensure!(ok.is_some(), "konto {n} finnes ikke i kontoplanen");
        ensure!(
            ok == Some(true),
            "konto {n} må være aktiv og leverandørreskontro for å føre utleggstrekk"
        );
    }
    if let Some(k) = &kid {
        ensure!(
            regnmed_core::kid::is_valid(k),
            "KID {k} har feil kontrollsiffer"
        );
    }
    if let Some(a) = &tax_account {
        ensure!(
            regnmed_core::pain001::is_valid_account_number(a),
            "kontonummer {a} er ugyldig (11 siffer, MOD11)"
        );
    }
    if let Some(o) = &payee {
        ensure!(
            regnmed_core::orgnumber::is_valid(o),
            "organisasjonsnummer {o} er ugyldig (MOD11)"
        );
    }
    insert_settings(
        &mut *pool.acquire().await?,
        company_id,
        &Settings {
            valid_from: Some(s.valid_from.unwrap_or_else(|| Utc::now().date_naive())),
            account_number: account,
            tax_kid: kid,
            tax_account,
            tax_payee_orgnumber: payee,
            set_by: s.set_by.clone(),
            created_at: None,
        },
    )
    .await
}

async fn insert_settings(conn: &mut PgConnection, company_id: Uuid, s: &Settings) -> Result<()> {
    sqlx::query(
        "insert into payroll_garnishment_setting
             (id, company_id, valid_from, account_number, tax_kid, tax_account,
              tax_payee_orgnumber, set_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(s.valid_from)
    .bind(&s.account_number)
    .bind(&s.tax_kid)
    .bind(&s.tax_account)
    .bind(&s.tax_payee_orgnumber)
    .bind(&s.set_by)
    .execute(conn)
    .await?;
    Ok(())
}

// ---- what the run uses ---------------------------------------------------

/// One order the run withholds on a pay date, whatever its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub garnishment_id: Uuid,
    pub version: i32,
    pub employee_id: Uuid,
    pub kind: Kind,
    pub size: Size,
    pub kid: Option<String>,
    pub payee_orgnumber: Option<String>,
    pub payee_account: Option<String>,
    pub due_days: Option<i32>,
    pub letter_id: Uuid,
    pub letter_filename: String,
}

/// The orders in force on `paid_date`, in the order the cap is spent on
/// them (samordnet, skatt, øvrige; oldest first within a type), and
/// warnings for registrations that wait for attestation.
pub async fn effective(
    conn: &mut PgConnection,
    company_id: Uuid,
    paid_date: NaiveDate,
) -> Result<(Vec<Effective>, Vec<String>)> {
    let mut orders = load(conn, company_id).await?;
    orders.sort_by_key(|o| (o.kind, o.created_at, o.id));
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for o in &orders {
        let (governing, waiting) = o.governing();
        let applies = governing
            .filter(|v| v.valid_from <= paid_date && v.valid_to.is_none_or(|to| paid_date <= to));
        for w in waiting {
            let refused = w.attestation.as_deref() == Some("avvist");
            warnings.push(format!(
                "{}: trekkpålegget ({}, versjon {}) {} og trekkes ikke{}.",
                o.employee_name,
                o.kind.label(),
                w.version,
                if refused {
                    "er avvist ved attestering"
                } else {
                    "venter på attestering"
                },
                match governing {
                    Some(g) => format!(" — versjon {} trekkes i stedet", g.version),
                    None => String::new(),
                },
            ));
        }
        if let Some(v) = applies {
            out.push(Effective {
                garnishment_id: o.id,
                version: v.version,
                employee_id: o.employee_id,
                kind: o.kind,
                size: v.size,
                kid: v.kid.clone(),
                payee_orgnumber: v.payee_orgnumber.clone(),
                payee_account: v.payee_account.clone(),
                due_days: v.due_days,
                letter_id: v.letter_id,
                letter_filename: v.letter_filename.clone(),
            });
        }
    }
    Ok((out, warnings))
}

/// The account trekk is credited to, and whether the run must create it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChoice {
    pub number: String,
    pub create: bool,
    /// Not yet the company's chosen account: the run remembers it.
    pub remember: bool,
}

/// §2 step 5: the chosen account; else 2610 when it is (or can become) a
/// supplier subledger; else the first FREE number in 2611–2619. An existing
/// account there is never used unchosen, since it may carry other debt.
pub async fn resolve_account(
    conn: &mut PgConnection,
    company_id: Uuid,
    settings: Option<&Settings>,
) -> Result<AccountChoice> {
    let accounts: Vec<(String, bool, Option<String>)> = sqlx::query_as(
        "select number, active, reskontro_kind from account
         where company_id = $1 and number between '2610' and '2619'
            or company_id = $1 and number = $2",
    )
    .bind(company_id)
    .bind(
        settings
            .and_then(|s| s.account_number.as_deref())
            .unwrap_or(""),
    )
    .fetch_all(conn)
    .await?;
    let find = |n: &str| accounts.iter().find(|a| a.0 == n);
    if let Some(n) = settings.and_then(|s| s.account_number.as_deref()) {
        ensure!(
            find(n).is_some_and(|a| a.1 && a.2.as_deref() == Some("leverandor")),
            "kontoen for utleggstrekk, {n}, er ikke lenger en aktiv leverandørreskontro — velg en \
             annen under Lønn → Trekkpålegg"
        );
        return Ok(AccountChoice {
            number: n.into(),
            create: false,
            remember: false,
        });
    }
    match find(DEFAULT_ACCOUNT) {
        None => Ok(AccountChoice {
            number: DEFAULT_ACCOUNT.into(),
            create: true,
            remember: true,
        }),
        Some(a) if a.1 && a.2.as_deref() == Some("leverandor") => Ok(AccountChoice {
            number: DEFAULT_ACCOUNT.into(),
            create: false,
            remember: true,
        }),
        Some(_) => (2611..=2619)
            .map(|n| n.to_string())
            .find(|n| find(n).is_none())
            .map(|number| AccountChoice {
                number,
                create: true,
                remember: true,
            })
            .context(
                "det finnes ingen trygg konto å føre utleggstrekket på: 2610 er ikke \
                 leverandørreskontro, og 2611–2619 er alle i bruk. Velg en \
                 leverandørreskontrokonto for utleggstrekk under Lønn → Trekkpålegg før lønnen \
                 kjøres.",
            ),
    }
}

/// Creates the chosen account and remembers it, inside the run.
pub async fn establish_account(
    conn: &mut PgConnection,
    company_id: Uuid,
    choice: &AccountChoice,
    settings: Option<&Settings>,
    from: NaiveDate,
    by: &str,
) -> Result<()> {
    if choice.create {
        sqlx::query(
            "insert into account (id, company_id, number, name, reskontro_kind)
             values ($1, $2, $3, $4, 'leverandor')",
        )
        .bind(Uuid::now_v7())
        .bind(company_id)
        .bind(&choice.number)
        .bind(ACCOUNT_NAME)
        .execute(&mut *conn)
        .await?;
    }
    if choice.remember {
        let mut s = settings.cloned().unwrap_or_default();
        s.valid_from = Some(from);
        s.account_number = Some(choice.number.clone());
        s.set_by = by.to_string();
        insert_settings(conn, company_id, &s).await?;
    }
    Ok(())
}

/// The payee as a supplier: the lowest-numbered supplier with the orgnr,
/// with its bank account set to the order's; created when none exists. The
/// payment list pays the party's account, so a stale one would pay the
/// wrong place. With `write` false nothing changes and the warnings say
/// what the run will do.
pub async fn supplier(
    conn: &mut PgConnection,
    company_id: Uuid,
    orgnumber: &str,
    bank_account: &str,
    write: bool,
    warnings: &mut Vec<String>,
) -> Result<String> {
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "select id, party_no, bank_account from party
         where company_id = $1 and kind = 'leverandor' and orgnr = $2
         order by party_no::bigint",
    )
    .bind(company_id)
    .bind(orgnumber)
    .fetch_all(&mut *conn)
    .await?;
    if let Some((id, party_no, account)) = rows.first() {
        if rows.len() > 1 {
            warnings.push(format!(
                "Det finnes {} leverandører med organisasjonsnummer {orgnumber}; utleggstrekket \
                 føres på nr. {party_no}.",
                rows.len()
            ));
        }
        if account.as_deref() != Some(bank_account) {
            warnings.push(format!(
                "Leverandør {party_no} får kontonummer {bank_account} fra trekkpålegget{}.",
                account
                    .as_deref()
                    .map(|a| format!(" (var {a})"))
                    .unwrap_or_default()
            ));
            if write {
                sqlx::query("update party set bank_account = $2 where id = $1")
                    .bind(id)
                    .bind(bank_account)
                    .execute(&mut *conn)
                    .await?;
            }
        }
        return Ok(party_no.clone());
    }
    let party_no: i64 = sqlx::query_scalar(
        "select coalesce(max(party_no::bigint) + 1, 50000)
         from party where company_id = $1 and kind = 'leverandor'",
    )
    .bind(company_id)
    .fetch_one(&mut *conn)
    .await?;
    warnings.push(format!(
        "Betalingsmottakeren {orgnumber} opprettes som leverandør nr. {party_no}."
    ));
    if write {
        sqlx::query(
            "insert into party (id, company_id, party_no, kind, name, orgnr, bank_account)
             values ($1, $2, $3, 'leverandor', $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(company_id)
        .bind(party_no.to_string())
        .bind(format!("Innkrevingsmyndigheten (org.nr. {orgnumber})"))
        .bind(orgnumber)
        .bind(bank_account)
        .execute(&mut *conn)
        .await?;
    }
    Ok(party_no.to_string())
}
