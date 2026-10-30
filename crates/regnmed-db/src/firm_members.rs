//! Byrå membership administration (#78, docs/marketplace.md).
//!
//! The mirror of `medlemmer` for firms, with the same disciplines and
//! for the same reasons: invitations are addressed to an e-mail address
//! and redeemed at login, the change trail is insert-only, and the firm
//! can never be left without an active admin — a firm without one is
//! not recoverable without DB access. The one structural difference is
//! what membership *reaches*: every active firm member has access to
//! every client of the firm, through the engagements (tenancy.rs), so
//! letting someone into the byrå is letting them into its client
//! portfolio. That is why everything here is firm-admin territory.

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::members::normalize_email;

/// The firm roles (0005): `admin` runs the byrå, `ansatt` works in it.
pub const ROLES: [&str; 2] = ["admin", "ansatt"];

fn require_role(role: &str) -> Result<()> {
    ensure!(
        ROLES.contains(&role),
        "«{role}» er ikke en byrårolle — velg admin eller ansatt"
    );
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Byramedlem {
    pub person_id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub role: String,
    pub active: bool,
}

pub async fn list_medlemmer(pool: &PgPool, firm_id: Uuid) -> Result<Vec<Byramedlem>> {
    let rows = sqlx::query(
        "select p.id as person_id,
                coalesce(p.name, p.oidc_sub) as navn,
                p.email as epost,
                fm.role as rolle,
                fm.active as aktiv
         from firm_member fm
         join person p on p.id = fm.person_id
         where fm.firm_id = $1
         order by 2",
    )
    .bind(firm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Byramedlem {
            person_id: r.get("person_id"),
            name: r.get("navn"),
            email: r.get("epost"),
            role: r.get("rolle"),
            active: r.get("aktiv"),
        })
        .collect())
}

pub(crate) async fn log(
    tx: &mut Transaction<'_, Postgres>,
    firm_id: Uuid,
    person_id: Uuid,
    change: &str,
    from: Option<&str>,
    to: Option<&str>,
    performed_by: Option<Uuid>,
    source: &str,
) -> Result<()> {
    sqlx::query(
        "insert into firm_member_change
             (id, firm_id, person_id, endring, fra_rolle, til_rolle, utfort_av, kilde)
         values ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(firm_id)
    .bind(person_id)
    .bind(change)
    .bind(from)
    .bind(to)
    .bind(performed_by)
    .bind(source)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Locks the firm's member rows, then requires at least one active admin.
/// Same reasoning as the company version: without the lock, two
/// concurrent demotions could each see "there is another admin".
async fn require_remaining_admin(tx: &mut Transaction<'_, Postgres>, firm_id: Uuid) -> Result<()> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from firm_member
         where firm_id = $1 and active and role = 'admin'",
    )
    .bind(firm_id)
    .fetch_one(&mut **tx)
    .await?;
    ensure!(
        n > 0,
        "byrået ville stått uten administrator — gi noen andre admin-rollen først"
    );
    Ok(())
}

async fn laas_medlemmer(tx: &mut Transaction<'_, Postgres>, firm_id: Uuid) -> Result<()> {
    sqlx::query("select 1 from firm_member where firm_id = $1 for update")
        .bind(firm_id)
        .fetch_all(&mut **tx)
        .await?;
    Ok(())
}

pub async fn set_role(
    pool: &PgPool,
    firm_id: Uuid,
    person_id: Uuid,
    new_role: &str,
    performed_by: Uuid,
) -> Result<()> {
    require_role(new_role)?;
    let mut tx = pool.begin().await?;
    laas_medlemmer(&mut tx, firm_id).await?;

    let from: Option<String> = sqlx::query_scalar(
        "select role from firm_member where firm_id = $1 and person_id = $2 and active",
    )
    .bind(firm_id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;
    let from = from.context("personen er ikke medlem av byrået")?;
    if from == new_role {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("update firm_member set role = $3 where firm_id = $1 and person_id = $2")
        .bind(firm_id)
        .bind(person_id)
        .bind(new_role)
        .execute(&mut *tx)
        .await?;
    require_remaining_admin(&mut tx, firm_id).await?;
    log(
        &mut tx,
        firm_id,
        person_id,
        "rolle_endret",
        Some(&from),
        Some(new_role),
        Some(performed_by),
        "admin",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Turns access off or back on. Never deleted — the row is the history
/// of who could reach the firm's clients.
pub async fn set_active(
    pool: &PgPool,
    firm_id: Uuid,
    person_id: Uuid,
    active: bool,
    performed_by: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    laas_medlemmer(&mut tx, firm_id).await?;

    let rad =
        sqlx::query("select role, active from firm_member where firm_id = $1 and person_id = $2")
            .bind(firm_id)
            .bind(person_id)
            .fetch_optional(&mut *tx)
            .await?
            .context("personen er ikke medlem av byrået")?;
    let role: String = rad.get("role");
    if rad.get::<bool, _>("active") == active {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("update firm_member set active = $3 where firm_id = $1 and person_id = $2")
        .bind(firm_id)
        .bind(person_id)
        .bind(active)
        .execute(&mut *tx)
        .await?;
    require_remaining_admin(&mut tx, firm_id).await?;
    log(
        &mut tx,
        firm_id,
        person_id,
        if active { "reaktivert" } else { "deaktivert" },
        Some(&role),
        Some(&role),
        Some(performed_by),
        "admin",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Byrainvitasjon {
    pub id: Uuid,
    pub email: String,
    pub role: String,
    pub invited_by: String,
    pub created_at: DateTime<Utc>,
    pub sist_sendt: Option<DateTime<Utc>>,
}

/// Invites an e-mail address into the firm. Answers identically whether
/// or not the address already has a user with us (migration 0037's
/// reasoning applies unchanged).
pub async fn inviter(
    pool: &PgPool,
    firm_id: Uuid,
    email: &str,
    role: &str,
    invited_by: Uuid,
) -> Result<Uuid> {
    require_role(role)?;
    let email = normalize_email(email);
    ensure!(
        email.contains('@') && !email.starts_with('@') && !email.ends_with('@'),
        "«{email}» ser ikke ut som en e-postadresse"
    );

    let id = Uuid::new_v4();
    let res = sqlx::query(
        "insert into firm_invitation (id, firm_id, epost, role, invited_by)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(firm_id)
    .bind(&email)
    .bind(role)
    .bind(invited_by)
    .execute(pool)
    .await;
    if let Err(e) = res {
        if let Some(db) = e.as_database_error() {
            if db.code().as_deref() == Some("23505") {
                bail!("{email} er allerede invitert til dette byrået");
            }
        }
        return Err(e.into());
    }
    Ok(id)
}

pub async fn list_invitasjoner(pool: &PgPool, firm_id: Uuid) -> Result<Vec<Byrainvitasjon>> {
    let rows = sqlx::query(
        "select i.id, i.epost, i.role, i.created_at,
                coalesce(p.name, p.oidc_sub) as invitert_av,
                (select max(u.created_at) from utsendelse u
                  where u.firm_invitation_id = i.id) as sist_sendt
         from firm_invitation i
         join person p on p.id = i.invited_by
         where i.firm_id = $1 and i.accepted_at is null and i.revoked_at is null
         order by i.created_at desc",
    )
    .bind(firm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Byrainvitasjon {
            id: r.get("id"),
            email: r.get("epost"),
            role: r.get("role"),
            invited_by: r.get("invitert_av"),
            created_at: r.get("created_at"),
            sist_sendt: r.get("sist_sendt"),
        })
        .collect())
}

pub async fn revoke_invitation(
    pool: &PgPool,
    firm_id: Uuid,
    invitation_id: Uuid,
    by: Uuid,
) -> Result<()> {
    let n = sqlx::query(
        "update firm_invitation set revoked_at = now(), revoked_by = $3
         where id = $2 and firm_id = $1
           and accepted_at is null and revoked_at is null",
    )
    .bind(firm_id)
    .bind(invitation_id)
    .bind(by)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n == 1, "invitasjonen finnes ikke, eller er alt brukt");
    Ok(())
}

/// Redeems firm invitations addressed to this person's e-mail address.
/// Called from `/me` next to the company redemption; same semantics: an
/// existing membership is never silently upgraded, the invitation is
/// marked used and the standing membership wins.
pub async fn los_inn_invitasjoner(pool: &PgPool, person_id: Uuid) -> Result<usize> {
    let email: Option<String> = sqlx::query_scalar("select email from person where id = $1")
        .bind(person_id)
        .fetch_one(pool)
        .await?;
    let Some(email) = email.map(|e| normalize_email(&e)).filter(|e| !e.is_empty()) else {
        return Ok(0);
    };

    let mut tx = pool.begin().await?;
    let rader = sqlx::query(
        "select id, firm_id, role from firm_invitation
         where epost = $1 and accepted_at is null and revoked_at is null
         for update",
    )
    .bind(&email)
    .fetch_all(&mut *tx)
    .await?;

    let mut count = 0;
    for rad in &rader {
        let firm_id: Uuid = rad.get("firm_id");
        let role: String = rad.get("role");
        let invitation_id: Uuid = rad.get("id");

        let fantes: Option<String> = sqlx::query_scalar(
            "select role from firm_member where firm_id = $1 and person_id = $2",
        )
        .bind(firm_id)
        .bind(person_id)
        .fetch_optional(&mut *tx)
        .await?;

        if fantes.is_none() {
            sqlx::query("insert into firm_member (firm_id, person_id, role) values ($1,$2,$3)")
                .bind(firm_id)
                .bind(person_id)
                .bind(&role)
                .execute(&mut *tx)
                .await?;
            log(
                &mut tx,
                firm_id,
                person_id,
                "lagt_til",
                None,
                Some(&role),
                None,
                "invitasjon",
            )
            .await?;
        }

        sqlx::query(
            "update firm_invitation set accepted_at = now(), accepted_by = $2 where id = $1",
        )
        .bind(invitation_id)
        .bind(person_id)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}

#[derive(Debug, Clone)]
pub struct Byraendring {
    pub name: String,
    pub change: String,
    pub from_role: Option<String>,
    pub to_role: Option<String>,
    pub performed_by: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

pub async fn tilgangshistorikk(pool: &PgPool, firm_id: Uuid) -> Result<Vec<Byraendring>> {
    let rows = sqlx::query(
        "select coalesce(p.name, p.oidc_sub) as navn,
                c.endring, c.fra_rolle, c.til_rolle, c.kilde, c.created_at,
                coalesce(u.name, u.oidc_sub) as utfort_av
         from firm_member_change c
         join person p on p.id = c.person_id
         left join person u on u.id = c.utfort_av
         where c.firm_id = $1
         order by c.created_at desc
         limit 200",
    )
    .bind(firm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Byraendring {
            name: r.get("navn"),
            change: r.get("endring"),
            from_role: r.get("fra_rolle"),
            to_role: r.get("til_rolle"),
            performed_by: r.get("utfort_av"),
            source: r.get("kilde"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// True when the person is an ACTIVE ADMIN of the firm — the gate for
/// everything in this module, and for deciding engagement requests now
/// that `ansatt` exists as a distinct role.
pub async fn is_firm_admin(pool: &PgPool, person_id: Uuid, firm_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists (select 1 from firm_member
         where firm_id = $1 and person_id = $2 and active and role = 'admin')",
    )
    .bind(firm_id)
    .bind(person_id)
    .fetch_one(pool)
    .await?)
}

/// The invitation mail's content — same rail and same "no secret token"
/// principle as the company variant (0044): the link is the portal front
/// page, redemption happens at login.
pub async fn firm_invitation_email_payload(
    pool: &PgPool,
    firm_id: Uuid,
    invitation_id: Uuid,
    portal_base: Option<&str>,
) -> Result<crate::EmailPayload> {
    let row = sqlx::query(
        "select i.epost, i.role, f.name as firm_name,
                coalesce(p.name, p.oidc_sub) as invited_by
         from firm_invitation i
         join firm f on f.id = i.firm_id
         join person p on p.id = i.invited_by
         where i.id = $1 and i.firm_id = $2
           and i.accepted_at is null and i.revoked_at is null",
    )
    .bind(invitation_id)
    .bind(firm_id)
    .fetch_optional(pool)
    .await?
    .context("ingen åpen invitasjon med den id-en")?;

    let email: String = row.get("epost");
    let role: String = row.get("role");
    let firm_name: String = row.get("firm_name");
    let invited_by: String = row.get("invited_by");

    let mut text = format!(
        "Hei,\n\n{invited_by} har invitert deg inn i byrået {firm_name} i \
         regnmed, med rollen «{role}».\n\n"
    );
    match portal_base {
        Some(base) => text.push_str(&format!(
            "Logg inn her, så er tilgangen på plass:\n{}\n\n",
            base.trim_end_matches('/')
        )),
        None => text.push_str("Logg inn i regnmed, så er tilgangen på plass.\n\n"),
    }
    text.push_str(
        "Bruk denne e-postadressen når du logger inn — tilgangen henger på \
         adressen, ikke på denne meldingen, så det hjelper ingen å \
         videresende den.\n",
    );

    Ok(crate::EmailPayload {
        to: email,
        subject: format!("Du er invitert inn i {firm_name} i regnmed"),
        text,
        reply_to: None,
        attachment: None,
        invoice_id: None,
        reminder_id: None,
        invitation_id: None,
    })
}

/// Logs a firm-invitation mail in the shared `utsendelse` log. The row
/// has no company — the constraint from 0046 demands the firm
/// invitation reference instead.
pub async fn log_firm_dispatch(
    pool: &PgPool,
    id: Uuid,
    firm_invitation_id: Uuid,
    to_email: &str,
    subject: &str,
    sent_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into utsendelse (id, firm_invitation_id, to_email, subject, sent_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(firm_invitation_id)
    .bind(to_email)
    .bind(subject)
    .bind(sent_by)
    .execute(pool)
    .await?;
    Ok(())
}
