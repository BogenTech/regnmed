//! Medlemsadministrasjon (#53, docs/auth.md).
//!
//! Hvem som har tilgang til et selskap, hvordan de fikk den, og hvordan
//! den tas bort igjen. Tilgang gitt gjennom et **oppdrag** styres ikke
//! herfra — den følger engasjementet (docs/marketplace.md), og et forsøk
//! på å endre den skal si fra i stedet for å se ut som om det virket.

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// Rollene som kan tildeles. `ansatt` er selvbetjening (#54) og ikke et
/// trinn under `les` — den får skrive noen få egne ting og lese nesten
/// ingenting.
pub const ROLLER: [&str; 4] = ["admin", "bokforing", "les", "ansatt"];

fn krev_tildelbar(rolle: &str) -> Result<()> {
    if rolle == "revisor" {
        // Ikke en skrivefeil å avvise stille: 'revisor' er en ekte rolle
        // i systemet, den kommer bare et annet sted fra.
        bail!(
            "«revisor» tildeles ikke her — den følger av et revisjonsoppdrag \
             (docs/marketplace.md)"
        );
    }
    // Egendefinerte roller (#60) slipper gjennom her; at navnet faktisk
    // finnes i selskapet kontrolleres av API-laget, som kjenner
    // selskapet. Et navn som ikke finnes gir uansett ingen rettigheter.
    Ok(())
}

/// E-post normalisert som vi sammenligner den: trimmet, små bokstaver.
///
/// Adressen er nøkkelen en invitasjon løses inn på, så «Ola@Firma.no»
/// og «ola@firma.no» må være samme invitasjon. Normaliseringen skjer
/// her, ett sted, og det som lagres er den normaliserte formen.
pub fn normaliser_epost(raw: &str) -> String {
    raw.trim().to_lowercase()
}

#[derive(Debug, Clone)]
pub struct Medlem {
    pub person_id: Uuid,
    pub navn: String,
    pub epost: Option<String>,
    pub rolle: String,
    pub aktiv: bool,
    /// «direkte» eller navnet på byrået oppdraget går gjennom.
    pub via: String,
    /// Tilgang gjennom oppdrag kan ikke endres her.
    pub kan_endres: bool,
}

/// Alle som har tilgang, uansett vei inn.
///
/// Direkte medlemskap og oppdragstilgang vises sammen, fordi det er slik
/// spørsmålet stilles («hvem kommer til her?»), men de er merket
/// forskjellig: den som kom inn via et oppdrag kan bare fjernes ved å
/// avslutte oppdraget.
pub async fn list_medlemmer(pool: &PgPool, company_id: Uuid) -> Result<Vec<Medlem>> {
    let rows = sqlx::query(
        "select p.id as person_id,
                coalesce(p.name, p.oidc_sub) as navn,
                p.email as epost,
                cm.role as rolle,
                cm.active as aktiv,
                'direkte' as via,
                true as kan_endres
         from company_member cm
         join person p on p.id = cm.person_id
         where cm.company_id = $1

         union all

         select p.id, coalesce(p.name, p.oidc_sub), p.email,
                case e.kind when 'regnskap' then 'bokforing' else 'revisor' end,
                true,
                f.name,
                false
         from firm_member fm
         join firm f on f.id = fm.firm_id
         join engagement e on e.firm_id = fm.firm_id
         join person p on p.id = fm.person_id
         where e.company_id = $1
           and fm.active
           and e.valid_from <= current_date
           and (e.valid_to is null or e.valid_to > current_date)

         order by 2",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Medlem {
            person_id: r.get("person_id"),
            navn: r.get("navn"),
            epost: r.get("epost"),
            rolle: r.get("rolle"),
            aktiv: r.get("aktiv"),
            via: r.get("via"),
            kan_endres: r.get("kan_endres"),
        })
        .collect())
}

async fn logg(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    person_id: Uuid,
    endring: &str,
    fra: Option<&str>,
    til: Option<&str>,
    utfort_av: Option<Uuid>,
    kilde: &str,
) -> Result<()> {
    sqlx::query(
        "insert into company_member_change
             (id, company_id, person_id, endring, fra_rolle, til_rolle, utfort_av, kilde)
         values ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
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

/// Krever at selskapet fortsatt har minst én aktiv admin.
///
/// Kjøres INNE i transaksjonen, etter endringen, og låser selskapets
/// medlemsrader først. Uten låsen kunne to samtidige degraderinger
/// begge se «det finnes en annen admin» og etterlate selskapet uten —
/// og et selskap uten admin er ikke gjenopprettelig uten DB-tilgang.
async fn krev_gjenvaerende_admin(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
) -> Result<()> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from company_member
         where company_id = $1 and active and role = 'admin'",
    )
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await?;
    ensure!(
        n > 0,
        "selskapet ville stått uten administrator — gi noen andre admin-rollen først"
    );
    Ok(())
}

/// Låser selskapets medlemsrader for resten av transaksjonen.
async fn laas_medlemmer(tx: &mut Transaction<'_, Postgres>, company_id: Uuid) -> Result<()> {
    sqlx::query("select 1 from company_member where company_id = $1 for update")
        .bind(company_id)
        .fetch_all(&mut **tx)
        .await?;
    Ok(())
}

/// Endrer rollen til et direkte medlem.
pub async fn sett_rolle(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    ny_rolle: &str,
    utfort_av: Uuid,
) -> Result<()> {
    krev_tildelbar(ny_rolle)?;
    let mut tx = pool.begin().await?;
    laas_medlemmer(&mut tx, company_id).await?;

    let fra: Option<String> = sqlx::query_scalar(
        "select role from company_member where company_id = $1 and person_id = $2 and active",
    )
    .bind(company_id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;
    let fra = fra.context(
        "personen er ikke et direkte medlem her — tilgang gjennom et oppdrag \
         endres ved å endre oppdraget",
    )?;
    if fra == ny_rolle {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("update company_member set role = $3 where company_id = $1 and person_id = $2")
        .bind(company_id)
        .bind(person_id)
        .bind(ny_rolle)
        .execute(&mut *tx)
        .await?;
    krev_gjenvaerende_admin(&mut tx, company_id).await?;
    logg(
        &mut tx,
        company_id,
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

/// Slår tilgangen av eller på igjen. Medlemskapet slettes aldri — det er
/// historikken over hvem som hadde tilgang.
pub async fn sett_aktiv(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    aktiv: bool,
    utfort_av: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    laas_medlemmer(&mut tx, company_id).await?;

    let rad = sqlx::query(
        "select role, active from company_member where company_id = $1 and person_id = $2",
    )
    .bind(company_id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?
    .context(
        "personen er ikke et direkte medlem her — tilgang gjennom et oppdrag \
         fjernes ved å avslutte oppdraget",
    )?;
    let rolle: String = rad.get("role");
    if rad.get::<bool, _>("active") == aktiv {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("update company_member set active = $3 where company_id = $1 and person_id = $2")
        .bind(company_id)
        .bind(person_id)
        .bind(aktiv)
        .execute(&mut *tx)
        .await?;
    krev_gjenvaerende_admin(&mut tx, company_id).await?;
    logg(
        &mut tx,
        company_id,
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
pub struct Invitasjon {
    pub id: Uuid,
    pub epost: String,
    pub rolle: String,
    pub invitert_av: String,
    pub created_at: DateTime<Utc>,
}

/// Inviterer en e-postadresse inn i selskapet.
///
/// Svarer likt uansett om adressen alt har en bruker hos oss — se
/// migrasjon 0037 for hvorfor. Har den det, blir invitasjonen løst inn
/// neste gang vedkommende laster portalen.
pub async fn inviter(
    pool: &PgPool,
    company_id: Uuid,
    epost: &str,
    rolle: &str,
    invited_by: Uuid,
) -> Result<Uuid> {
    krev_tildelbar(rolle)?;
    let epost = normaliser_epost(epost);
    ensure!(
        epost.contains('@') && !epost.starts_with('@') && !epost.ends_with('@'),
        "«{epost}» ser ikke ut som en e-postadresse"
    );

    let id = Uuid::new_v4();
    let res = sqlx::query(
        "insert into company_invitation (id, company_id, epost, role, invited_by)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&epost)
    .bind(rolle)
    .bind(invited_by)
    .execute(pool)
    .await;
    if let Err(e) = res {
        if e.to_string().contains("company_invitation_open_uq") {
            bail!("{epost} er allerede invitert til dette selskapet");
        }
        return Err(e.into());
    }
    Ok(id)
}

pub async fn list_invitasjoner(pool: &PgPool, company_id: Uuid) -> Result<Vec<Invitasjon>> {
    let rows = sqlx::query(
        "select i.id, i.epost, i.role, i.created_at,
                coalesce(p.name, p.oidc_sub) as invitert_av
         from company_invitation i
         join person p on p.id = i.invited_by
         where i.company_id = $1 and i.accepted_at is null and i.revoked_at is null
         order by i.created_at desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Invitasjon {
            id: r.get("id"),
            epost: r.get("epost"),
            rolle: r.get("role"),
            invitert_av: r.get("invitert_av"),
            created_at: r.get("created_at"),
        })
        .collect())
}

pub async fn tilbakekall_invitasjon(
    pool: &PgPool,
    company_id: Uuid,
    invitasjon_id: Uuid,
    av: Uuid,
) -> Result<()> {
    let n = sqlx::query(
        "update company_invitation set revoked_at = now(), revoked_by = $3
         where id = $2 and company_id = $1
           and accepted_at is null and revoked_at is null",
    )
    .bind(company_id)
    .bind(invitasjon_id)
    .bind(av)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n == 1, "invitasjonen finnes ikke, eller er alt brukt");
    Ok(())
}

/// Løser inn invitasjoner som står til personens e-postadresse.
///
/// Kalles fra `/me`, altså når portalen starter en økt. Det er samme
/// mønster som oppdrag: tilgangen blir synlig uten ny innlogging, men
/// den materialiseres når vi faktisk vet hvem som spør.
///
/// Har personen allerede et medlemskap, oppgraderes rollen aldri i det
/// stille — invitasjonen merkes brukt, og den sterkeste av de to
/// rollene beholdes ikke automatisk. Vi lar det bestående medlemskapet
/// stå: en admin som mener noe annet kan endre rollen uttrykkelig, og
/// det er tryggere enn at en invitasjon kan heve tilgang uten at noen
/// ser det.
///
/// Returnerer antall invitasjoner som ble løst inn.
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
        "select id, company_id, role from company_invitation
         where epost = $1 and accepted_at is null and revoked_at is null
         for update",
    )
    .bind(&epost)
    .fetch_all(&mut *tx)
    .await?;

    let mut antall = 0;
    for rad in &rader {
        let company_id: Uuid = rad.get("company_id");
        let rolle: String = rad.get("role");
        let invitasjon_id: Uuid = rad.get("id");

        let fantes: Option<String> = sqlx::query_scalar(
            "select role from company_member where company_id = $1 and person_id = $2",
        )
        .bind(company_id)
        .bind(person_id)
        .fetch_optional(&mut *tx)
        .await?;

        if fantes.is_none() {
            sqlx::query(
                "insert into company_member (company_id, person_id, role) values ($1,$2,$3)",
            )
            .bind(company_id)
            .bind(person_id)
            .bind(&rolle)
            .execute(&mut *tx)
            .await?;
            logg(
                &mut tx,
                company_id,
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
            "update company_invitation set accepted_at = now(), accepted_by = $2 where id = $1",
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
pub struct Tilgangsendring {
    pub navn: String,
    pub endring: String,
    pub fra_rolle: Option<String>,
    pub til_rolle: Option<String>,
    pub utfort_av: Option<String>,
    pub kilde: String,
    pub created_at: DateTime<Utc>,
}

/// Sporet over hvem som ga hvem tilgang — spørsmålet en revisor stiller.
pub async fn tilgangshistorikk(pool: &PgPool, company_id: Uuid) -> Result<Vec<Tilgangsendring>> {
    let rows = sqlx::query(
        "select coalesce(p.name, p.oidc_sub) as navn,
                c.endring, c.fra_rolle, c.til_rolle, c.kilde, c.created_at,
                coalesce(u.name, u.oidc_sub) as utfort_av
         from company_member_change c
         join person p on p.id = c.person_id
         left join person u on u.id = c.utfort_av
         where c.company_id = $1
         order by c.created_at desc
         limit 200",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Tilgangsendring {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eposten_normaliseres_for_sammenligning() {
        assert_eq!(normaliser_epost("  Ola@Firma.NO "), "ola@firma.no");
        assert_eq!(normaliser_epost("ola@firma.no"), "ola@firma.no");
    }
}
