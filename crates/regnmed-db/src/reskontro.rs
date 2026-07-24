//! Reskontro: kunde- og leverandørspesifikasjon and åpne poster.
//!
//! Parties are master data (name editable); the party *number* is part
//! of the v2 hash chain and immutable once referenced. "Open" is always
//! computed — an item is open while the sum matched against it is below
//! its own amount — never stored state.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Auto-numbering start per kind (SAF-T-friendly ranges).
fn first_party_no(kind: &str) -> i64 {
    match kind {
        "kunde" => 10_000,
        _ => 50_000,
    }
}

pub async fn create_party(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    name: &str,
    orgnr: Option<&str>,
    party_no: Option<&str>,
) -> Result<(Uuid, String)> {
    ensure!(
        kind == "kunde" || kind == "leverandor",
        "kind must be 'kunde' or 'leverandor'"
    );
    let party_no = match party_no {
        Some(no) => {
            ensure!(
                !no.is_empty() && no.chars().all(|c| c.is_ascii_digit()),
                "party_no must be digits"
            );
            no.to_string()
        }
        None => {
            let next: i64 = sqlx::query_scalar(
                "select coalesce(max(party_no::bigint) + 1, $2)
                 from party where company_id = $1 and kind = $3",
            )
            .bind(company_id)
            .bind(first_party_no(kind))
            .bind(kind)
            .fetch_one(pool)
            .await?;
            next.to_string()
        }
    };
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into party (id, company_id, party_no, kind, name, orgnr)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&party_no)
    .bind(kind)
    .bind(name)
    .bind(orgnr)
    .execute(pool)
    .await
    .context("creating party (duplicate party_no?)")?;
    Ok((id, party_no))
}

#[derive(Debug)]
pub struct PartyRow {
    pub id: Uuid,
    pub party_no: String,
    pub kind: String,
    pub name: String,
    pub orgnr: Option<String>,
    pub address: Option<String>,
    pub email: Option<String>,
    /// SUM(amount_ore) over the party's entries — the reskontro saldo.
    pub saldo_ore: i64,
}

/// The spesifikasjon: every party of a kind with its saldo.
pub async fn list_parties(
    pool: &PgPool,
    company_id: Uuid,
    kind: Option<&str>,
) -> Result<Vec<PartyRow>> {
    let rows = sqlx::query(
        "select p.id, p.party_no, p.kind, p.name, p.orgnr, p.address, p.email,
                coalesce(sum(e.amount_ore), 0)::bigint as saldo_ore
         from party p
         left join entry e on e.party_id = p.id
         where p.company_id = $1 and ($2::text is null or p.kind = $2)
         group by p.id
         order by p.party_no",
    )
    .bind(company_id)
    .bind(kind)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PartyRow {
            id: r.get("id"),
            party_no: r.get("party_no"),
            kind: r.get("kind"),
            name: r.get("name"),
            orgnr: r.get("orgnr"),
            address: r.get("address"),
            email: r.get("email"),
            saldo_ore: r.get("saldo_ore"),
        })
        .collect())
}

#[derive(Debug)]
pub struct PartyItem {
    pub entry_id: Uuid,
    pub voucher_label: String,
    pub date: NaiveDate,
    pub description: Option<String>,
    pub amount_ore: i64,
    /// Sum already matched against this entry (signed like the entry).
    pub matched_ore: i64,
    /// `amount - matched`; the item is open while this is non-zero.
    pub remaining_ore: i64,
}

/// All items for one party, oldest first, with their open remainder.
pub async fn party_items(
    pool: &PgPool,
    company_id: Uuid,
    party_id: Uuid,
    open_only: bool,
) -> Result<Vec<PartyItem>> {
    let rows = sqlx::query(
        "select e.id, v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description, e.amount_ore,
                coalesce((select sum(m.amount_ore) from reskontro_match m
                          where m.entry_a = e.id), 0)::bigint
              + coalesce((select sum(-m.amount_ore) from reskontro_match m
                          where m.entry_b = e.id), 0)::bigint as matched_ore
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.party_id = $2
         order by v.voucher_date, v.voucher_number, e.line_no",
    )
    .bind(company_id)
    .bind(party_id)
    .fetch_all(pool)
    .await?;

    let items: Vec<PartyItem> = rows
        .iter()
        .map(|r| {
            let amount_ore: i64 = r.get("amount_ore");
            let matched_ore: i64 = r.get("matched_ore");
            PartyItem {
                entry_id: r.get("id"),
                voucher_label: format!(
                    "{}-{}",
                    r.get::<i32, _>("fiscal_year"),
                    r.get::<i64, _>("voucher_number")
                ),
                date: r.get("voucher_date"),
                description: r.get("description"),
                amount_ore,
                matched_ore,
                remaining_ore: amount_ore - matched_ore,
            }
        })
        .filter(|item| !open_only || item.remaining_ore != 0)
        .collect();
    Ok(items)
}

/// Matches an invoice-side entry (`entry_a`, positive remainder) against
/// a settlement-side entry (`entry_b`, negative remainder) for
/// `amount_ore` (> 0, ≤ both remainders). Both entries must belong to
/// the same company, party and account.
pub async fn match_items(
    pool: &PgPool,
    company_id: Uuid,
    entry_a: Uuid,
    entry_b: Uuid,
    amount_ore: i64,
    matched_by: &str,
) -> Result<Uuid> {
    ensure!(amount_ore > 0, "amount must be positive");
    let valid: bool = sqlx::query_scalar(
        "select exists (
             select 1
             from entry a
             join voucher va on va.id = a.voucher_id and va.company_id = $1
             join entry b on b.id = $3
             join voucher vb on vb.id = b.voucher_id and vb.company_id = $1
             where a.id = $2
               and a.party_id is not null and a.party_id = b.party_id
               and a.account_id = b.account_id
         )",
    )
    .bind(company_id)
    .bind(entry_a)
    .bind(entry_b)
    .fetch_one(pool)
    .await?;
    ensure!(
        valid,
        "entries must belong to the same company, party and reskontro account"
    );

    let party_id: Uuid = sqlx::query_scalar("select party_id from entry where id = $1")
        .bind(entry_a)
        .fetch_one(pool)
        .await?;
    let items = party_items(pool, company_id, party_id, false).await?;
    let remaining = |id: Uuid| {
        items
            .iter()
            .find(|i| i.entry_id == id)
            .map(|i| i.remaining_ore)
    };
    let remaining_a = remaining(entry_a).context("entry_a not found")?;
    let remaining_b = remaining(entry_b).context("entry_b not found")?;
    if remaining_a < amount_ore {
        bail!("entry_a has only {remaining_a} øre open, cannot match {amount_ore}");
    }
    if -remaining_b < amount_ore {
        bail!(
            "entry_b has only {} øre open, cannot match {amount_ore}",
            -remaining_b
        );
    }

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(entry_a)
    .bind(entry_b)
    .bind(amount_ore)
    .bind(matched_by)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn unmatch_items(pool: &PgPool, company_id: Uuid, match_id: Uuid) -> Result<()> {
    let removed = sqlx::query(
        "delete from reskontro_match m
         using entry e, voucher v
         where m.id = $2 and e.id = m.entry_a and v.id = e.voucher_id and v.company_id = $1",
    )
    .bind(company_id)
    .bind(match_id)
    .execute(pool)
    .await?;
    ensure!(removed.rows_affected() == 1, "no such match");
    Ok(())
}

/// Flags an account as a reskontro account (or clears the flag).
pub async fn set_account_reskontro(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    kind: Option<&str>,
) -> Result<()> {
    let updated =
        sqlx::query("update account set reskontro_kind = $3 where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(account_number)
            .bind(kind)
            .execute(pool)
            .await?;
    ensure!(updated.rows_affected() == 1, "no account {account_number}");
    Ok(())
}
