//! Egendefinerte roller (#60, docs/auth.md).
//!
//! De innebygde rollene bor i koden (`regnmed-api::tilgang`). Dette
//! modulet holder bare det et selskap har satt sammen selv, av
//! rettighetene som allerede finnes.
//!
//! Rettighetsnavnene lagres som tekst, og **databasen kjenner dem
//! ikke**. Oppslaget filtrerer bort det koden ikke gjenkjenner, så en
//! rolle kan ikke love en rettighet ingen håndhever — og en tilbakerullet
//! versjon som ikke kjenner en ny rettighet ser den bare forsvinne.
//!
//! **Hver endring er ÉN transaksjon** (#62), av to grunner som begge
//! rammer noen andre enn den som skriver:
//!
//! 1. Rollen, rettighetene og loggraden hører sammen. Sto de i hvert
//!    sitt statement, kunne en rolle bli til uten rettigheter og uten
//!    en rad i `company_role_change` — og en rolle loggen ikke
//!    forklarer er nøyaktig det endringsloggen finnes for å umuliggjøre.
//! 2. Å sette rettigheter er `delete` + `insert`. Utenfor en
//!    transaksjon kan `rettigheter_for` (tilgangsvakten, i en helt
//!    annen forespørsel) lese MELLOM dem og se en tom liste: den som
//!    har rollen mister tilgangen et øyeblikk, tilfeldig, uten at noe
//!    er galt. Nå ser oppslaget alltid enten den gamle eller den nye
//!    listen.
//!
//! Rollen låses (`for update`) før rettighetene skrives om. Uten låsen
//! ville to samtidige endringer begge slettet den gamle listen og
//! sluppet igjennom hver sin — resultatet ble UNIONEN, altså mer
//! tilgang enn noen av dem ba om.

use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// Navn som tilhører de innebygde rollene og ikke kan gjenbrukes.
pub const RESERVERTE_NAVN: [&str; 6] = ["admin", "bokforing", "les", "ansatt", "revisor", "ukjent"];

/// SQLSTATE for unique_violation.
const UNIK_KRENKELSE: &str = "23505";

#[derive(Debug, Clone)]
pub struct Rolle {
    pub id: Uuid,
    pub navn: String,
    pub aktiv: bool,
    pub rettigheter: Vec<String>,
    pub i_bruk: i64,
}

/// Rettighetene knyttet til navngitte roller i ett selskap.
///
/// Kalles fra tilgangsvakten for de rollenavnene som ikke er innebygde.
/// En deaktivert rolle gir ingenting — det er slik en rolle «fjernes»
/// uten at historikken om hvem som hadde den forsvinner.
pub async fn rettigheter_for(
    pool: &PgPool,
    company_id: Uuid,
    navn: &[String],
) -> Result<Vec<String>> {
    if navn.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "select rr.rett
         from company_role r
         join company_role_right rr on rr.role_id = r.id
         where r.company_id = $1 and r.aktiv and r.navn = any($2)",
    )
    .bind(company_id)
    .bind(navn)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| r.get("rett")).collect())
}

pub async fn list_roller(pool: &PgPool, company_id: Uuid) -> Result<Vec<Rolle>> {
    let rows = sqlx::query(
        "select r.id, r.navn, r.aktiv,
                coalesce(array_agg(rr.rett) filter (where rr.rett is not null), '{}') as rettigheter,
                (select count(*) from company_member m
                  where m.company_id = r.company_id and m.role = r.navn and m.active) as i_bruk
         from company_role r
         left join company_role_right rr on rr.role_id = r.id
         where r.company_id = $1
         group by r.id
         order by r.navn",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Rolle {
            id: r.get("id"),
            navn: r.get("navn"),
            aktiv: r.get("aktiv"),
            rettigheter: r.get("rettigheter"),
            i_bruk: r.get("i_bruk"),
        })
        .collect())
}

async fn logg(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    role_id: Uuid,
    endring: &str,
    rettigheter: Option<&str>,
    utfort_av: Uuid,
) -> Result<()> {
    sqlx::query(
        "insert into company_role_change
             (id, company_id, role_id, endring, rettigheter, utfort_av)
         values ($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(role_id)
    .bind(endring)
    .bind(rettigheter)
    .bind(utfort_av)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Oppretter en rolle. `godkjente` er rettighetene kalleren har
/// kontrollert at kan delegeres — modulet her kjenner ikke vokabularet.
pub async fn opprett(
    pool: &PgPool,
    company_id: Uuid,
    navn: &str,
    godkjente: &[String],
    av: Uuid,
    av_navn: &str,
) -> Result<Uuid> {
    let navn = navn.trim();
    ensure!(!navn.is_empty(), "rollen må ha et navn");
    ensure!(
        !RESERVERTE_NAVN.contains(&navn.to_lowercase().as_str()),
        "«{navn}» er navnet på en innebygd rolle"
    );

    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    let res = sqlx::query(
        "insert into company_role (id, company_id, navn, created_by) values ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(company_id)
    .bind(navn)
    .bind(av_navn)
    .execute(&mut *tx)
    .await;
    if let Err(e) = res {
        // Feilkoden, ikke constraint-navnet: en rename av constrainten
        // skal ikke gjøre «navnet er opptatt» om til en 500.
        if e.as_database_error().and_then(|d| d.code()).as_deref() == Some(UNIK_KRENKELSE) {
            bail!("selskapet har allerede en rolle som heter «{navn}»");
        }
        return Err(e.into());
    }
    sett_rettigheter_in(&mut tx, id, godkjente).await?;
    logg(
        &mut tx,
        company_id,
        id,
        "opprettet",
        Some(&godkjente.join(" ")),
        av,
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Skriver rettighetslisten om. Kjøres alltid inne i transaksjonen som
/// også skriver loggraden, og etter at rollen er låst.
async fn sett_rettigheter_in(
    tx: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    rettigheter: &[String],
) -> Result<()> {
    sqlx::query("delete from company_role_right where role_id = $1")
        .bind(role_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "insert into company_role_right (role_id, rett)
         select $1, rett from unnest($2::text[]) as rett
         on conflict do nothing",
    )
    .bind(role_id)
    .bind(rettigheter)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn sett_rettigheter(
    pool: &PgPool,
    company_id: Uuid,
    role_id: Uuid,
    godkjente: &[String],
    av: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    // Låser rollen og sjekker at den finnes i ett — to samtidige
    // endringer skal skje etter hverandre, ikke blande listene sine.
    let finnes: Option<Uuid> = sqlx::query_scalar(
        "select id from company_role where id = $1 and company_id = $2 for update",
    )
    .bind(role_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?;
    ensure!(finnes.is_some(), "ukjent rolle");
    sett_rettigheter_in(&mut tx, role_id, godkjente).await?;
    logg(
        &mut tx,
        company_id,
        role_id,
        "rettigheter_endret",
        Some(&godkjente.join(" ")),
        av,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Deaktiverer eller gjenoppliver en rolle. Roller slettes aldri: de er
/// forklaringen på hvilken tilgang noen HADDE.
pub async fn sett_aktiv(
    pool: &PgPool,
    company_id: Uuid,
    role_id: Uuid,
    aktiv: bool,
    av: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query("update company_role set aktiv = $3 where id = $2 and company_id = $1")
        .bind(company_id)
        .bind(role_id)
        .bind(aktiv)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    ensure!(n == 1, "ukjent rolle");
    logg(
        &mut tx,
        company_id,
        role_id,
        if aktiv { "reaktivert" } else { "deaktivert" },
        None,
        av,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Finnes rollenavnet som en aktiv egendefinert rolle her?
pub async fn finnes(pool: &PgPool, company_id: Uuid, navn: &str) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from company_role where company_id = $1 and navn = $2 and aktiv",
    )
    .bind(company_id)
    .bind(navn)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

#[derive(Debug, Clone)]
pub struct Rolleendring {
    pub navn: String,
    pub endring: String,
    pub rettigheter: Option<String>,
    pub utfort_av: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn historikk(pool: &PgPool, company_id: Uuid) -> Result<Vec<Rolleendring>> {
    let rows = sqlx::query(
        "select r.navn, c.endring, c.rettigheter, c.created_at,
                coalesce(p.name, p.oidc_sub) as utfort_av
         from company_role_change c
         join company_role r on r.id = c.role_id
         left join person p on p.id = c.utfort_av
         where c.company_id = $1
         order by c.created_at desc
         limit 200",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Rolleendring {
            navn: r.get("navn"),
            endring: r.get("endring"),
            rettigheter: r.get("rettigheter"),
            utfort_av: r.get("utfort_av"),
            created_at: r.get("created_at"),
        })
        .collect())
}
