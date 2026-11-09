//! The read side of the ledger (migration 0072, docs/hovedbok.md): sums per
//! account over any date range, read from the monthly totals for every
//! whole month in the range and from the entries themselves for the
//! partial months at its edges.
//!
//! The answer is identical to `SUM(entry.amount_ore)` over the range --
//! the totals are kept inside the posting transaction, so there is no
//! "not caught up yet" to correct for. `verify_month_totals` checks that
//! claim against the journal, and `regnmed verify-ledger` runs it.

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One account's movement over a range, in ledger sign.
#[derive(Debug, Clone)]
pub struct AccountSum {
    pub account_id: Uuid,
    pub number: String,
    pub name: String,
    /// Sum of the positive entries.
    pub debit_ore: i64,
    /// Sum of the negative entries (zero or negative).
    pub credit_ore: i64,
}

impl AccountSum {
    pub fn balance_ore(&self) -> i64 {
        self.debit_ore + self.credit_ore
    }
}

/// How a range splits: whole months `[months_from, months_to)` read from the
/// totals (`months_from` None = from the first month there is), and at
/// most two inclusive day ranges read from the entries.
#[derive(Debug, PartialEq)]
struct Split {
    months_from: Option<NaiveDate>,
    months_to: NaiveDate,
    left: Option<(NaiveDate, NaiveDate)>,
    right: Option<(NaiveDate, NaiveDate)>,
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).expect("day 1 exists in every month")
}

fn first_of_next_month(d: NaiveDate) -> NaiveDate {
    first_of_month(first_of_month(d) + chrono::Days::new(31))
}

fn split(from: Option<NaiveDate>, to: NaiveDate) -> Split {
    let months_to = first_of_month(to.succ_opt().unwrap_or(to));
    let months_from = from.map(|f| {
        if f.day() == 1 {
            f
        } else {
            first_of_next_month(f)
        }
    });
    match (from, months_from) {
        // No whole month inside the range: all of it from the entries.
        (Some(f), Some(mf)) if mf >= months_to => Split {
            months_from: Some(months_to),
            months_to,
            left: Some((f, to)),
            right: None,
        },
        _ => Split {
            months_from,
            months_to,
            left: from
                .zip(months_from)
                .filter(|(f, mf)| f < mf)
                .map(|(f, mf)| (f, mf.pred_opt().expect("after a real date"))),
            right: (months_to <= to).then_some((months_to, to)),
        },
    }
}

/// Every account with entries in `[from, to]` (`from` None = since the
/// first voucher), ordered by account number. An account whose entries
/// in the range net to zero is included with zero, exactly as a
/// `GROUP BY` over the entries would include it.
pub async fn account_sums(
    pool: &PgPool,
    company_id: Uuid,
    from: Option<NaiveDate>,
    to: NaiveDate,
) -> Result<Vec<AccountSum>> {
    let s = split(from, to);
    let rows = sqlx::query(
        "select a.id, a.number, a.name,
                sum(s.debit)::bigint as debit, sum(s.credit)::bigint as credit
         from (
             select account_id, debit_ore as debit, credit_ore as credit
             from account_month_total
             where company_id = $1 and ($2::date is null or month >= $2) and month < $3
             union all
             select e.account_id, greatest(e.amount_ore, 0), least(e.amount_ore, 0)
             from entry e
             join voucher v on v.id = e.voucher_id
             where v.company_id = $1
               and (v.voucher_date between $4 and $5 or v.voucher_date between $6 and $7)
         ) s
         join account a on a.id = s.account_id and a.company_id = $1
         group by a.id, a.number, a.name
         order by a.number",
    )
    .bind(company_id)
    .bind(s.months_from)
    .bind(s.months_to)
    .bind(s.left.map(|r| r.0))
    .bind(s.left.map(|r| r.1))
    .bind(s.right.map(|r| r.0))
    .bind(s.right.map(|r| r.1))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AccountSum {
            account_id: r.get("id"),
            number: r.get("number"),
            name: r.get("name"),
            debit_ore: r.get("debit"),
            credit_ore: r.get("credit"),
        })
        .collect())
}

/// Every (account, month) whose stored total differs from the entries,
/// as readable lines. Empty means the read side agrees with the journal.
pub async fn verify_month_totals(pool: &PgPool, company_id: Uuid) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "with actual as (
             select e.account_id, date_trunc('month', v.voucher_date)::date as month,
                    coalesce(sum(e.amount_ore) filter (where e.amount_ore > 0), 0)::bigint as debit,
                    coalesce(sum(e.amount_ore) filter (where e.amount_ore < 0), 0)::bigint as credit,
                    count(*)::bigint as n
             from entry e join voucher v on v.id = e.voucher_id
             where v.company_id = $1
             group by 1, 2
         ), stored as (
             select account_id, month, debit_ore as debit, credit_ore as credit, entry_count as n
             from account_month_total where company_id = $1
         )
         select a.number, coalesce(x.month, t.month) as month,
                x.debit as actual_debit, x.credit as actual_credit, x.n as actual_n,
                t.debit as stored_debit, t.credit as stored_credit, t.n as stored_n
         from actual x
         full join stored t on t.account_id = x.account_id and t.month = x.month
         join account a on a.id = coalesce(x.account_id, t.account_id)
         where x.debit is distinct from t.debit
            or x.credit is distinct from t.credit
            or x.n is distinct from t.n
         order by a.number, 2",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let month: NaiveDate = r.get("month");
            format!(
                "account {} {}: entries debit {:?} credit {:?} count {:?}, stored debit {:?} credit {:?} count {:?}",
                r.get::<String, _>("number"),
                month.format("%Y-%m"),
                r.get::<Option<i64>, _>("actual_debit"),
                r.get::<Option<i64>, _>("actual_credit"),
                r.get::<Option<i64>, _>("actual_n"),
                r.get::<Option<i64>, _>("stored_debit"),
                r.get::<Option<i64>, _>("stored_credit"),
                r.get::<Option<i64>, _>("stored_n"),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn whole_months_come_from_the_totals_and_the_edges_from_the_entries() {
        assert_eq!(
            split(Some(d("2026-01-15")), d("2026-04-10")),
            Split {
                months_from: Some(d("2026-02-01")),
                months_to: d("2026-04-01"),
                left: Some((d("2026-01-15"), d("2026-01-31"))),
                right: Some((d("2026-04-01"), d("2026-04-10"))),
            }
        );
        // A calendar year is twelve whole months and nothing else.
        assert_eq!(
            split(Some(d("2026-01-01")), d("2026-12-31")),
            Split {
                months_from: Some(d("2026-01-01")),
                months_to: d("2027-01-01"),
                left: None,
                right: None,
            }
        );
        // Since the beginning, through the middle of a month.
        assert_eq!(
            split(None, d("2026-02-28").pred_opt().unwrap()),
            Split {
                months_from: None,
                months_to: d("2026-02-01"),
                left: None,
                right: Some((d("2026-02-01"), d("2026-02-27"))),
            }
        );
        // Leap day ends February.
        assert_eq!(split(None, d("2028-02-29")).months_to, d("2028-03-01"));
    }

    #[test]
    fn a_range_inside_one_month_is_read_from_the_entries_alone() {
        for (from, to) in [
            ("2026-03-05", "2026-03-20"),
            ("2026-03-05", "2026-04-20"),
            ("2026-03-01", "2026-03-30"),
            ("2026-12-31", "2026-12-31"),
        ] {
            let s = split(Some(d(from)), d(to));
            assert_eq!(s.left, Some((d(from), d(to))), "{from}..{to}");
            assert_eq!(s.right, None);
            assert_eq!(s.months_from, Some(s.months_to), "no whole months");
        }
    }
}
