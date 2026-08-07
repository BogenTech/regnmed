//! Lovpålagte spesifikasjoner (bokføringsforskriften §3-1): saldobalanse,
//! kontospesifikasjon and bokføringsspesifikasjon, plus the saldo lines
//! that feed resultat/balanse in `regnmed-core::regnskap`.
//!
//! All of it is `SUM(amount_ore)` and ordered SELECTs over the immutable
//! ledger — never stored state, so the reports are correct the moment a
//! voucher is posted and reproducible for any historical period.

use anyhow::Result;
use chrono::NaiveDate;
use regnmed_core::regnskap::SaldoLine;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One account over a period: inngående saldo, movement, utgående saldo.
#[derive(Debug)]
pub struct SaldobalanseRow {
    pub number: String,
    pub name: String,
    pub inngaende_ore: i64,
    pub debet_ore: i64,
    pub kredit_ore: i64,
    pub utgaende_ore: i64,
}

pub async fn saldobalanse(
    pool: &PgPool,
    company_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<SaldobalanseRow>> {
    let rows = sqlx::query(
        "select a.number, a.name,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0)::bigint as inngaende,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date between $2 and $3
                                                     and e.amount_ore > 0), 0)::bigint as debet,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date between $2 and $3
                                                     and e.amount_ore < 0), 0)::bigint as kredit,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0)::bigint as utgaende
         from account a
         join entry e on e.account_id = a.id
         join voucher v on v.id = e.voucher_id
         where a.company_id = $1 and v.voucher_date <= $3
         group by a.number, a.name
         having coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0) <> 0
             or coalesce(sum(e.amount_ore) filter (where v.voucher_date between $2 and $3), 0) <> 0
             or coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0) <> 0
         order by a.number",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SaldobalanseRow {
            number: r.get("number"),
            name: r.get("name"),
            inngaende_ore: r.get("inngaende"),
            debet_ore: r.get("debet"),
            kredit_ore: r.get("kredit"),
            utgaende_ore: r.get("utgaende"),
        })
        .collect())
}

/// Saldo per account from day one through `to` (ledger sign) — the input
/// to resultat/balanse. For resultat, pass the period's `from` too.
/// Optional dimension filters restrict to entries carrying that
/// avdeling/prosjekt code — resultat per dimensjon, same pure SUM.
pub async fn saldo_lines(
    pool: &PgPool,
    company_id: Uuid,
    from: Option<NaiveDate>,
    to: NaiveDate,
    avdeling: Option<&str>,
    prosjekt: Option<&str>,
) -> Result<Vec<SaldoLine>> {
    let rows = sqlx::query(
        "select a.number, a.name, sum(e.amount_ore)::bigint as saldo
         from account a
         join entry e on e.account_id = a.id
         join voucher v on v.id = e.voucher_id
         left join dimension da on da.id = e.avdeling_id
         left join dimension dp on dp.id = e.prosjekt_id
         where a.company_id = $1 and v.voucher_date <= $3
           and ($2::date is null or v.voucher_date >= $2)
           and ($4::text is null or da.code = $4)
           and ($5::text is null or dp.code = $5)
         group by a.number, a.name
         order by a.number",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .bind(avdeling)
    .bind(prosjekt)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SaldoLine {
            number: r.get("number"),
            name: r.get("name"),
            saldo_ore: r.get("saldo"),
        })
        .collect())
}

/// Per-prosjekt account sums over a period (#71) — the overview's data
/// in ONE query, folded per project with `regnskap::lonnsomhet` by the
/// caller. Same SUM style as `saldo_lines`, grouped by the project
/// code; entries without a prosjekt are not project economics and are
/// deliberately absent.
pub async fn prosjekt_saldo_lines(
    pool: &PgPool,
    company_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<(String, SaldoLine)>> {
    let rows = sqlx::query(
        "select dp.code as prosjekt, a.number, a.name, sum(e.amount_ore)::bigint as saldo
         from account a
         join entry e on e.account_id = a.id
         join voucher v on v.id = e.voucher_id
         join dimension dp on dp.id = e.prosjekt_id
         where a.company_id = $1 and v.voucher_date between $2 and $3
         group by dp.code, a.number, a.name
         order by dp.code, a.number",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get("prosjekt"),
                SaldoLine {
                    number: r.get("number"),
                    name: r.get("name"),
                    saldo_ore: r.get("saldo"),
                },
            )
        })
        .collect())
}

/// One posting on one account, with the dokumentasjonshenvisning
/// (journal + bilagsnummer) the forskrift requires.
#[derive(Debug)]
pub struct KontoPost {
    pub number: String,
    pub account_name: String,
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: NaiveDate,
    pub description: String,
    pub amount_ore: i64,
    /// Running saldo on the account, including this posting, from the
    /// period's inngående saldo.
    pub saldo_ore: i64,
    pub party_no: Option<String>,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

/// Kontospesifikasjon: every posting per account in date/bilag order,
/// with running saldo seeded from the inngående balance. `account`
/// filters to one account when given.
pub async fn kontospesifikasjon(
    pool: &PgPool,
    company_id: Uuid,
    account: Option<&str>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<KontoPost>> {
    let rows = sqlx::query(
        "select a.number, a.name as account_name, j.code as journal_code,
                v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description,
                e.amount_ore, p.party_no,
                da.code as avdeling, dp.code as prosjekt,
                ib.saldo as inngaende,
                sum(e.amount_ore) over (partition by a.number
                    order by v.voucher_date, v.chain_seq, e.line_no)::bigint as bevegelse
         from entry e
         join voucher v on v.id = e.voucher_id
         join journal j on j.id = v.journal_id
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join dimension da on da.id = e.avdeling_id
         left join dimension dp on dp.id = e.prosjekt_id
         left join lateral (
             select coalesce(sum(e2.amount_ore), 0)::bigint as saldo
             from entry e2 join voucher v2 on v2.id = e2.voucher_id
             where e2.account_id = a.id and v2.voucher_date < $3
         ) ib on true
         where v.company_id = $1
           and ($2::text is null or a.number = $2)
           and v.voucher_date between $3 and $4
         order by a.number, v.voucher_date, v.chain_seq, e.line_no",
    )
    .bind(company_id)
    .bind(account)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| KontoPost {
            number: r.get("number"),
            account_name: r.get("account_name"),
            journal_code: r.get("journal_code"),
            fiscal_year: r.get("fiscal_year"),
            voucher_number: r.get("voucher_number"),
            voucher_date: r.get("voucher_date"),
            description: r.get("description"),
            amount_ore: r.get("amount_ore"),
            saldo_ore: r.get::<i64, _>("inngaende") + r.get::<i64, _>("bevegelse"),
            party_no: r.get("party_no"),
            avdeling: r.get("avdeling"),
            prosjekt: r.get("prosjekt"),
        })
        .collect())
}

/// One posting on one party, with the dokumentasjonshenvisning the
/// forskrift requires and the running saldo per party.
#[derive(Debug)]
pub struct ReskontroPost {
    pub account_number: String,
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: NaiveDate,
    pub description: String,
    pub amount_ore: i64,
    /// Running saldo for the party, including this posting, from the
    /// period's inngående saldo.
    pub saldo_ore: i64,
}

/// One party's block of the spesifikasjon: inngående saldo, every
/// posting in the period, utgående saldo.
#[derive(Debug)]
pub struct ReskontroParty {
    pub party_no: String,
    pub party_name: String,
    pub inngaende_ore: i64,
    pub utgaende_ore: i64,
    pub posts: Vec<ReskontroPost>,
}

/// Kunde-/leverandørspesifikasjon (bokføringsforskriften §3-1 nr. 3–4):
/// every party-bound posting per party in date/bilag order, with running
/// saldo seeded from the party's inngående saldo. `kind` is `kunde` or
/// `leverandor`. A party with no movement in the period is still listed
/// when its inngående saldo is nonzero — the saldo exists whether or not
/// the period touched it.
pub async fn reskontrospesifikasjon(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<ReskontroParty>> {
    // Saldi first: the party list is everyone with a nonzero inngående
    // saldo or any movement in the period, so the blocks below can be
    // seeded even when a party has no lines.
    let saldi = sqlx::query(
        "select p.party_no, p.name,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date < $3), 0)::bigint as inngaende,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $4), 0)::bigint as utgaende
         from party p
         join entry e on e.party_id = p.id
         join voucher v on v.id = e.voucher_id
         where p.company_id = $1 and p.kind = $2
         group by p.id, p.party_no, p.name
         having coalesce(sum(e.amount_ore) filter (where v.voucher_date < $3), 0) <> 0
             or count(*) filter (where v.voucher_date between $3 and $4) > 0
         order by p.party_no",
    )
    .bind(company_id)
    .bind(kind)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let mut parties: Vec<ReskontroParty> = saldi
        .iter()
        .map(|r| ReskontroParty {
            party_no: r.get("party_no"),
            party_name: r.get("name"),
            inngaende_ore: r.get("inngaende"),
            utgaende_ore: r.get("utgaende"),
            posts: Vec::new(),
        })
        .collect();
    let rows = sqlx::query(
        "select p.party_no, a.number as account_number, j.code as journal_code,
                v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description,
                e.amount_ore
         from entry e
         join voucher v on v.id = e.voucher_id
         join journal j on j.id = v.journal_id
         join account a on a.id = e.account_id
         join party p on p.id = e.party_id
         where p.company_id = $1 and p.kind = $2
           and v.voucher_date between $3 and $4
         order by p.party_no, v.voucher_date, v.chain_seq, e.line_no",
    )
    .bind(company_id)
    .bind(kind)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    // Index by party_no rather than walking both orderings in step: the
    // two queries are separate statements, so a voucher posted between
    // them can produce a line whose party the first query never saw.
    // That line belongs to no block and is skipped — the alternative,
    // assuming the orderings line up, would be an index panic.
    let plass: std::collections::HashMap<String, usize> = parties
        .iter()
        .enumerate()
        .map(|(i, p)| (p.party_no.clone(), i))
        .collect();
    for r in &rows {
        let party_no: String = r.get("party_no");
        let Some(&idx) = plass.get(&party_no) else {
            continue;
        };
        let block = &mut parties[idx];
        let amount: i64 = r.get("amount_ore");
        let saldo = block
            .posts
            .last()
            .map_or(block.inngaende_ore, |p| p.saldo_ore)
            + amount;
        block.posts.push(ReskontroPost {
            account_number: r.get("account_number"),
            journal_code: r.get("journal_code"),
            fiscal_year: r.get("fiscal_year"),
            voucher_number: r.get("voucher_number"),
            voucher_date: r.get("voucher_date"),
            description: r.get("description"),
            amount_ore: amount,
            saldo_ore: saldo,
        });
    }
    Ok(parties)
}

#[derive(Debug)]
pub struct BokforingLine {
    pub line_no: i32,
    pub account_number: String,
    pub account_name: String,
    pub amount_ore: i64,
    pub vat_code: Option<String>,
    pub description: Option<String>,
    pub party_no: Option<String>,
}

#[derive(Debug)]
pub struct BokforingVoucher {
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: NaiveDate,
    pub description: String,
    pub lines: Vec<BokforingLine>,
}

/// Bokføringsspesifikasjon: every voucher in the period in posting
/// order (chain order — which is also the audit order), with all lines.
pub async fn bokforingsspesifikasjon(
    pool: &PgPool,
    company_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<BokforingVoucher>> {
    let rows = sqlx::query(
        "select j.code as journal_code, v.fiscal_year, v.voucher_number, v.voucher_date,
                v.description as voucher_description, v.chain_seq,
                e.line_no, a.number as account_number, a.name as account_name,
                e.amount_ore, e.vat_code, e.description as line_description, p.party_no
         from voucher v
         join journal j on j.id = v.journal_id
         join entry e on e.voucher_id = v.id
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by v.chain_seq, e.line_no",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let mut vouchers: Vec<BokforingVoucher> = Vec::new();
    let mut last_seq: Option<i64> = None;
    for row in &rows {
        let seq: i64 = row.get("chain_seq");
        if last_seq != Some(seq) {
            last_seq = Some(seq);
            vouchers.push(BokforingVoucher {
                journal_code: row.get("journal_code"),
                fiscal_year: row.get("fiscal_year"),
                voucher_number: row.get("voucher_number"),
                voucher_date: row.get("voucher_date"),
                description: row.get("voucher_description"),
                lines: Vec::new(),
            });
        }
        vouchers
            .last_mut()
            .expect("voucher pushed above")
            .lines
            .push(BokforingLine {
                line_no: row.get("line_no"),
                account_number: row.get("account_number"),
                account_name: row.get("account_name"),
                amount_ore: row.get("amount_ore"),
                vat_code: row.get("vat_code"),
                description: row.get("line_description"),
                party_no: row.get("party_no"),
            });
    }
    Ok(vouchers)
}

/// Nøkkeltall for oversikten (docs/rapporter.md, #36): resultat hittil
/// Key figures for the overview (docs/rapporter.md, #36): result year to
/// date against the same period last year, result per month, and the
/// liquidity picture — all plain SUM queries over the hovedbok and the
/// reskontro, never stored state.
#[derive(Debug)]
pub struct Nokkeltall {
    pub year: i32,
    /// Presentasjonsfortegn: overskudd positivt.
    pub resultat_hittil_ore: i64,
    pub resultat_fjor_ore: i64,
    /// Index 0 = januar; presentasjonsfortegn.
    pub maaneder: [i64; 12],
    /// Bank and cash (the 19xx accounts), now.
    pub bank_ore: i64,
    /// Utestående kundefordringer (kundereskontroens saldo), nå.
    pub kundefordringer_ore: i64,
    /// Owed to suppliers (positive amount), now.
    pub leverandorgjeld_ore: i64,
}

pub async fn nokkeltall(
    pool: &sqlx::PgPool,
    company_id: uuid::Uuid,
    year: i32,
    today: chrono::NaiveDate,
) -> anyhow::Result<Nokkeltall> {
    use chrono::Datelike;
    // The result accounts are 3xxx–8xxx; presentation flips the sign
    // (income is credit in the hovedbok).
    let month_rows = sqlx::query(
        "select extract(month from v.voucher_date)::int as maned,
                coalesce(-sum(e.amount_ore), 0)::bigint as resultat
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         where v.company_id = $1 and a.number >= '3000'
           and v.voucher_date between $2 and $3
         group by 1",
    )
    .bind(company_id)
    .bind(chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap())
    .bind(chrono::NaiveDate::from_ymd_opt(year, 12, 31).unwrap())
    .fetch_all(pool)
    .await?;
    let mut maaneder = [0i64; 12];
    for row in &month_rows {
        let maned: i32 = row.get("maned");
        if (1..=12).contains(&maned) {
            maaneder[(maned - 1) as usize] = row.get("resultat");
        }
    }

    // "Year to date": through today's date in the report year; last year
    // is measured to the same date a year earlier (29 February falls back
    // to the 28th).
    let cutoff = if today.year() == year {
        today
    } else {
        chrono::NaiveDate::from_ymd_opt(year, 12, 31).unwrap()
    };
    let fjor_cutoff = cutoff
        .with_year(cutoff.year() - 1)
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(cutoff.year() - 1, 2, 28).unwrap());
    let resultat_between = |from: chrono::NaiveDate, to: chrono::NaiveDate| {
        let pool = pool.clone();
        async move {
            let sum: i64 = sqlx::query_scalar(
                "select coalesce(-sum(e.amount_ore), 0)::bigint
                 from entry e
                 join voucher v on v.id = e.voucher_id
                 join account a on a.id = e.account_id
                 where v.company_id = $1 and a.number >= '3000'
                   and v.voucher_date between $2 and $3",
            )
            .bind(company_id)
            .bind(from)
            .bind(to)
            .fetch_one(&pool)
            .await?;
            anyhow::Ok(sum)
        }
    };
    let resultat_hittil =
        resultat_between(chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap(), cutoff).await?;
    let resultat_fjor = resultat_between(
        chrono::NaiveDate::from_ymd_opt(year - 1, 1, 1).unwrap(),
        fjor_cutoff,
    )
    .await?;

    let bank: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         where v.company_id = $1 and a.number like '19%'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    let party_saldo = |kind: &'static str| {
        let pool = pool.clone();
        async move {
            let sum: i64 = sqlx::query_scalar(
                "select coalesce(sum(e.amount_ore), 0)::bigint
                 from entry e
                 join party p on p.id = e.party_id
                 join voucher v on v.id = e.voucher_id
                 where v.company_id = $1 and p.kind = $2",
            )
            .bind(company_id)
            .bind(kind)
            .fetch_one(&pool)
            .await?;
            anyhow::Ok(sum)
        }
    };
    let kundefordringer = party_saldo("kunde").await?;
    let leverandorgjeld = -party_saldo("leverandor").await?;

    Ok(Nokkeltall {
        year,
        resultat_hittil_ore: resultat_hittil,
        resultat_fjor_ore: resultat_fjor,
        maaneder,
        bank_ore: bank,
        kundefordringer_ore: kundefordringer,
        leverandorgjeld_ore: leverandorgjeld,
    })
}
