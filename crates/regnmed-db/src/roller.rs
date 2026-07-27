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

use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Navn som tilhører de innebygde rollene og ikke kan gjenbrukes.
pub const RESERVERTE_NAVN: [&str; 6] = ["admin", "bokforing", "les", "ansatt", "revisor", "ukjent"];

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
    pool: &PgPool,
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
    .execute(pool)
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
    let res = sqlx::query(
        "insert into company_role (id, company_id, navn, created_by) values ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(company_id)
    .bind(navn)
    .bind(av_navn)
    .execute(pool)
    .await;
    if let Err(e) = res {
        if e.to_string().contains("company_role_company_id_navn_key") {
            bail!("selskapet har allerede en rolle som heter «{navn}»");
        }
        return Err(e.into());
    }
    sett_rettigheter_in(pool, id, godkjente).await?;
    logg(
        pool,
        company_id,
        id,
        "opprettet",
        Some(&godkjente.join(" ")),
        av,
    )
    .await?;
    Ok(id)
}

async fn sett_rettigheter_in(pool: &PgPool, role_id: Uuid, rettigheter: &[String]) -> Result<()> {
    sqlx::query("delete from company_role_right where role_id = $1")
        .bind(role_id)
        .execute(pool)
        .await?;
    for rett in rettigheter {
        sqlx::query(
            "insert into company_role_right (role_id, rett) values ($1,$2)
             on conflict do nothing",
        )
        .bind(role_id)
        .bind(rett)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn sett_rettigheter(
    pool: &PgPool,
    company_id: Uuid,
    role_id: Uuid,
    godkjente: &[String],
    av: Uuid,
) -> Result<()> {
    let finnes: Option<Uuid> =
        sqlx::query_scalar("select id from company_role where id = $1 and company_id = $2")
            .bind(role_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    ensure!(finnes.is_some(), "ukjent rolle");
    sett_rettigheter_in(pool, role_id, godkjente).await?;
    logg(
        pool,
        company_id,
        role_id,
        "rettigheter_endret",
        Some(&godkjente.join(" ")),
        av,
    )
    .await?;
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
    let n = sqlx::query("update company_role set aktiv = $3 where id = $2 and company_id = $1")
        .bind(company_id)
        .bind(role_id)
        .bind(aktiv)
        .execute(pool)
        .await?
        .rows_affected();
    ensure!(n == 1, "ukjent rolle");
    logg(
        pool,
        company_id,
        role_id,
        if aktiv { "reaktivert" } else { "deaktivert" },
        None,
        av,
    )
    .await?;
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
