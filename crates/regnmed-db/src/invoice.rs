//! Invoice persistence: gap-free issuing atomic with the ledger posting,
//! listing with reskontro remainders, and kreditnotaer.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::fakturapdf::{DocumentType, InvoicePdfInput, PdfLinje, render_invoice_pdf};
use regnmed_core::invoice::{InvoiceLineInput, build_voucher, compute, invoice_kid};
use regnmed_core::vat::rate_on;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::attachment::add_attachments_in;
use crate::ledger::post_voucher_in;
use crate::vat::load_vat_rates;

#[derive(Debug, Clone)]
pub struct InvoiceLineDraft {
    pub description: String,
    pub account_number: String,
    /// Thousandths; defaults to 1000 (one unit) at the API layer.
    pub quantity_milli: i64,
    pub unit_price_ore: i64,
    pub vat_code: Option<String>,
    pub department: Option<String>,
    pub project: Option<String>,
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
    pub cash_payment_method: Option<String>,
    pub party_no: String,
    /// The buyer's own reference ("Deres referanse", EN 16931 BT-10),
    /// printed on the PDF and carried in the EHF. None = the customer's
    /// standing reference, if it has one (#221).
    pub buyer_reference: Option<String>,
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
    pub currency: Option<String>,
    /// Forces the booking rate (kreditnota reverses at the ORIGINAL
    /// kurs so the NOK zeroes exactly). None = resolve by invoice date.
    pub exchange_rate_micro: Option<i64>,
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
    company_id: Uuid,
    invoice_date: NaiveDate,
    lines: &[InvoiceLineDraft],
) -> Result<Vec<InvoiceLineInput>> {
    // Refused before any rate is looked up: an unregistered seller may
    // not charge output VAT at all (#99).
    if lines.iter().any(|l| {
        l.vat_code
            .as_deref()
            .is_some_and(crate::settings::requires_vat_registration)
    }) {
        crate::settings::require_vat_registered(pool, company_id, invoice_date).await?;
    }
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
            department: line.department.clone(),
            project: line.project.clone(),
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
        "select id, kind, name, orgnr, address, buyer_reference from party
         where company_id = $1 and party_no = $2",
    )
    .bind(company_id)
    .bind(&draft.party_no)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no party {}", draft.party_no))?;
    let party_id: Uuid = party.get("id");
    // The invoice's own reference, else the customer's standing one (#221).
    // A credit note takes exactly the original's (the caller passes it):
    // falling back would give it a reference the invoice never carried.
    let buyer_reference =
        match crate::settings::normalize_buyer_reference(draft.buyer_reference.as_deref())? {
            Some(own) => Some(own),
            None if credits_invoice_id.is_none() => party.get("buyer_reference"),
            None => None,
        };
    ensure!(
        party.get::<String, _>("kind") == "kunde",
        "party {} is not a kunde",
        draft.party_no
    );
    let company = sqlx::query(
        "select name, orgnr, address, bank_account, orgform, kid_avtale
         from company where id = $1",
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

    let lines = resolve_lines(pool, company_id, draft.invoice_date, &draft.lines).await?;
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
    let voucher = match &draft.currency {
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
            let rate_micro = match draft.exchange_rate_micro {
                Some(exchange_rate) => exchange_rate,
                None => {
                    crate::currency::require_exchange_rate(pool, code, draft.invoice_date)
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
                rate_micro,
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
                              valuta, delivery_date, delivery_place, buyer_reference)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
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
    .bind(&draft.currency)
    .bind(draft.delivery_date)
    .bind(&draft.delivery_place)
    .bind(&buyer_reference)
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
        .bind(&line.department)
        .bind(&line.project)
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
    let pdf = render_invoice_pdf(&InvoicePdfInput {
        document_type: if credits_invoice_id.is_some() {
            DocumentType::CreditNote
        } else if draft.cash_payment_method.is_some() {
            DocumentType::CashInvoice
        } else {
            DocumentType::Invoice
        },
        payment_method: draft.cash_payment_method.clone(),
        credits_number: credited_invoice_no,
        seller_name: company.get("name"),
        seller_orgnumber: company.get("orgnr"),
        seller_address: company.get("address"),
        seller_vat_registered: reg.vat_registered,
        seller_business_registered: reg.business_registered,
        seller_account_number: company.get("bank_account"),
        buyer_name: party.get("name"),
        buyer_number: draft.party_no.clone(),
        buyer_reference: buyer_reference.clone(),
        buyer_orgnumber: party.get("orgnr"),
        buyer_address: party.get("address"),
        invoice_number: invoice_no,
        invoice_date: draft.invoice_date,
        due_date: draft.due_date,
        delivery_date: Some(draft.delivery_date),
        delivery_place: draft.delivery_place.clone(),
        // Stored on the invoice either way (OCR resolves payments BY
        // kid, #7) — but only PRINTED when the bank can honour it (#104).
        kid: company.get::<bool, _>("kid_avtale").then(|| kid.clone()),
        currency: draft.currency.clone(),
        motverdi_nok_ore: draft.currency.as_ref().map(|_| gross_nok_ore),
        linjer: lines
            .iter()
            .zip(&computed.lines)
            .map(|(line, amounts)| PdfLinje {
                description: line.description.clone(),
                quantity_milli: line.quantity_milli,
                unit_price_ore: line.unit_price_ore,
                vat_rate_bp: line.vat_code.as_ref().map(|_| line.rate_bp),
                net_ore: amounts.net_ore,
                vat_ore: amounts.vat_ore,
            })
            .collect(),
    });
    let document = if credits_invoice_id.is_some() {
        "kreditnota"
    } else {
        "faktura"
    };
    let pdf_name = format!("{document}-{invoice_no}.pdf");
    let xml_name = format!("{document}-{invoice_no}.xml");

    // The EHF is stored in the same transaction (#222): rendered later it
    // would read whatever contact data the parties have by then, and an
    // e-invoice must be kept in its original format (bokføringsloven § 13
    // second paragraph). Without the buyer's orgnr there is no Peppol
    // address and so no EHF.
    let xml = match party.get::<Option<String>, _>("orgnr") {
        Some(_) => Some(
            crate::ehf::render_invoice_ehf(
                pool,
                tx,
                company_id,
                invoice_id,
                Some(crate::ehf::IssueFacts {
                    registrering: reg,
                    line_rates_bp: lines
                        .iter()
                        .map(|l| l.vat_code.as_ref().map(|_| l.rate_bp))
                        .collect(),
                    paid_at_issue: draft.cash_payment_method.is_some(),
                }),
            )
            .await?,
        ),
        None => None,
    };
    let mut documents: Vec<(&str, &str, &[u8])> =
        vec![(&pdf_name, "application/pdf", pdf.as_slice())];
    if let Some(xml) = &xml {
        documents.push((&xml_name, "application/xml", xml.as_bytes()));
    }
    add_attachments_in(tx, company_id, posted.id, &documents, created_by).await?;

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
    currency: &str,
    rate_micro: i64,
) -> Result<(regnmed_core::voucher::VoucherDraft, i64)> {
    use regnmed_core::currency::{Currency, nok_ore};
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};

    let vat_nok = nok_ore(computed.vat_ore, rate_micro);
    let mut gross_nok = vat_nok;
    let mut entries = Vec::with_capacity(lines.len() + 2);
    for (line, amounts) in lines.iter().zip(&computed.lines) {
        let net_nok = nok_ore(amounts.net_ore, rate_micro);
        gross_nok += net_nok;
        entries.push(EntryDraft {
            account_number: line.account_number.clone(),
            amount: regnmed_core::Ore(-net_nok),
            vat_code: line.vat_code.clone(),
            description: Some(line.description.clone()),
            party_no: None,
            department: line.department.clone(),
            project: line.project.clone(),
            currency: Some(Currency {
                code: currency.to_string(),
                amount_cent: -amounts.net_ore,
                rate_micro,
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
            department: None,
            project: None,
            currency: Some(Currency {
                code: currency.to_string(),
                amount_cent: -computed.vat_ore,
                rate_micro,
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
            department: None,
            project: None,
            currency: Some(Currency {
                code: currency.to_string(),
                amount_cent: computed.gross_ore,
                rate_micro,
            }),
        },
    );
    let label = if credit_note { "Kreditnota" } else { "Faktura" };
    let voucher = VoucherDraft {
        journal_code: draft.journal_code.clone(),
        voucher_date: draft.invoice_date,
        description: format!("{label} {invoice_no} ({currency})"),
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
                i.invoice_date, i.delivery_date, i.delivery_place, i.buyer_reference,
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
        cash_payment_method: None,
        party_no: original.get("party_no"),
        // The buyer matches a credit note to the invoice on the same
        // reference it routed the invoice on.
        buyer_reference: original.get("buyer_reference"),
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
        currency: original.get("valuta"),
        exchange_rate_micro: original.get("kurs_micro"),
        lines: line_rows
            .iter()
            .map(|r| InvoiceLineDraft {
                description: r.get("description"),
                account_number: r.get("account_number"),
                quantity_milli: -r.get::<i64, _>("quantity_milli"),
                unit_price_ore: r.get("unit_price_ore"),
                vat_code: r.get("vat_code"),
                department: r.get("avdeling"),
                project: r.get("prosjekt"),
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
    let items = crate::subledger::party_items(pool, company_id, party_id, false).await?;
    let remaining = items
        .iter()
        .find(|i| i.entry_id == original_entry)
        .map(|i| i.remaining_ore)
        .unwrap_or(0);
    let creditable = remaining.min(-credit.gross_ore);
    if creditable > 0 {
        crate::subledger::match_items(
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
    /// Derived, never stored: `credit_note`, `credited` (a kreditnota
    /// exists for it), `paid` (nothing remains), `overdue` (remains and
    /// past due) or `open`.
    pub status: String,
}

/// Which invoices a listing asks for. `None` everywhere = every invoice,
/// oldest first -- what the API returned before filtering existed.
#[derive(Debug, Default)]
pub struct InvoiceFilter<'a> {
    /// `open` (open or overdue), `overdue`, `paid`, or `credit`
    /// (kreditnotaer and the invoices they credit).
    pub status: Option<&'a str>,
    /// Matches an exact invoice number or KID, or part of the customer's
    /// name (case-insensitive).
    pub search: Option<&'a str>,
    /// Newest first and at most this many. None = all, oldest first.
    pub limit: Option<i64>,
    pub offset: i64,
    /// Keyset paging for a newest-first list: only invoices numbered
    /// below this. Unlike `offset`, a page cannot repeat or skip rows
    /// when invoices are issued between two requests. `total` and
    /// `counts` ignore it.
    pub before: Option<i64>,
    /// One customer's invoices (party number).
    pub party_no: Option<&'a str>,
}

#[derive(Debug, Default)]
pub struct InvoicePage {
    pub rows: Vec<InvoiceRow>,
    /// Matching the filter, before limit/offset.
    pub total: i64,
    /// Per derived status, over the search but regardless of the status
    /// filter -- what the filter chips show.
    pub counts: std::collections::BTreeMap<String, i64>,
}

/// Every invoice with its reskontro remainder and derived status. The
/// status is SQL so the filter, the counts and the page come from one
/// definition.
pub(crate) const INVOICE_STATUS_CTE: &str = "
    with base as (
        select i.id, i.invoice_no, p.party_no, p.name as party_name, i.invoice_date,
               i.due_date, i.kid, (i.credits_invoice_id is not null) as is_credit_note,
               e.amount_ore as gross_ore,
               e.amount_ore
               - coalesce((select sum(m.amount_ore) from reskontro_match m
                           where m.entry_a = e.id), 0)::bigint
               + coalesce((select sum(m.amount_ore) from reskontro_match m
                           where m.entry_b = e.id), 0)::bigint as remaining_ore,
               (c.id is not null) as credited
        from invoice i
        join party p on p.id = i.party_id
        join entry e on e.id = i.receivable_entry_id
        -- A join, not EXISTS: credits_invoice_id has no index, and a
        -- per-row subquery would scan the invoices once per invoice. An
        -- invoice is credited at most once (credit_invoice refuses a
        -- second), so the join never multiplies rows.
        left join invoice c on c.credits_invoice_id = i.id
        where i.company_id = $1
          and ($3::text is null or p.party_no = $3)
          and ($2::text is null
               or i.invoice_no::text = $2
               or i.kid = $2
               -- position(), not ILIKE: % and _ in a name are text here.
               or position(lower($2) in lower(p.name)) > 0)
    ), inv as (
        select base.*,
               case when is_credit_note then 'credit_note'
                    when credited then 'credited'
                    -- Overpaid counts as paid: nothing is owed.
                    when remaining_ore <= 0 then 'paid'
                    when due_date < current_date then 'overdue'
                    else 'open' end as status
        from base
    )";

fn status_matches(filter: Option<&str>) -> Result<&'static str> {
    Ok(match filter {
        None => "true",
        Some("open") => "status in ('open', 'overdue')",
        Some("overdue") => "status = 'overdue'",
        Some("paid") => "status = 'paid'",
        Some("credit") => "status in ('credit_note', 'credited')",
        Some(other) => bail!("ukjent fakturastatus «{other}» (open, overdue, paid, credit)"),
    })
}

pub async fn list_invoices(
    pool: &PgPool,
    company_id: Uuid,
    filter: &InvoiceFilter<'_>,
) -> Result<InvoicePage> {
    let search = filter.search.map(str::trim).filter(|s| !s.is_empty());
    let status_sql = status_matches(filter.status)?;
    let order = if filter.limit.is_some() {
        "invoice_no desc"
    } else {
        "invoice_no"
    };
    // One pass: the CTE is referenced twice, so Postgres materializes it
    // once, and the per-status counts ride along on every row.
    let rows = sqlx::query(&format!(
        "{INVOICE_STATUS_CTE}, counted as (select status, count(*) as n from inv group by status)
         select * from (
             select inv.*, count(*) over () as total,
                    (select array_agg(status) from counted) as count_status,
                    (select array_agg(n) from counted) as count_n
             from inv
             where {status_sql}
         ) page
         where ($6::bigint is null or invoice_no < $6)
         order by {order}
         limit $4 offset $5"
    ))
    .bind(company_id)
    .bind(search)
    .bind(filter.party_no)
    .bind(filter.limit)
    .bind(filter.offset.max(0))
    .bind(filter.before)
    .fetch_all(pool)
    .await?;
    let (total, counts) = match rows.first() {
        Some(r) => {
            let names: Vec<String> = r
                .get::<Option<Vec<String>>, _>("count_status")
                .unwrap_or_default();
            let ns: Vec<i64> = r.get::<Option<Vec<i64>>, _>("count_n").unwrap_or_default();
            (r.get("total"), names.into_iter().zip(ns).collect())
        }
        // An empty page (no match, or an offset past the end) still owes
        // the caller true totals and counts.
        None => {
            let r = sqlx::query(&format!(
                "{INVOICE_STATUS_CTE}
                 select (select count(*) from inv where {status_sql}) as total,
                        array_agg(status) as count_status, array_agg(n) as count_n
                 from (select status, count(*) as n from inv group by status) x"
            ))
            .bind(company_id)
            .bind(search)
            .bind(filter.party_no)
            .fetch_one(pool)
            .await?;
            let names: Vec<String> = r
                .get::<Option<Vec<String>>, _>("count_status")
                .unwrap_or_default();
            let ns: Vec<i64> = r.get::<Option<Vec<i64>>, _>("count_n").unwrap_or_default();
            (r.get("total"), names.into_iter().zip(ns).collect())
        }
    };
    Ok(InvoicePage {
        rows: rows
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
                status: r.get("status"),
            })
            .collect(),
        total,
        counts,
    })
}

#[derive(Debug)]
pub struct InvoiceLineRow {
    pub line_no: i32,
    pub description: String,
    pub quantity_milli: i64,
    pub unit_price_ore: i64,
    pub net_ore: i64,
    pub vat_code: Option<String>,
    pub vat_ore: i64,
    pub department: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug)]
pub struct InvoiceDetail {
    pub row: InvoiceRow,
    pub delivery_date: Option<NaiveDate>,
    pub delivery_place: Option<String>,
    pub buyer_reference: Option<String>,
    pub currency: Option<String>,
    pub party_email: Option<String>,
    /// The invoice a kreditnota credits, or the kreditnota that credits
    /// this invoice: (id, number).
    pub credits: Option<(Uuid, i64)>,
    pub credited_by: Option<(Uuid, i64)>,
    pub created_by: String,
    pub lines: Vec<InvoiceLineRow>,
    /// The voucher the invoice was posted on, and its `year-number`.
    pub voucher_id: Uuid,
    pub voucher_label: String,
    /// What has settled the receivable: payments and credit notes, each
    /// with the voucher it was posted on (bokføringsloven §4 nr. 7).
    pub settlements: Vec<InvoiceSettlement>,
}

#[derive(Debug, Clone)]
pub struct InvoiceSettlement {
    pub voucher_id: Uuid,
    pub voucher_label: String,
    pub date: NaiveDate,
    pub description: String,
    pub amount_ore: i64,
    pub matched_by: String,
}

/// One invoice with its lines, for the portal's invoice page. None when
/// the id is not this company's invoice.
pub async fn get_invoice(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<InvoiceDetail>> {
    let Some(r) = sqlx::query(&format!(
        "{INVOICE_STATUS_CTE}
         select inv.*, i.delivery_date, i.delivery_place, i.buyer_reference, i.valuta,
                i.created_by, p.email,
                o.id as credits_id, o.invoice_no as credits_no,
                c.id as credited_by_id, c.invoice_no as credited_by_no,
                i.voucher_id, iv.fiscal_year as voucher_year, iv.voucher_number
         from inv
         join invoice i on i.id = inv.id
         join voucher iv on iv.id = i.voucher_id
         join party p on p.id = i.party_id
         left join invoice o on o.id = i.credits_invoice_id
         left join invoice c on c.credits_invoice_id = i.id
         where inv.id = $4"
    ))
    .bind(company_id)
    .bind(None::<&str>)
    .bind(None::<&str>)
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let lines = sqlx::query(
        "select line_no, description, quantity_milli, unit_price_ore, net_ore, vat_code,
                vat_ore, avdeling, prosjekt
         from invoice_line where invoice_id = $1 order by line_no",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|l| InvoiceLineRow {
        line_no: l.get("line_no"),
        description: l.get("description"),
        quantity_milli: l.get("quantity_milli"),
        unit_price_ore: l.get("unit_price_ore"),
        net_ore: l.get("net_ore"),
        vat_code: l.get("vat_code"),
        vat_ore: l.get("vat_ore"),
        department: l.get("avdeling"),
        project: l.get("prosjekt"),
    })
    .collect();
    // The other side of every match on the receivable: for an invoice the
    // payments and credit notes, for a credit note the invoice it offsets.
    let settlements = sqlx::query(
        "select v.id as voucher_id, v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description,
                m.amount_ore, m.matched_by
         from invoice i
         join reskontro_match m on i.receivable_entry_id in (m.entry_a, m.entry_b)
         join entry e on e.id = case when m.entry_a = i.receivable_entry_id
                                     then m.entry_b else m.entry_a end
         join voucher v on v.id = e.voucher_id
         where i.id = $1
         order by v.voucher_date, m.created_at, m.id",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|m| InvoiceSettlement {
        voucher_id: m.get("voucher_id"),
        voucher_label: format!(
            "{}-{}",
            m.get::<i32, _>("fiscal_year"),
            m.get::<i64, _>("voucher_number")
        ),
        date: m.get("voucher_date"),
        description: m.get("description"),
        amount_ore: m.get("amount_ore"),
        matched_by: m.get("matched_by"),
    })
    .collect();
    let pair = |id: &str, no: &str| -> Option<(Uuid, i64)> {
        Some((r.get::<Option<Uuid>, _>(id)?, r.get::<Option<i64>, _>(no)?))
    };
    Ok(Some(InvoiceDetail {
        voucher_id: r.get("voucher_id"),
        voucher_label: format!(
            "{}-{}",
            r.get::<i32, _>("voucher_year"),
            r.get::<i64, _>("voucher_number")
        ),
        settlements,
        credits: pair("credits_id", "credits_no"),
        credited_by: pair("credited_by_id", "credited_by_no"),
        delivery_date: r.get("delivery_date"),
        delivery_place: r.get("delivery_place"),
        buyer_reference: r.get("buyer_reference"),
        currency: r.get("valuta"),
        party_email: r.get("email"),
        created_by: r.get("created_by"),
        row: InvoiceRow {
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
            status: r.get("status"),
        },
        lines,
    }))
}

/// A payment the user records by hand on an issued invoice: cash, Vipps,
/// or a bank payment they would rather not match line by line in Bank.
///
/// Posted exactly like the card rail and the kontantfaktura settle a
/// receivable: one voucher — the money account against the receivable
/// WITH the customer as party — and the reskontro match, in one
/// transaction under the party's lock, so the invoice can never be paid
/// twice by two people at once. Part payments are allowed; paying more
/// than is open is not (an overpayment is a credit balance the user must
/// decide about, not something to post silently).
pub async fn register_invoice_payment(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    payment_date: NaiveDate,
    amount_ore: i64,
    account: &str,
    created_by: &str,
) -> Result<crate::ledger::PostedVoucher> {
    ensure!(amount_ore > 0, "beløpet må være større enn null");
    // Only where money can actually land: kontanter, bank, clearing.
    ensure!(
        account.len() == 4
            && account.starts_with("19")
            && account.bytes().all(|b| b.is_ascii_digit()),
        "innbetalingen må føres på en bank- eller kassekonto (19xx), ikke {account}"
    );
    let today: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(pool)
        .await?;
    ensure!(
        payment_date <= today,
        "innbetalingsdatoen kan ikke være fram i tid"
    );
    if account == "1920" {
        crate::ensure_account(pool, company_id, "1920", "Bankinnskudd").await?;
    }

    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "select i.invoice_no, i.receivable_entry_id, i.valuta, i.credits_invoice_id,
                p.id as party_id, p.party_no, a.number as receivable_account
         from invoice i
         join party p on p.id = i.party_id
         join entry e on e.id = i.receivable_entry_id
         join account a on a.id = e.account_id
         where i.id = $1 and i.company_id = $2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("fakturaen finnes ikke")?;
    let invoice_no: i64 = row.get("invoice_no");
    ensure!(
        row.get::<Option<Uuid>, _>("credits_invoice_id").is_none(),
        "en kreditnota betales ikke inn — den er allerede ført mot fakturaen den krediterer"
    );
    if let Some(currency) = row.get::<Option<String>, _>("valuta") {
        bail!(
            "faktura {invoice_no} er i {currency}; registrer innbetalingen fra Bank, \
             der kursdifferansen bokføres"
        );
    }
    let receivable_entry: Uuid = row.get("receivable_entry_id");
    let party_id: Uuid = row.get("party_id");
    let account_exists: bool = sqlx::query_scalar(
        "select exists (select 1 from account where company_id = $1 and number = $2 and active)",
    )
    .bind(company_id)
    .bind(account)
    .fetch_one(&mut *tx)
    .await?;
    ensure!(
        account_exists,
        "konto {account} finnes ikke eller er deaktivert i kontoplanen"
    );

    // Same lock as every other match on this party (subledger::match_items).
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(party_id)
        .execute(&mut *tx)
        .await?;
    let open = crate::party_items(&mut *tx, company_id, party_id, false)
        .await?
        .into_iter()
        .find(|i| i.entry_id == receivable_entry)
        .map_or(0, |i| i.remaining_ore);
    ensure!(open > 0, "faktura {invoice_no} har ikke noe utestående");
    ensure!(
        amount_ore <= open,
        "faktura {invoice_no} har bare {} kr utestående, så {} kr kan ikke registreres",
        regnmed_core::Ore(open),
        regnmed_core::Ore(amount_ore)
    );

    let party_no: String = row.get("party_no");
    let receivable_account: String = row.get("receivable_account");
    let draft = regnmed_core::voucher::VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: payment_date,
        description: format!("Innbetaling faktura {invoice_no}"),
        reverses: None,
        entries: vec![
            regnmed_core::voucher::EntryDraft {
                account_number: account.to_string(),
                amount: regnmed_core::Ore(amount_ore),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            regnmed_core::voucher::EntryDraft {
                account_number: receivable_account,
                amount: regnmed_core::Ore(-amount_ore),
                vat_code: None,
                description: None,
                party_no: Some(party_no),
                department: None,
                project: None,
                currency: None,
            },
        ],
    };
    // A hand-recorded settlement is a manual voucher: the company's
    // attestation policy governs it like any other (#47).
    crate::chart_of_accounts::refuse_if_attestation_required(&mut tx, company_id, &draft).await?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
    let payment_entry: Uuid =
        sqlx::query_scalar("select id from entry where voucher_id = $1 and party_id is not null")
            .bind(posted.id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query(
        "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::now_v7())
    .bind(receivable_entry)
    .bind(payment_entry)
    .bind(amount_ore)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(posted)
}

/// Kontantfaktura (#89, bokføringsforskriften §5-3): a salgsdokument for
/// a ytelse paid on delivery.
///
/// The receivable ARISES AND IS SETTLED in one transaction. It would be
/// simpler to post the sale straight against bank and skip 1500
/// entirely, and that is exactly what must not happen: the reskontro
/// doctrine says a customer's postings carry a party, and a side door
/// past it would make `reskontro_check` — the revisor's tie-out —
/// quietly incomplete. The customer history stays true, and the open
/// item is closed the instant it exists.
///
/// `oppgjorskonto` is where the money landed: 1900 kontanter, 1920 bank,
/// or the card acquirer's clearing account. The caller names it; we do
/// not guess how someone was paid.
pub async fn create_cash_invoice(
    pool: &PgPool,
    company_id: Uuid,
    draft: &InvoiceDraft,
    settlement_account: &str,
    payment_method: &str,
    created_by: &str,
) -> Result<IssuedInvoice> {
    ensure!(
        !payment_method.trim().is_empty(),
        "kontantfakturaen må si hva den ble gjort opp med (§5-3)"
    );
    // Set here rather than trusted from the caller: a kontantfaktura
    // that reached the PDF as an ordinary faktura would print a KID for
    // money already received.
    let draft = &InvoiceDraft {
        cash_payment_method: Some(payment_method.trim().to_string()),
        ..InvoiceDraft::clone(draft)
    };
    let mut tx = pool.begin().await?;
    let issued = create_invoice_in(pool, &mut tx, company_id, draft, created_by, None).await?;

    let (party_no, receivable_entry, gross): (String, Uuid, i64) = {
        let row = sqlx::query(
            "select p.party_no, i.receivable_entry_id, e.amount_ore::bigint as gross
             from invoice i
             join party p on p.id = i.party_id
             join entry e on e.id = i.receivable_entry_id
             where i.id = $1",
        )
        .bind(issued.invoice_id)
        .fetch_one(&mut *tx)
        .await?;
        (
            row.get("party_no"),
            row.get("receivable_entry_id"),
            row.get("gross"),
        )
    };

    let settlement = regnmed_core::voucher::VoucherDraft {
        journal_code: draft.journal_code.clone(),
        voucher_date: draft.invoice_date,
        description: format!(
            "Kontantsalg, oppgjør faktura {} ({payment_method})",
            issued.invoice_no
        ),
        reverses: None,
        entries: vec![
            regnmed_core::voucher::EntryDraft {
                account_number: settlement_account.to_string(),
                amount: regnmed_core::Ore(gross),
                vat_code: None,
                description: Some(payment_method.trim().to_string()),
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            regnmed_core::voucher::EntryDraft {
                account_number: draft.receivable_account.clone(),
                amount: regnmed_core::Ore(-gross),
                vat_code: None,
                description: None,
                // The party goes on BOTH sides of the customer's history:
                // the claim and its settlement.
                party_no: Some(party_no),
                department: None,
                project: None,
                currency: None,
            },
        ],
    };
    let posted = crate::post_voucher_in(&mut tx, company_id, &settlement, created_by).await?;
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
    Ok(issued)
}
