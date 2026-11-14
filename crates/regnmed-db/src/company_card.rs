//! Company cards (docs/utlegg.md «Firmakort»): the register, who may use
//! which card, and the card statement lines still waiting for a receipt.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CompanyCard {
    pub id: Uuid,
    pub name: String,
    pub last4: String,
    pub holder_person_id: Option<Uuid>,
    pub holder_name: Option<String>,
    pub account_number: String,
    pub account_name: String,
    pub active: bool,
}

impl CompanyCard {
    /// «Visa bedrift ••1234» — how people tell cards apart.
    pub fn label(&self) -> String {
        format!("{} ••{}", self.name, self.last4)
    }
}

const SELECT: &str = "select c.id, c.name, c.last4, c.holder_person_id, c.active,
                             coalesce(p.name, p.oidc_sub) as holder_name,
                             a.number as account_number, a.name as account_name
                      from company_card c
                      join account a on a.id = c.account_id
                      left join person p on p.id = c.holder_person_id";

fn card(r: &sqlx::postgres::PgRow) -> CompanyCard {
    CompanyCard {
        id: r.get("id"),
        name: r.get("name"),
        last4: r.get("last4"),
        holder_person_id: r.get("holder_person_id"),
        holder_name: r.get("holder_name"),
        account_number: r.get("account_number"),
        account_name: r.get("account_name"),
        active: r.get("active"),
    }
}

/// The company's cards. `usable_by` narrows to the active cards that
/// person may file a purchase for: their own, and the shared ones when
/// `shared_too` (approvers file for shared cards).
pub async fn list_cards(
    pool: &PgPool,
    company_id: Uuid,
    usable_by: Option<(Uuid, bool)>,
) -> Result<Vec<CompanyCard>> {
    let (person, shared_too) = usable_by.unzip();
    let rows = sqlx::query(&format!(
        "{SELECT} where c.company_id = $1
           and ($2::uuid is null
                or (c.active and (c.holder_person_id = $2
                                  or (c.holder_person_id is null and $3))))
         order by c.active desc, c.name, c.last4"
    ))
    .bind(company_id)
    .bind(person)
    .bind(shared_too.unwrap_or(false))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(card).collect())
}

pub async fn get_card(pool: &PgPool, company_id: Uuid, card_id: Uuid) -> Result<CompanyCard> {
    let row = sqlx::query(&format!("{SELECT} where c.company_id = $1 and c.id = $2"))
        .bind(company_id)
        .bind(card_id)
        .fetch_optional(pool)
        .await?
        .context("fant ikke kortet")?;
    Ok(card(&row))
}

fn check_last4(last4: &str) -> Result<()> {
    ensure!(
        last4.len() == 4 && last4.bytes().all(|b| b.is_ascii_digit()),
        "skriv de fire siste sifrene på kortet — aldri hele kortnummeret"
    );
    Ok(())
}

async fn check_holder(pool: &PgPool, company_id: Uuid, holder: Option<Uuid>) -> Result<()> {
    if let Some(h) = holder {
        ensure!(
            crate::is_claimant(pool, company_id, h).await?,
            "kortholderen er ikke medlem av selskapet"
        );
    }
    Ok(())
}

/// Registers a card on its ledger account. The account must exist and
/// carry no other active card: one card per account is what lets a
/// statement line name its card.
pub async fn create_card(
    pool: &PgPool,
    company_id: Uuid,
    name: &str,
    last4: &str,
    holder: Option<Uuid>,
    account_number: &str,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(!name.trim().is_empty(), "gi kortet et navn");
    check_last4(last4)?;
    check_holder(pool, company_id, holder).await?;
    let account_id: Uuid = sqlx::query_scalar(
        "select id from account where company_id = $1 and number = $2 and active",
    )
    .bind(company_id)
    .bind(account_number)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("kontoen {account_number} finnes ikke i kontoplanen"))?;
    let id = Uuid::now_v7();
    let inserted = sqlx::query(
        "insert into company_card (id, company_id, name, last4, holder_person_id, account_id,
                                   created_by)
         values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(name.trim())
    .bind(last4)
    .bind(holder)
    .bind(account_id)
    .bind(created_by)
    .execute(pool)
    .await;
    if let Err(e) = &inserted
        && e.as_database_error().and_then(|d| d.code()).as_deref() == Some("23505")
    {
        anyhow::bail!(
            "konto {account_number} har allerede et aktivt kort — gi hvert kort sin egen konto, \
             så kontoutskriften kan avstemmes per kort"
        );
    }
    inserted?;
    Ok(id)
}

/// Renames, hands over or retires a card. The account never moves:
/// purchases are posted against it.
pub async fn update_card(
    pool: &PgPool,
    company_id: Uuid,
    card_id: Uuid,
    name: &str,
    holder: Option<Uuid>,
    active: bool,
) -> Result<()> {
    ensure!(!name.trim().is_empty(), "gi kortet et navn");
    check_holder(pool, company_id, holder).await?;
    let updated = sqlx::query(
        "update company_card set name = $3, holder_person_id = $4, active = $5
         where id = $1 and company_id = $2",
    )
    .bind(card_id)
    .bind(company_id)
    .bind(name.trim())
    .bind(holder)
    .bind(active)
    .execute(pool)
    .await;
    if let Err(e) = &updated
        && e.as_database_error().and_then(|d| d.code()).as_deref() == Some("23505")
    {
        anyhow::bail!("kontoen har allerede et annet aktivt kort");
    }
    ensure!(updated?.rows_affected() == 1, "fant ikke kortet");
    Ok(())
}

/// A card statement line nobody has documented yet: money out on a card's
/// account, not reconciled against the ledger, and no live claim filed
/// from it. A debit card on the bank account (19xx) shares it with rent,
/// salaries and supplier payments, so there only a line that names the
/// card's last four digits is the card's; a credit card's own account
/// carries nothing else.
#[derive(Debug, Clone)]
pub struct CardLine {
    pub bank_transaction_id: Uuid,
    pub card_id: Uuid,
    pub card_label: String,
    pub holder_person_id: Option<Uuid>,
    pub date: NaiveDate,
    /// Positive: what was spent.
    pub amount_ore: i64,
    pub description: String,
}

/// The lines still waiting for a receipt: on the holder's own cards, or
/// on every card for whoever handles claims (`all`).
pub async fn card_lines(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
    all: bool,
) -> Result<Vec<CardLine>> {
    lines(pool, company_id, viewer, all, None).await
}

/// `except`: a claim whose own link does not count (the draft being
/// rewritten).
async fn lines(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
    all: bool,
    except: Option<Uuid>,
) -> Result<Vec<CardLine>> {
    let rows = sqlx::query(
        "select t.id, c.id as card_id, c.name, c.last4, c.holder_person_id, t.booking_date,
                -t.amount_ore as spent, coalesce(t.description, '') as description
         from bank_transaction t
         join bank_statement s on s.id = t.statement_id
         -- The account's active card, or the last one it had: a retired
         -- card's undocumented purchases still need their receipts.
         join lateral (select * from company_card k where k.account_id = s.account_id
                       order by k.active desc, k.created_at desc limit 1) c on true
         join account a on a.id = s.account_id
         where c.company_id = $1 and t.amount_ore < 0
           and (a.number not like '19%'
                or coalesce(t.description, '') ~ ('(^|[^0-9])' || c.last4 || '([^0-9]|$)'))
           and ($3 or c.holder_person_id = $2)
           and not exists (select 1 from bank_match m where m.bank_transaction_id = t.id)
           and not exists (select 1 from expense x where x.bank_transaction_id = t.id
                                                     and x.status not in ('avvist', 'forkastet')
                                                     and x.id is distinct from $4)
         order by t.booking_date desc, t.id",
    )
    .bind(company_id)
    .bind(viewer)
    .bind(all)
    .bind(except)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| CardLine {
            bank_transaction_id: r.get("id"),
            card_id: r.get("card_id"),
            card_label: format!(
                "{} ••{}",
                r.get::<String, _>("name"),
                r.get::<String, _>("last4")
            ),
            holder_person_id: r.get("holder_person_id"),
            date: r.get("booking_date"),
            amount_ore: r.get("spent"),
            description: r.get("description"),
        })
        .collect())
}

/// One line, if it is still waiting (for anyone but `except`): its card,
/// date, amount and text.
pub async fn card_line(
    pool: &PgPool,
    company_id: Uuid,
    bank_transaction_id: Uuid,
    except: Option<Uuid>,
) -> Result<CardLine> {
    // shortcut: reads every waiting line of the company to find one; a
    // query by id when a company has thousands of undocumented lines.
    lines(pool, company_id, Uuid::nil(), true, except)
        .await?
        .into_iter()
        .find(|l| l.bank_transaction_id == bank_transaction_id)
        .context("kortlinjen finnes ikke, er allerede avstemt eller har allerede en kvittering")
}
