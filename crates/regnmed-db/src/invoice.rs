//! Invoice persistence: gap-free issuing atomic with the ledger posting,
//! listing with reskontro remainders, and kreditnotaer.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::fakturapdf::{Dokumenttype, FakturaPdfInput, PdfLinje, render_faktura_pdf};
use regnmed_core::invoice::{InvoiceLineInput, build_voucher, compute, invoice_kid};
use regnmed_core::mva::rate_on;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::attachment::add_attachment_in;
use crate::ledger::post_voucher_in;
use crate::mva::load_vat_rates;

#[derive(Debug, Clone)]
pub struct InvoiceLineDraft {
    pub description: String,
    pub account_number: String,
    /// Thousandths; defaults to 1000 (one unit) at the API layer.
    pub quantity_milli: i64,
    pub unit_price_ore: i64,
    pub vat_code: Option<String>,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
    /// Product reference (docs/produkter.md) — for lager and
    /// traceability only; every value above is a copy taken at issue.
    pub product_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct InvoiceDraft {
    /// Set for a kontantfaktura (#89, §5-3): the ytelse was paid on
    /// delivery, and this is what settled it ("Kort", "Vipps",
    /// "Kontant"). It changes the DOCUMENT — no KID, no forfall — so it
    /// belongs on the draft, not only on the posting.
    pub kontant_betalingsmiddel: Option<String>,
    pub party_no: String,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    /// Leveringstidspunkt (bokføringsforskriften §5-1-1 nr. 4). NOT an
    /// Option: every salgsdokument must state when the ytelse was
    /// delivered, so the caller has to decide. The invoice date is a
    /// legitimate value — it is the usual one — but it has to be
    /// CHOSEN here, never assumed further down.
    pub delivery_date: NaiveDate,
    /// Leveringssted, same hjemmel. Optional because the forskrift asks
    /// for it "der det er relevant" — a place is meaningful for a
    /// vareleveranse and rarely for a fjernlevert tjeneste.
    pub delivery_place: Option<String>,
    pub journal_code: String,
    pub receivable_account: String,
    pub vat_account: String,
    /// Document currency (docs/valuta.md). None = NOK. When set, every
    /// line amount is in the currency's MINOR UNIT (cent); posting
    /// converts to NOK at the dagskurs and the entries carry the
    /// valutainformasjon (hash format v4).
    pub valuta: Option<String>,
    /// Forces the booking rate (kreditnota reverses at the ORIGINAL
    /// kurs so the NOK zeroes exactly). None = resolve by invoice date.
    pub valuta_kurs_micro: Option<i64>,
    pub lines: Vec<InvoiceLineDraft>,
}

#[derive(Debug)]
pub struct IssuedInvoice {
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub kid: String,
    /// Document amounts: øre for NOK invoices, the currency's minor
    /// unit (cent) when the invoice is in valuta.
    pub net_ore: i64,
    pub vat_ore: i64,
    pub gross_ore: i64,
    /// NOK gross actually posted (equals gross_ore for NOK invoices).
    pub gross_nok_ore: i64,
    pub voucher_number: i64,
    pub fiscal_year: i32,
}

/// Resolves rates per line (dated by invoice date) and turns drafts into
/// the pure computation input. Zero-rate/uncoded lines get rate 0.
async fn resolve_lines(
    pool: &PgPool,
    invoice_date: NaiveDate,
    lines: &[InvoiceLineDraft],
) -> Result<Vec<InvoiceLineInput>> {
    let rates = load_vat_rates(pool).await?;
    let mut resolved = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let rate_bp = match &line.vat_code {
            Some(code) => {
                let rate_class: String =
                    sqlx::query_scalar("select rate_class from vat_code where code = $1")
                        .bind(code)
                        .fetch_optional(pool)
                        .await?
                        .with_context(|| format!("line {}: unknown vat code {code}", i + 1))?;
                rate_on(&rates, &rate_class, invoice_date)
                    .with_context(|| format!("line {}: no rate for {invoice_date}", i + 1))?
            }
            None => 0,
        };
        ensure!(
            !line.description.is_empty(),
            "line {}: empty description",
            i + 1
        );
        resolved.push(InvoiceLineInput {
            description: line.description.clone(),
            account_number: line.account_number.clone(),
            quantity_milli: line.quantity_milli,
            unit_price_ore: line.unit_price_ore,
            vat_code: line.vat_code.clone(),
            rate_bp,
            avdeling: line.avdeling.clone(),
            prosjekt: line.prosjekt.clone(),
        });
    }
    Ok(resolved)
}

/// Issues an invoice: one transaction covering the gap-free invoice
/// number, the ledger posting (voucher counter, hash chain), the
/// invoice rows and the salgsdokument-PDF — everything rolls back
/// together.
pub async fn create_invoice(
    pool: &PgPool,
    company_id: Uuid,
    draft: &InvoiceDraft,
    created_by: &str,
    credits_invoice_id: Option<Uuid>,
) -> Result<IssuedInvoice> {
    let mut tx = pool.begin().await?;
    let issued = create_invoice_in(
        pool,
        &mut tx,
        company_id,
        draft,
        created_by,
        credits_invoice_id,
    )
    .await?;
    tx.commit().await?;
    Ok(issued)
}

/// Transaction-taking variant, so callers (repeterende faktura) can
/// make the issue atomic with their own writes. Master-data pre-reads
/// go to the pool; every write goes to the caller's transaction.
pub async fn create_invoice_in(
    pool: &PgPool,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    draft: &InvoiceDraft,
    created_by: &str,
    credits_invoice_id: Option<Uuid>,
) -> Result<IssuedInvoice> {
    ensure!(
        !draft.lines.is_empty(),
        "an invoice needs at least one line"
    );
    let party = sqlx::query(
        "select id, kind, name, orgnr, address from party
         where company_id = $1 and party_no = $2",
    )
    .bind(company_id)
    .bind(&draft.party_no)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no party {}", draft.party_no))?;
    let party_id: Uuid = party.get("id");
    ensure!(
        party.get::<String, _>("kind") == "kunde",
        "party {} is not a kunde",
        draft.party_no
    );
    let company = sqlx::query(
        "select name, orgnr, address, bank_account, orgform from company where id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    // §5-1-2 krever navn OG adresse for begge parter. Kravet håndheves
    // ved utstedelse, ikke som en advarsel etterpå: fakturaen er
    // uforanderlig i det den finnes, så et manglende felt kan ikke
    // rettes — bare krediteres og gjøres om (#81).
    let utfylt = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    ensure!(
        utfylt(company.get("address")).is_some(),
        "selskapet mangler adresse — salgsdokumentet krever den (bokføringsforskriften §5-1-2). \
         Fyll den ut under Administrasjon → Firmaopplysninger."
    );
    ensure!(
        utfylt(party.get("address")).is_some(),
        "kunde {} mangler adresse — salgsdokumentet krever den (bokføringsforskriften §5-1-2). \
         Fyll den ut på kundens side under Kunder.",
        draft.party_no
    );
    let credited_invoice_no: Option<i64> = match credits_invoice_id {
        Some(id) => {
            sqlx::query_scalar("select invoice_no from invoice where id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
        None => None,
    };

    let lines = resolve_lines(pool, draft.invoice_date, &draft.lines).await?;
    let computed = compute(&lines);
    if credits_invoice_id.is_none() && computed.gross_ore <= 0 {
        bail!("invoice total must be positive (use a kreditnota to credit)");
    }

    let invoice_no: i64 = sqlx::query(
        "insert into invoice_counter (company_id, last_number) values ($1, 1)
         on conflict (company_id)
         do update set last_number = invoice_counter.last_number + 1
         returning last_number",
    )
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await?
    .get("last_number");
    let kid = invoice_kid(invoice_no);

    let mut gross_nok_ore = computed.gross_ore;
    let voucher = match &draft.valuta {
        None => build_voucher(
            &draft.journal_code,
            draft.invoice_date,
            invoice_no,
            credits_invoice_id.is_some(),
            &draft.party_no,
            &draft.receivable_account,
            &draft.vat_account,
            &lines,
            &computed,
        )?,
        Some(code) => {
            let kurs_micro = match draft.valuta_kurs_micro {
                Some(kurs) => kurs,
                None => {
                    crate::valuta::require_kurs(pool, code, draft.invoice_date)
                        .await?
                        .1
                }
            };
            let (voucher, gross_nok) = build_valuta_voucher(
                draft,
                invoice_no,
                credits_invoice_id.is_some(),
                &lines,
                &computed,
                code,
                kurs_micro,
            )?;
            gross_nok_ore = gross_nok;
            voucher
        }
    };
    let posted = post_voucher_in(tx, company_id, &voucher, created_by).await?;

    let receivable_entry_id: Uuid =
        sqlx::query_scalar("select id from entry where voucher_id = $1 and party_id = $2")
            .bind(posted.id)
            .bind(party_id)
            .fetch_one(&mut **tx)
            .await?;

    let invoice_id = Uuid::now_v7();
    sqlx::query(
        "insert into invoice (id, company_id, party_id, invoice_no, invoice_date, due_date,
                              kid, credits_invoice_id, voucher_id, receivable_entry_id, created_by,
                              valuta, delivery_date, delivery_place)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(invoice_id)
    .bind(company_id)
    .bind(party_id)
    .bind(invoice_no)
    .bind(draft.invoice_date)
    .bind(draft.due_date)
    .bind(&kid)
    .bind(credits_invoice_id)
    .bind(posted.id)
    .bind(receivable_entry_id)
    .bind(created_by)
    .bind(&draft.valuta)
    .bind(draft.delivery_date)
    .bind(&draft.delivery_place)
    .execute(&mut **tx)
    .await?;

    for (i, (line, amounts)) in lines.iter().zip(&computed.lines).enumerate() {
        sqlx::query(
            "insert into invoice_line (id, invoice_id, line_no, description, account_number,
                                       quantity_milli, unit_price_ore, net_ore, vat_code, vat_ore,
                                       avdeling, prosjekt, product_id)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .bind((i + 1) as i32)
        .bind(&line.description)
        .bind(&line.account_number)
        .bind(line.quantity_milli)
        .bind(line.unit_price_ore)
        .bind(amounts.net_ore)
        .bind(&line.vat_code)
        .bind(amounts.vat_ore)
        .bind(&line.avdeling)
        .bind(&line.prosjekt)
        .bind(draft.lines[i].product_id)
        .execute(&mut **tx)
        .await?;
    }

    // Lagerførte products move stock in the same transaction as the
    // posting (kreditnota lines return it) — docs/produkter.md.
    crate::product::record_sales_in(
        tx,
        company_id,
        invoice_id,
        draft.invoice_date,
        &draft.lines,
        created_by,
    )
    .await?;

    // The salgsdokument itself: rendered deterministically and stored
    // as an attachment on the voucher IN THE SAME TRANSACTION — what
    // the customer receives is part of oppbevaringen from the moment
    // the invoice exists (bokføringsforskriften §5-1, issue #32).
    // Påtegningene «MVA» og «Foretaksregisteret» (§5-1-2) kommer fra
    // selskapets LAGREDE registreringsstatus på fakturadatoen, ikke fra
    // dokumentet: en registrert selger som fakturerer eksport eller
    // fritatt omsetning har ingen mva på fakturaen og skal likevel ha
    // påtegningen, og registreringsplikten i Foretaksregisteret gjelder
    // flere enn AS/ASA (#81).
    let reg = crate::settings::registrering_on(pool, company_id, draft.invoice_date).await?;
    let pdf = render_faktura_pdf(&FakturaPdfInput {
        dokumenttype: if credits_invoice_id.is_some() {
            Dokumenttype::Kreditnota
        } else if draft.kontant_betalingsmiddel.is_some() {
            Dokumenttype::Kontantfaktura
        } else {
            Dokumenttype::Faktura
        },
        betalingsmiddel: draft.kontant_betalingsmiddel.clone(),
        krediterer_nr: credited_invoice_no,
        selger_navn: company.get("name"),
        selger_orgnr: company.get("orgnr"),
        selger_adresse: company.get("address"),
        selger_mva_registrert: reg.mva_registrert,
        selger_foretaksregistrert: reg.foretaksregistrert,
        selger_kontonummer: company.get("bank_account"),
        kjoper_navn: party.get("name"),
        kjoper_nr: draft.party_no.clone(),
        kjoper_orgnr: party.get("orgnr"),
        kjoper_adresse: party.get("address"),
        fakturanr: invoice_no,
        fakturadato: draft.invoice_date,
        forfallsdato: draft.due_date,
        leveringsdato: Some(draft.delivery_date),
        leveringssted: draft.delivery_place.clone(),
        kid: kid.clone(),
        valuta: draft.valuta.clone(),
        motverdi_nok_ore: draft.valuta.as_ref().map(|_| gross_nok_ore),
        linjer: lines
            .iter()
            .zip(&computed.lines)
            .map(|(line, amounts)| PdfLinje {
                beskrivelse: line.description.clone(),
                antall_milli: line.quantity_milli,
                enhetspris_ore: line.unit_price_ore,
                mva_sats_bp: line.vat_code.as_ref().map(|_| line.rate_bp),
                netto_ore: amounts.net_ore,
                mva_ore: amounts.vat_ore,
            })
            .collect(),
    });
    let dokument = if credits_invoice_id.is_some() {
        "kreditnota"
    } else {
        "faktura"
    };
    add_attachment_in(
        tx,
        company_id,
        posted.id,
        &format!("{dokument}-{invoice_no}.pdf"),
        "application/pdf",
        &pdf,
        created_by,
    )
    .await?;

    Ok(IssuedInvoice {
        invoice_id,
        invoice_no,
        kid,
        net_ore: computed.net_ore,
        vat_ore: computed.vat_ore,
        gross_ore: computed.gross_ore,
        gross_nok_ore,
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
    })
}

/// The ledger posting for a valuta invoice: every line amount is
/// converted cent → øre at the SAME kurs, half away from zero per
/// line; the receivable is the exact sum of the converted parts (so
/// the voucher balances by construction), and every entry carries the
/// valutainformasjon it arose from (hash format v4). Returns the
/// voucher and the NOK gross.
#[allow(clippy::too_many_arguments)]
fn build_valuta_voucher(
    draft: &InvoiceDraft,
    invoice_no: i64,
    credit_note: bool,
    lines: &[regnmed_core::invoice::InvoiceLineInput],
    computed: &regnmed_core::invoice::ComputedInvoice,
    valuta: &str,
    kurs_micro: i64,
) -> Result<(regnmed_core::voucher::VoucherDraft, i64)> {
    use regnmed_core::valuta::{Valuta, nok_ore};
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};

    let vat_nok = nok_ore(computed.vat_ore, kurs_micro);
    let mut gross_nok = vat_nok;
    let mut entries = Vec::with_capacity(lines.len() + 2);
    for (line, amounts) in lines.iter().zip(&computed.lines) {
        let net_nok = nok_ore(amounts.net_ore, kurs_micro);
        gross_nok += net_nok;
        entries.push(EntryDraft {
            account_number: line.account_number.clone(),
            amount: regnmed_core::Ore(-net_nok),
            vat_code: line.vat_code.clone(),
            description: Some(line.description.clone()),
            party_no: None,
            avdeling: line.avdeling.clone(),
            prosjekt: line.prosjekt.clone(),
            valuta: Some(Valuta {
                valuta: valuta.to_string(),
                belop_cent: -amounts.net_ore,
                kurs_micro,
            }),
        });
    }
    if vat_nok != 0 {
        entries.push(EntryDraft {
            account_number: draft.vat_account.clone(),
            amount: regnmed_core::Ore(-vat_nok),
            vat_code: None,
            description: None,
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: Some(Valuta {
                valuta: valuta.to_string(),
                belop_cent: -computed.vat_ore,
                kurs_micro,
            }),
        });
    }
    entries.insert(
        0,
        EntryDraft {
            account_number: draft.receivable_account.clone(),
            amount: regnmed_core::Ore(gross_nok),
            vat_code: None,
            description: None,
            party_no: Some(draft.party_no.clone()),
            avdeling: None,
            prosjekt: None,
            valuta: Some(Valuta {
                valuta: valuta.to_string(),
                belop_cent: computed.gross_ore,
                kurs_micro,
            }),
        },
    );
    let label = if credit_note { "Kreditnota" } else { "Faktura" };
    let voucher = VoucherDraft {
        journal_code: draft.journal_code.clone(),
        voucher_date: draft.invoice_date,
        description: format!("{label} {invoice_no} ({valuta})"),
        reverses: None,
        entries,
    };
    voucher.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok((voucher, gross_nok))
}

/// Full kreditnota for an invoice: same lines negated, posted, and the
/// two receivable entries are reskontro-matched for whatever remains
/// open on the original.
pub async fn credit_invoice(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    created_by: &str,
) -> Result<IssuedInvoice> {
    let original = sqlx::query(
        "select i.id, i.invoice_no, i.receivable_entry_id, p.party_no, i.valuta,
                i.invoice_date, i.delivery_date, i.delivery_place,
                (select e.kurs_micro from entry e where e.id = i.receivable_entry_id)
                    as kurs_micro,
                (select exists (select 1 from invoice c where c.credits_invoice_id = i.id))
                    as already_credited
         from invoice i
         join party p on p.id = i.party_id
         where i.id = $1 and i.company_id = $2 and i.credits_invoice_id is null",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice (or it is itself a kreditnota)")?;
    ensure!(
        !original.get::<bool, _>("already_credited"),
        "invoice is already credited"
    );

    let line_rows = sqlx::query(
        "select l.description, l.account_number, l.quantity_milli, l.unit_price_ore, l.vat_code,
                l.avdeling, l.prosjekt, l.product_id,
                v.voucher_date, i.due_date, j.code as journal_code,
                (select a.number from entry e join account a on a.id = e.account_id
                 where e.id = i.receivable_entry_id) as receivable_account
         from invoice_line l
         join invoice i on i.id = l.invoice_id
         join voucher v on v.id = i.voucher_id
         join journal j on j.id = v.journal_id
         where l.invoice_id = $1
         order by l.line_no",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;
    ensure!(!line_rows.is_empty(), "invoice has no lines");

    let today: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(pool)
        .await?;
    let draft = InvoiceDraft {
        kontant_betalingsmiddel: None,
        party_no: original.get("party_no"),
        invoice_date: today,
        due_date: today,
        // The kreditnota credits a delivery that already happened, so
        // it repeats the ORIGINAL leveringstidspunkt — dating it today
        // would state that something was delivered on the day the
        // correction was written. Invoices issued before #81 have no
        // recorded delivery date; their own invoice date is the closest
        // fact we hold, and it beats leaving the field empty.
        delivery_date: original
            .get::<Option<NaiveDate>, _>("delivery_date")
            .unwrap_or_else(|| original.get("invoice_date")),
        delivery_place: original.get("delivery_place"),
        journal_code: line_rows[0].get("journal_code"),
        receivable_account: line_rows[0].get("receivable_account"),
        vat_account: "2700".into(),
        // A kreditnota reverses at the ORIGINAL kurs, so the NOK side
        // zeroes exactly — no fake agio out of a correction.
        valuta: original.get("valuta"),
        valuta_kurs_micro: original.get("kurs_micro"),
        lines: line_rows
            .iter()
            .map(|r| InvoiceLineDraft {
                description: r.get("description"),
                account_number: r.get("account_number"),
                quantity_milli: -r.get::<i64, _>("quantity_milli"),
                unit_price_ore: r.get("unit_price_ore"),
                vat_code: r.get("vat_code"),
                avdeling: r.get("avdeling"),
                prosjekt: r.get("prosjekt"),
                product_id: r.get("product_id"),
            })
            .collect(),
    };
    let credit = create_invoice(pool, company_id, &draft, created_by, Some(invoice_id)).await?;

    // Match the credit against whatever is still open on the original.
    let original_entry: Uuid = original.get("receivable_entry_id");
    let credit_entry: Uuid =
        sqlx::query_scalar("select receivable_entry_id from invoice where id = $1")
            .bind(credit.invoice_id)
            .fetch_one(pool)
            .await?;
    let party_id: Uuid = sqlx::query_scalar("select party_id from entry where id = $1")
        .bind(original_entry)
        .fetch_one(pool)
        .await?;
    let items = crate::reskontro::party_items(pool, company_id, party_id, false).await?;
    let remaining = items
        .iter()
        .find(|i| i.entry_id == original_entry)
        .map(|i| i.remaining_ore)
        .unwrap_or(0);
    let creditable = remaining.min(-credit.gross_ore);
    if creditable > 0 {
        crate::reskontro::match_items(
            pool,
            company_id,
            original_entry,
            credit_entry,
            creditable,
            created_by,
        )
        .await?;
    }
    Ok(credit)
}

#[derive(Debug)]
pub struct InvoiceRow {
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub party_no: String,
    pub party_name: String,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    pub kid: String,
    pub gross_ore: i64,
    pub remaining_ore: i64,
    pub is_credit_note: bool,
}

pub async fn list_invoices(
    pool: &PgPool,
    company_id: Uuid,
    open_only: bool,
) -> Result<Vec<InvoiceRow>> {
    let rows = sqlx::query(
        "select i.id, i.invoice_no, p.party_no, p.name as party_name, i.invoice_date,
                i.due_date, i.kid, (i.credits_invoice_id is not null) as is_credit_note,
                e.amount_ore as gross_ore,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore
         from invoice i
         join party p on p.id = i.party_id
         join entry e on e.id = i.receivable_entry_id
         where i.company_id = $1
         order by i.invoice_no",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| InvoiceRow {
            invoice_id: r.get("id"),
            invoice_no: r.get("invoice_no"),
            party_no: r.get("party_no"),
            party_name: r.get("party_name"),
            invoice_date: r.get("invoice_date"),
            due_date: r.get("due_date"),
            kid: r.get("kid"),
            gross_ore: r.get("gross_ore"),
            remaining_ore: r.get("remaining_ore"),
            is_credit_note: r.get("is_credit_note"),
        })
        .filter(|row| !open_only || row.remaining_ore != 0)
        .collect())
}

/// Kontantfaktura (#89, bokføringsforskriften §5-3): a salgsdokument for
/// a ytelse paid on delivery.
///
/// The receivable ARISES AND IS SETTLED in one transaction. It would be
/// simpler to post the sale straight against bank and skip 1500
/// entirely, and that is exactly what must not happen: the reskontro
/// doctrine says a customer's postings carry a party, and a side door
/// past it would make `reskontro_kontroll` — the revisor's tie-out —
/// quietly incomplete. The customer history stays true, and the open
/// item is closed the instant it exists.
///
/// `oppgjorskonto` is where the money landed: 1900 kontanter, 1920 bank,
/// or the card acquirer's clearing account. The caller names it; we do
/// not guess how someone was paid.
pub async fn create_kontantfaktura(
    pool: &PgPool,
    company_id: Uuid,
    draft: &InvoiceDraft,
    oppgjorskonto: &str,
    betalingsmiddel: &str,
    created_by: &str,
) -> Result<IssuedInvoice> {
    ensure!(
        !betalingsmiddel.trim().is_empty(),
        "kontantfakturaen må si hva den ble gjort opp med (§5-3)"
    );
    // Set here rather than trusted from the caller: a kontantfaktura
    // that reached the PDF as an ordinary faktura would print a KID for
    // money already received.
    let draft = &InvoiceDraft {
        kontant_betalingsmiddel: Some(betalingsmiddel.trim().to_string()),
        ..InvoiceDraft::clone(draft)
    };
    let mut tx = pool.begin().await?;
    let utstedt = create_invoice_in(pool, &mut tx, company_id, draft, created_by, None).await?;

    let (party_no, receivable_entry, gross): (String, Uuid, i64) = {
        let row = sqlx::query(
            "select p.party_no, i.receivable_entry_id, e.amount_ore::bigint as gross
             from invoice i
             join party p on p.id = i.party_id
             join entry e on e.id = i.receivable_entry_id
             where i.id = $1",
        )
        .bind(utstedt.invoice_id)
        .fetch_one(&mut *tx)
        .await?;
        (
            row.get("party_no"),
            row.get("receivable_entry_id"),
            row.get("gross"),
        )
    };

    let oppgjor = regnmed_core::voucher::VoucherDraft {
        journal_code: draft.journal_code.clone(),
        voucher_date: draft.invoice_date,
        description: format!(
            "Kontantsalg, oppgjør faktura {} ({betalingsmiddel})",
            utstedt.invoice_no
        ),
        reverses: None,
        entries: vec![
            regnmed_core::voucher::EntryDraft {
                account_number: oppgjorskonto.to_string(),
                amount: regnmed_core::Ore(gross),
                vat_code: None,
                description: Some(betalingsmiddel.trim().to_string()),
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            regnmed_core::voucher::EntryDraft {
                account_number: draft.receivable_account.clone(),
                amount: regnmed_core::Ore(-gross),
                vat_code: None,
                description: None,
                // The party goes on BOTH sides of the customer's history:
                // the claim and its settlement.
                party_no: Some(party_no),
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    };
    let posted = crate::post_voucher_in(&mut tx, company_id, &oppgjor, created_by).await?;
    let betalings_entry: Uuid = sqlx::query_scalar(
        "select e.id from entry e join account a on a.id = e.account_id
         where e.voucher_id = $1 and a.number = $2",
    )
    .bind(posted.id)
    .bind(&draft.receivable_account)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::now_v7())
    .bind(receivable_entry)
    .bind(betalings_entry)
    .bind(gross)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(utstedt)
}
