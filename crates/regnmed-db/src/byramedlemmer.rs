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

use crate::medlemmer::normaliser_epost;

/// The firm roles (0005): `admin` runs the byrå, `ansatt` works in it.
pub const ROLLER: [&str; 2] = ["admin", "ansatt"];

fn krev_rolle(rolle: &str) -> Result<()> {
    ensure!(
        ROLLER.contains(&rolle),
        "«{rolle}» er ikke en byrårolle — velg admin eller ansatt"
    );
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Byramedlem {
    pub person_id: Uuid,
    pub navn: String,
    pub epost: Option<String>,
    pub rolle: String,
    pub aktiv: bool,
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
            navn: r.get("navn"),
            epost: r.get("epost"),
            rolle: r.get("rolle"),
            aktiv: r.get("aktiv"),
        })
        .collect())
}

pub(crate) async fn logg(
    tx: &mut Transaction<'_, Postgres>,
    firm_id: Uuid,
    person_id: Uuid,
    endring: &str,
    fra: Option<&str>,
    til: Option<&str>,
    utfort_av: Option<Uuid>,
    kilde: &str,
) -> Result<()> {
    sqlx::query(
        "insert into firm_member_change
             (id, firm_id, person_id, endring, fra_rolle, til_rolle, utfort_av, kilde)
         values ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(firm_id)
    .bind(person_id)
    .bind(endring)
    .bind(fra)
    .bind(til)
    .bind(utfort_av)
    .bind(kilde)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Locks the firm's member rows, then requires at least one active admin.
/// Same reasoning as the company version: without the lock, two
/// concurrent demotions could each see "there is another admin".
async fn krev_gjenvaerende_admin(tx: &mut Transaction<'_, Postgres>, firm_id: Uuid) -> Result<()> {
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

pub async fn sett_rolle(
    pool: &PgPool,
    firm_id: Uuid,
    person_id: Uuid,
    ny_rolle: &str,
    utfort_av: Uuid,
) -> Result<()> {
    krev_rolle(ny_rolle)?;
    let mut tx = pool.begin().await?;
    laas_medlemmer(&mut tx, firm_id).await?;

    let fra: Option<String> = sqlx::query_scalar(
        "select role from firm_member where firm_id = $1 and person_id = $2 and active",
    )
    .bind(firm_id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;
    let fra = fra.context("personen er ikke medlem av byrået")?;
    if fra == ny_rolle {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("update firm_member set role = $3 where firm_id = $1 and person_id = $2")
        .bind(firm_id)
        .bind(person_id)
        .bind(ny_rolle)
        .execute(&mut *tx)
        .await?;
    krev_gjenvaerende_admin(&mut tx, firm_id).await?;
    logg(
        &mut tx,
        firm_id,
        person_id,
        "rolle_endret",
        Some(&fra),
        Some(ny_rolle),
        Some(utfort_av),
        "admin",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Turns access off or back on. Never deleted — the row is the history
/// of who could reach the firm's clients.
pub async fn sett_aktiv(
    pool: &PgPool,
    firm_id: Uuid,
    person_id: Uuid,
    aktiv: bool,
    utfort_av: Uuid,
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
    let rolle: String = rad.get("role");
    if rad.get::<bool, _>("active") == aktiv {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("update firm_member set active = $3 where firm_id = $1 and person_id = $2")
        .bind(firm_id)
        .bind(person_id)
        .bind(aktiv)
        .execute(&mut *tx)
        .await?;
    krev_gjenvaerende_admin(&mut tx, firm_id).await?;
    logg(
        &mut tx,
        firm_id,
        person_id,
        if aktiv { "reaktivert" } else { "deaktivert" },
        Some(&rolle),
        Some(&rolle),
        Some(utfort_av),
        "admin",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Byrainvitasjon {
    pub id: Uuid,
    pub epost: String,
    pub rolle: String,
    pub invitert_av: String,
    pub created_at: DateTime<Utc>,
    pub sist_sendt: Option<DateTime<Utc>>,
}

/// Invites an e-mail address into the firm. Answers identically whether
/// or not the address already has a user with us (migration 0037's
/// reasoning applies unchanged).
pub async fn inviter(
    pool: &PgPool,
    firm_id: Uuid,
    epost: &str,
    rolle: &str,
    invited_by: Uuid,
) -> Result<Uuid> {
    krev_rolle(rolle)?;
    let epost = normaliser_epost(epost);
    ensure!(
        epost.contains('@') && !epost.starts_with('@') && !epost.ends_with('@'),
        "«{epost}» ser ikke ut som en e-postadresse"
    );

    let id = Uuid::new_v4();
    let res = sqlx::query(
        "insert into firm_invitation (id, firm_id, epost, role, invited_by)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(firm_id)
    .bind(&epost)
    .bind(rolle)
    .bind(invited_by)
    .execute(pool)
    .await;
    if let Err(e) = res {
        if let Some(db) = e.as_database_error() {
            if db.code().as_deref() == Some("23505") {
                bail!("{epost} er allerede invitert til dette byrået");
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
            epost: r.get("epost"),
            rolle: r.get("role"),
            invitert_av: r.get("invitert_av"),
            created_at: r.get("created_at"),
            sist_sendt: r.get("sist_sendt"),
        })
        .collect())
}

pub async fn tilbakekall_invitasjon(
    pool: &PgPool,
    firm_id: Uuid,
    invitasjon_id: Uuid,
    av: Uuid,
) -> Result<()> {
    let n = sqlx::query(
        "update firm_invitation set revoked_at = now(), revoked_by = $3
         where id = $2 and firm_id = $1
           and accepted_at is null and revoked_at is null",
    )
    .bind(firm_id)
    .bind(invitasjon_id)
    .bind(av)
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
    let epost: Option<String> = sqlx::query_scalar("select email from person where id = $1")
        .bind(person_id)
        .fetch_one(pool)
        .await?;
    let Some(epost) = epost
        .map(|e| normaliser_epost(&e))
        .filter(|e| !e.is_empty())
    else {
        return Ok(0);
    };

    let mut tx = pool.begin().await?;
    let rader = sqlx::query(
        "select id, firm_id, role from firm_invitation
         where epost = $1 and accepted_at is null and revoked_at is null
         for update",
    )
    .bind(&epost)
    .fetch_all(&mut *tx)
    .await?;

    let mut antall = 0;
    for rad in &rader {
        let firm_id: Uuid = rad.get("firm_id");
        let rolle: String = rad.get("role");
        let invitasjon_id: Uuid = rad.get("id");

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
                .bind(&rolle)
                .execute(&mut *tx)
                .await?;
            logg(
                &mut tx,
                firm_id,
                person_id,
                "lagt_til",
                None,
                Some(&rolle),
                None,
                "invitasjon",
            )
            .await?;
        }

        sqlx::query(
            "update firm_invitation set accepted_at = now(), accepted_by = $2 where id = $1",
        )
        .bind(invitasjon_id)
        .bind(person_id)
        .execute(&mut *tx)
        .await?;
        antall += 1;
    }
    tx.commit().await?;
    Ok(antall)
}

#[derive(Debug, Clone)]
pub struct Byraendring {
    pub navn: String,
    pub endring: String,
    pub fra_rolle: Option<String>,
    pub til_rolle: Option<String>,
    pub utfort_av: Option<String>,
    pub kilde: String,
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
            navn: r.get("navn"),
            endring: r.get("endring"),
            fra_rolle: r.get("fra_rolle"),
            til_rolle: r.get("til_rolle"),
            utfort_av: r.get("utfort_av"),
            kilde: r.get("kilde"),
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
    invitasjon_id: Uuid,
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
    .bind(invitasjon_id)
    .bind(firm_id)
    .fetch_optional(pool)
    .await?
    .context("ingen åpen invitasjon med den id-en")?;

    let epost: String = row.get("epost");
    let rolle: String = row.get("role");
    let firm_name: String = row.get("firm_name");
    let invited_by: String = row.get("invited_by");

    let mut text = format!(
        "Hei,\n\n{invited_by} har invitert deg inn i byrået {firm_name} i \
         regnmed, med rollen «{rolle}».\n\n"
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
        to: epost,
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
pub async fn log_firm_utsendelse(
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
