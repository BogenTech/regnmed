use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Utc};
use regnmed_core::Ore;
use regnmed_core::hash::{
    EntryHashInput, GENESIS_HASH, VoucherHashInput, chain_hash, truncate_to_micros,
};
use regnmed_core::voucher::VoucherDraft;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct PostedVoucher {
    pub id: Uuid,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub chain_seq: i64,
    pub hash: [u8; 32],
}

#[derive(Debug)]
pub struct ChainReport {
    pub vouchers_checked: i64,
}

/// Creates a company together with the genesis of its hash chain.
pub async fn create_company(pool: &PgPool, orgnr: &str, name: &str) -> Result<Uuid> {
    let mut tx = pool.begin().await?;
    let id = Uuid::now_v7();
    sqlx::query("insert into company (id, orgnr, name) values ($1, $2, $3)")
        .bind(id)
        .bind(orgnr)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("insert into chain_head (company_id, last_seq, last_hash) values ($1, 0, $2)")
        .bind(id)
        .bind(GENESIS_HASH.as_slice())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn find_company_by_orgnr(pool: &PgPool, orgnr: &str) -> Result<Option<Uuid>> {
    let row = sqlx::query("select id from company where orgnr = $1")
        .bind(orgnr)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("id")))
}

pub async fn all_company_ids(pool: &PgPool) -> Result<Vec<Uuid>> {
    let rows = sqlx::query("select id from company order by created_at")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(|r| r.get("id")).collect())
}

pub async fn ensure_journal(
    pool: &PgPool,
    company_id: Uuid,
    code: &str,
    name: &str,
) -> Result<Uuid> {
    sqlx::query(
        "insert into journal (id, company_id, code, name) values ($1, $2, $3, $4)
         on conflict (company_id, code) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(code)
    .bind(name)
    .execute(pool)
    .await?;
    let row = sqlx::query("select id from journal where company_id = $1 and code = $2")
        .bind(company_id)
        .bind(code)
        .fetch_one(pool)
        .await?;
    Ok(row.get("id"))
}

pub async fn ensure_account(
    pool: &PgPool,
    company_id: Uuid,
    number: &str,
    name: &str,
) -> Result<Uuid> {
    sqlx::query(
        "insert into account (id, company_id, number, name) values ($1, $2, $3, $4)
         on conflict (company_id, number) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(number)
    .bind(name)
    .execute(pool)
    .await?;
    let row = sqlx::query("select id from account where company_id = $1 and number = $2")
        .bind(company_id)
        .bind(number)
        .fetch_one(pool)
        .await?;
    Ok(row.get("id"))
}

/// Posts a voucher in a single transaction:
///
/// 1. lock the company's chain head (`FOR UPDATE`), which serializes
///    postings per company — required for both the hash chain and gap-free
///    numbering;
/// 2. take the next voucher number from the counter (rolls back with the
///    transaction, so numbers stay gap-free);
/// 3. hash the voucher content against the previous hash (in Rust, via
///    regnmed-core's canonical serialization);
/// 4. append voucher + entries and advance the chain head.
///
/// The deferred database triggers re-check the double-entry balance at
/// commit, independently of the validation done here.
pub async fn post_voucher(
    pool: &PgPool,
    company_id: Uuid,
    draft: &VoucherDraft,
    created_by: &str,
) -> Result<PostedVoucher> {
    let mut tx = pool.begin().await?;
    let posted = post_voucher_in(&mut tx, company_id, draft, created_by).await?;
    tx.commit().await?;
    Ok(posted)
}

/// Transaction-taking variant, so callers (e.g. invoice issuing) can make
/// the posting atomic with their own writes — gap-free counters on both
/// sides survive a rollback together.
pub async fn post_voucher_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    draft: &VoucherDraft,
    created_by: &str,
) -> Result<PostedVoucher> {
    draft.validate()?;

    // Ajourhold: locked periods reject postings — corrections go in an
    // open period as reversing vouchers. The database trigger re-checks
    // this at insert, independently.
    let lock: Option<chrono::NaiveDate> = sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(&mut **tx)
        .await?;
    if let Some(lock) = lock
        && draft.voucher_date <= lock
    {
        bail!(
            "period is locked through {lock}: voucher dated {} cannot be posted — \
             correct in an open period",
            draft.voucher_date
        );
    }

    let head =
        sqlx::query("select last_seq, last_hash from chain_head where company_id = $1 for update")
            .bind(company_id)
            .fetch_optional(&mut **tx)
            .await?
            .context("company has no chain head — was it created with create_company?")?;
    let last_seq: i64 = head.get("last_seq");
    let prev_hash = to_hash32(head.get("last_hash"))?;

    let journal_id: Uuid =
        sqlx::query("select id from journal where company_id = $1 and code = $2")
            .bind(company_id)
            .bind(&draft.journal_code)
            .fetch_optional(&mut **tx)
            .await?
            .with_context(|| format!("unknown journal '{}' for this company", draft.journal_code))?
            .get("id");

    let fiscal_year = draft.voucher_date.year();
    let voucher_number: i64 = sqlx::query(
        "insert into voucher_counter (journal_id, fiscal_year, last_number) values ($1, $2, 1)
         on conflict (journal_id, fiscal_year)
         do update set last_number = voucher_counter.last_number + 1
         returning last_number",
    )
    .bind(journal_id)
    .bind(fiscal_year)
    .fetch_one(&mut **tx)
    .await?
    .get("last_number");

    // Resolve account numbers and reskontro parties up front so a typo
    // fails the whole voucher with a clear message instead of a
    // foreign-key error. Reskontro rule: accounts flagged with a
    // reskontro kind require a party of that kind; other accounts refuse
    // parties.
    let mut account_ids: Vec<Uuid> = Vec::with_capacity(draft.entries.len());
    let mut party_ids: Vec<Option<Uuid>> = Vec::with_capacity(draft.entries.len());
    for (i, entry) in draft.entries.iter().enumerate() {
        let account = sqlx::query(
            "select id, reskontro_kind from account
             where company_id = $1 and number = $2 and active",
        )
        .bind(company_id)
        .bind(&entry.account_number)
        .fetch_optional(&mut **tx)
        .await?
        .with_context(|| {
            format!(
                "entry line {}: no active account {} for this company",
                i + 1,
                entry.account_number
            )
        })?;
        account_ids.push(account.get("id"));

        let reskontro_kind: Option<String> = account.get("reskontro_kind");
        let party_id = match (&reskontro_kind, &entry.party_no) {
            (Some(kind), Some(party_no)) => {
                let party = sqlx::query(
                    "select id, kind from party where company_id = $1 and party_no = $2",
                )
                .bind(company_id)
                .bind(party_no)
                .fetch_optional(&mut **tx)
                .await?
                .with_context(|| format!("entry line {}: no party {party_no}", i + 1))?;
                let party_kind: String = party.get("kind");
                if &party_kind != kind {
                    bail!(
                        "entry line {}: party {party_no} is a {party_kind}, but account {} is a {kind}-reskontro account",
                        i + 1,
                        entry.account_number
                    );
                }
                Some(party.get("id"))
            }
            (Some(kind), None) => bail!(
                "entry line {}: account {} is a {kind}-reskontro account and requires a party",
                i + 1,
                entry.account_number
            ),
            (None, Some(_)) => bail!(
                "entry line {}: account {} is not a reskontro account — remove the party",
                i + 1,
                entry.account_number
            ),
            (None, None) => None,
        };
        party_ids.push(party_id);
    }

    let chain_seq = last_seq + 1;
    let created_at = truncate_to_micros(Utc::now());
    let voucher_id = Uuid::now_v7();

    let hash_input = VoucherHashInput {
        hash_version: regnmed_core::hash::HASH_VERSION_CURRENT,
        company_id,
        chain_seq,
        journal_code: draft.journal_code.clone(),
        fiscal_year,
        voucher_number,
        voucher_date: draft.voucher_date,
        description: draft.description.clone(),
        reverses: draft.reverses,
        created_by: created_by.to_string(),
        created_at,
        entries: draft
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| EntryHashInput {
                line_no: (i + 1) as i32,
                account_number: e.account_number.clone(),
                amount: e.amount,
                vat_code: none_if_empty(&e.vat_code),
                description: none_if_empty(&e.description),
                party_no: none_if_empty(&e.party_no),
            })
            .collect(),
    };
    let hash = chain_hash(&prev_hash, &hash_input);

    sqlx::query(
        "insert into voucher (id, company_id, journal_id, fiscal_year, voucher_number,
                              voucher_date, description, reverses_voucher_id, created_by,
                              created_at, chain_seq, prev_hash, hash, hash_version)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(voucher_id)
    .bind(company_id)
    .bind(journal_id)
    .bind(fiscal_year)
    .bind(voucher_number)
    .bind(draft.voucher_date)
    .bind(&draft.description)
    .bind(draft.reverses)
    .bind(created_by)
    .bind(created_at)
    .bind(chain_seq)
    .bind(prev_hash.as_slice())
    .bind(hash.as_slice())
    .bind(regnmed_core::hash::HASH_VERSION_CURRENT)
    .execute(&mut **tx)
    .await?;

    for (i, ((entry, account_id), party_id)) in draft
        .entries
        .iter()
        .zip(&account_ids)
        .zip(&party_ids)
        .enumerate()
    {
        sqlx::query(
            "insert into entry (id, voucher_id, line_no, account_id, amount_ore, vat_code,
                                description, party_id)
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(Uuid::now_v7())
        .bind(voucher_id)
        .bind((i + 1) as i32)
        .bind(account_id)
        .bind(entry.amount.0)
        .bind(none_if_empty(&entry.vat_code))
        .bind(none_if_empty(&entry.description))
        .bind(party_id)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query("update chain_head set last_seq = $2, last_hash = $3 where company_id = $1")
        .bind(company_id)
        .bind(chain_seq)
        .bind(hash.as_slice())
        .execute(&mut **tx)
        .await?;

    Ok(PostedVoucher {
        id: voucher_id,
        fiscal_year,
        voucher_number,
        chain_seq,
        hash,
    })
}

/// Re-walks a company's entire voucher chain from the genesis hash,
/// re-hashing every voucher from its stored business content and checking
/// both the link (prev_hash) and the content (hash) at every step, and
/// finally comparing the result against the chain head.
pub async fn verify_chain(pool: &PgPool, company_id: Uuid) -> Result<ChainReport> {
    let vouchers = sqlx::query(
        "select v.id, v.chain_seq, v.fiscal_year, v.voucher_number, v.voucher_date,
                v.description, v.reverses_voucher_id, v.created_by, v.created_at,
                v.prev_hash, v.hash, v.hash_version, j.code as journal_code
         from voucher v
         join journal j on j.id = v.journal_id
         where v.company_id = $1
         order by v.chain_seq",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let mut prev = GENESIS_HASH;
    for row in &vouchers {
        let id: Uuid = row.get("id");
        let chain_seq: i64 = row.get("chain_seq");
        let stored_prev = to_hash32(row.get("prev_hash"))?;
        let stored_hash = to_hash32(row.get("hash"))?;

        if stored_prev != prev {
            bail!(
                "chain broken at seq {chain_seq} (voucher {id}): \
                 stored prev_hash does not match the previous voucher's hash"
            );
        }

        let entry_rows = sqlx::query(
            "select e.line_no, a.number as account_number, e.amount_ore, e.vat_code,
                    e.description, p.party_no
             from entry e
             join account a on a.id = e.account_id
             left join party p on p.id = e.party_id
             where e.voucher_id = $1
             order by e.line_no",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        let input = VoucherHashInput {
            hash_version: row.get("hash_version"),
            company_id,
            chain_seq,
            journal_code: row.get("journal_code"),
            fiscal_year: row.get("fiscal_year"),
            voucher_number: row.get("voucher_number"),
            voucher_date: row.get("voucher_date"),
            description: row.get("description"),
            reverses: row.get("reverses_voucher_id"),
            created_by: row.get("created_by"),
            created_at: row.get::<DateTime<Utc>, _>("created_at"),
            entries: entry_rows
                .iter()
                .map(|e| EntryHashInput {
                    line_no: e.get("line_no"),
                    account_number: e.get("account_number"),
                    amount: Ore(e.get("amount_ore")),
                    vat_code: e.get("vat_code"),
                    description: e.get("description"),
                    party_no: e.get("party_no"),
                })
                .collect(),
        };

        if chain_hash(&prev, &input) != stored_hash {
            bail!(
                "content tampered at seq {chain_seq} (voucher {id}): \
                 recomputed hash does not match the stored hash"
            );
        }
        prev = stored_hash;
    }

    let head = sqlx::query("select last_seq, last_hash from chain_head where company_id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("company has no chain head")?;
    let last_seq: i64 = head.get("last_seq");
    let last_hash = to_hash32(head.get("last_hash"))?;

    if last_seq != vouchers.len() as i64 {
        bail!(
            "chain head is at seq {last_seq} but {} vouchers exist",
            vouchers.len()
        );
    }
    if last_hash != prev {
        bail!("chain head hash does not match the last voucher's hash");
    }

    Ok(ChainReport {
        vouchers_checked: vouchers.len() as i64,
    })
}

fn none_if_empty(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn to_hash32(bytes: Vec<u8>) -> Result<[u8; 32]> {
    bytes.try_into().ok().context("stored hash is not 32 bytes")
}
