//! Tilbud → ordre → faktura (docs/faktura.md, #31): the commercial
//! chain before the invoice, outside the ledger. Tilbud are freely
//! editable until akseptert/avslått; an ordre is a frozen confirmation;
//! converting an ordre runs the normal atomic invoice path and links
//! the whole chain for traceability. One ordre → one faktura (no
//! delfakturering in v1).

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::fakturapdf::{Dokumenttype, FakturaPdfInput, PdfLinje, render_faktura_pdf};
use regnmed_core::invoice::compute;
use regnmed_core::mva::rate_on;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::invoice::{InvoiceDraft, InvoiceLineDraft, IssuedInvoice, create_invoice_in};
use crate::mva::load_vat_rates;

pub use crate::invoice_template::TemplateLineDraft as SalgsLineDraft;

fn tilbud_editable(status: &str) -> bool {
    matches!(status, "utkast" | "sendt")
}

async fn next_doc_no(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    kind: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "insert into salgsdokument_counter (company_id, kind, last_number) values ($1, $2, 1)
         on conflict (company_id, kind)
         do update set last_number = salgsdokument_counter.last_number + 1
         returning last_number",
    )
    .bind(company_id)
    .bind(kind)
    .fetch_one(&mut **tx)
    .await?)
}

async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    dokument_id: Uuid,
    lines: &[SalgsLineDraft],
) -> Result<()> {
    ensure!(!lines.is_empty(), "dokumentet trenger minst én linje");
    for (i, line) in lines.iter().enumerate() {
        sqlx::query(
            "insert into salgsdokument_line
                 (id, dokument_id, line_no, description, account_number,
                  quantity_milli, unit_price_ore, vat_code, avdeling, prosjekt)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(Uuid::now_v7())
        .bind(dokument_id)
        .bind((i + 1) as i32)
        .bind(&line.description)
        .bind(&line.account_number)
        .bind(line.quantity_milli)
        .bind(line.unit_price_ore)
        .bind(&line.vat_code)
        .bind(&line.avdeling)
        .bind(&line.prosjekt)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Creates a tilbud (status utkast) or a direct ordre (status
/// bekreftet) with its own gap-free number.
pub async fn create_salgsdokument(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    party_no: &str,
    doc_date: NaiveDate,
    lines: &[SalgsLineDraft],
    created_by: &str,
) -> Result<(Uuid, i64)> {
    ensure!(
        matches!(kind, "tilbud" | "ordre"),
        "kind must be tilbud or ordre"
    );
    let party_id: Uuid = sqlx::query_scalar(
        "select id from party where company_id = $1 and party_no = $2 and kind = 'kunde'",
    )
    .bind(company_id)
    .bind(party_no)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no kunde {party_no}"))?;

    let mut tx = pool.begin().await?;
    let doc_no = next_doc_no(&mut tx, company_id, kind).await?;
    let id = Uuid::now_v7();
    let status = if kind == "tilbud" {
        "utkast"
    } else {
        "bekreftet"
    };
    sqlx::query(
        "insert into salgsdokument (id, company_id, kind, doc_no, party_id, doc_date,
                                    status, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(company_id)
    .bind(kind)
    .bind(doc_no)
    .bind(party_id)
    .bind(doc_date)
    .bind(status)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    insert_lines(&mut tx, id, lines).await?;
    tx.commit().await?;
    Ok((id, doc_no))
}

/// Edits a tilbud while it is editable (utkast/sendt). Lines replace
/// the whole set. Ordrer are frozen confirmations — never edited.
pub async fn update_tilbud(
    pool: &PgPool,
    company_id: Uuid,
    tilbud_id: Uuid,
    doc_date: Option<NaiveDate>,
    lines: Option<&[SalgsLineDraft]>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar(
        "select status from salgsdokument
         where id = $1 and company_id = $2 and kind = 'tilbud' for update",
    )
    .bind(tilbud_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such tilbud")?;
    ensure!(
        tilbud_editable(&status),
        "tilbudet er {status} og kan ikke lenger endres"
    );
    sqlx::query(
        "update salgsdokument set doc_date = coalesce($3, doc_date), updated_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(tilbud_id)
    .bind(company_id)
    .bind(doc_date)
    .execute(&mut *tx)
    .await?;
    if let Some(lines) = lines {
        sqlx::query("delete from salgsdokument_line where dokument_id = $1")
            .bind(tilbud_id)
            .execute(&mut *tx)
            .await?;
        insert_lines(&mut tx, tilbud_id, lines).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// One-way tilbud transitions: utkast → sendt → akseptert | avslått
/// (accepting or rejecting straight from utkast is allowed — the
/// customer said yes across the table).
pub async fn set_tilbud_status(
    pool: &PgPool,
    company_id: Uuid,
    tilbud_id: Uuid,
    new_status: &str,
) -> Result<()> {
    ensure!(
        matches!(new_status, "sendt" | "akseptert" | "avslatt"),
        "status must be sendt, akseptert or avslatt"
    );
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar(
        "select status from salgsdokument
         where id = $1 and company_id = $2 and kind = 'tilbud' for update",
    )
    .bind(tilbud_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such tilbud")?;
    let allowed = match new_status {
        "sendt" => status == "utkast",
        "akseptert" | "avslatt" => tilbud_editable(&status),
        _ => false,
    };
    ensure!(allowed, "kan ikke gå fra {status} til {new_status}");
    sqlx::query(
        "update salgsdokument set status = $3, updated_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(tilbud_id)
    .bind(company_id)
    .bind(new_status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// An accepted tilbud becomes an ordre: lines copied losslessly, chain
/// linked, at most one ordre per tilbud (unique index).
pub async fn tilbud_to_ordre(
    pool: &PgPool,
    company_id: Uuid,
    tilbud_id: Uuid,
    doc_date: NaiveDate,
    created_by: &str,
) -> Result<(Uuid, i64)> {
    let mut tx = pool.begin().await?;
    let tilbud = sqlx::query(
        "select status, party_id from salgsdokument
         where id = $1 and company_id = $2 and kind = 'tilbud' for update",
    )
    .bind(tilbud_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such tilbud")?;
    ensure!(
        tilbud.get::<String, _>("status") == "akseptert",
        "bare aksepterte tilbud blir ordre"
    );

    let doc_no = next_doc_no(&mut tx, company_id, "ordre").await?;
    let ordre_id = Uuid::now_v7();
    sqlx::query(
        "insert into salgsdokument (id, company_id, kind, doc_no, party_id, doc_date,
                                    status, tilbud_id, created_by)
         values ($1, $2, 'ordre', $3, $4, $5, 'bekreftet', $6, $7)",
    )
    .bind(ordre_id)
    .bind(company_id)
    .bind(doc_no)
    .bind(tilbud.get::<Uuid, _>("party_id"))
    .bind(doc_date)
    .bind(tilbud_id)
    .bind(created_by)
    .execute(&mut *tx)
    .await
    .context("dette tilbudet har allerede en ordre?")?;
    sqlx::query(
        "insert into salgsdokument_line
             (id, dokument_id, line_no, description, account_number, quantity_milli,
              unit_price_ore, vat_code, avdeling, prosjekt)
         select gen_random_uuid(), $2, line_no, description, account_number, quantity_milli,
                unit_price_ore, vat_code, avdeling, prosjekt
         from salgsdokument_line where dokument_id = $1",
    )
    .bind(tilbud_id)
    .bind(ordre_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((ordre_id, doc_no))
}

/// A confirmed ordre becomes an invoice through the NORMAL atomic path
/// (gap-free number, KID, posting, PDF) — ordre status and invoice link
/// commit in the same transaction. One ordre → one faktura.
pub async fn ordre_to_invoice(
    pool: &PgPool,
    company_id: Uuid,
    ordre_id: Uuid,
    invoice_date: NaiveDate,
    due_date: NaiveDate,
    created_by: &str,
) -> Result<IssuedInvoice> {
    let mut tx = pool.begin().await?;
    let ordre = sqlx::query(
        "select s.status, p.party_no from salgsdokument s
         join party p on p.id = s.party_id
         where s.id = $1 and s.company_id = $2 and s.kind = 'ordre' for update of s",
    )
    .bind(ordre_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such ordre")?;
    ensure!(
        ordre.get::<String, _>("status") == "bekreftet",
        "ordren er allerede fakturert"
    );

    let lines = load_lines(pool, ordre_id).await?;
    let draft = InvoiceDraft {
        party_no: ordre.get("party_no"),
        invoice_date,
        due_date,
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        lines: lines
            .iter()
            .map(|l| InvoiceLineDraft {
                description: l.description.clone(),
                account_number: l.account_number.clone(),
                quantity_milli: l.quantity_milli,
                unit_price_ore: l.unit_price_ore,
                vat_code: l.vat_code.clone(),
                avdeling: l.avdeling.clone(),
                prosjekt: l.prosjekt.clone(),
            })
            .collect(),
    };
    let issued = create_invoice_in(pool, &mut tx, company_id, &draft, created_by, None).await?;
    sqlx::query(
        "update salgsdokument set status = 'fakturert', invoice_id = $3, updated_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(ordre_id)
    .bind(company_id)
    .bind(issued.invoice_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(issued)
}

async fn load_lines(pool: &PgPool, dokument_id: Uuid) -> Result<Vec<SalgsLineDraft>> {
    let rows = sqlx::query(
        "select description, account_number, quantity_milli, unit_price_ore,
                vat_code, avdeling, prosjekt
         from salgsdokument_line where dokument_id = $1 order by line_no",
    )
    .bind(dokument_id)
    .fetch_all(pool)
    .await?;
    ensure!(!rows.is_empty(), "dokumentet har ingen linjer");
    Ok(rows
        .iter()
        .map(|r| SalgsLineDraft {
            description: r.get("description"),
            account_number: r.get("account_number"),
            quantity_milli: r.get("quantity_milli"),
            unit_price_ore: r.get("unit_price_ore"),
            vat_code: r.get("vat_code"),
            avdeling: r.get("avdeling"),
            prosjekt: r.get("prosjekt"),
        })
        .collect())
}

#[derive(Debug)]
pub struct SalgsdokumentRow {
    pub id: Uuid,
    pub kind: String,
    pub doc_no: i64,
    pub party_no: String,
    pub party_name: String,
    pub doc_date: NaiveDate,
    pub status: String,
    pub netto_ore: i64,
    pub tilbud_no: Option<i64>,
    pub invoice_no: Option<i64>,
}

pub async fn list_salgsdokumenter(
    pool: &PgPool,
    company_id: Uuid,
    kind: Option<&str>,
) -> Result<Vec<SalgsdokumentRow>> {
    let rows = sqlx::query(
        "select s.id, s.kind, s.doc_no, p.party_no, p.name as party_name, s.doc_date,
                s.status, t.doc_no as tilbud_no, i.invoice_no,
                coalesce((select sum((l.quantity_milli * l.unit_price_ore) / 1000)
                          from salgsdokument_line l where l.dokument_id = s.id), 0)::bigint
                    as netto_ore
         from salgsdokument s
         join party p on p.id = s.party_id
         left join salgsdokument t on t.id = s.tilbud_id
         left join invoice i on i.id = s.invoice_id
         where s.company_id = $1 and ($2::text is null or s.kind = $2)
         order by s.kind, s.doc_no",
    )
    .bind(company_id)
    .bind(kind)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SalgsdokumentRow {
            id: r.get("id"),
            kind: r.get("kind"),
            doc_no: r.get("doc_no"),
            party_no: r.get("party_no"),
            party_name: r.get("party_name"),
            doc_date: r.get("doc_date"),
            status: r.get("status"),
            netto_ore: r.get("netto_ore"),
            tilbud_no: r.get("tilbud_no"),
            invoice_no: r.get("invoice_no"),
        })
        .collect())
}

/// On-demand PDF of a tilbud/ordrebekreftelse — rendered from the
/// current (editable) state, never stored: the invoice, once issued, is
/// the stored document.
pub async fn salgsdokument_pdf(
    pool: &PgPool,
    company_id: Uuid,
    dokument_id: Uuid,
) -> Result<(String, Vec<u8>)> {
    let doc = sqlx::query(
        "select s.kind, s.doc_no, s.doc_date, p.party_no, p.name as party_name,
                p.orgnr as party_orgnr, p.address as party_address,
                c.name as company_name, c.orgnr, c.address, c.orgform
         from salgsdokument s
         join party p on p.id = s.party_id
         join company c on c.id = s.company_id
         where s.id = $1 and s.company_id = $2",
    )
    .bind(dokument_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such dokument")?;
    let kind: String = doc.get("kind");
    let doc_date: NaiveDate = doc.get("doc_date");

    let rates = load_vat_rates(pool).await?;
    let lines = load_lines(pool, dokument_id).await?;
    let mut pdf_linjer = Vec::with_capacity(lines.len());
    let mut inputs = Vec::with_capacity(lines.len());
    for line in &lines {
        let rate_bp = match &line.vat_code {
            Some(code) => {
                let rate_class: String =
                    sqlx::query_scalar("select rate_class from vat_code where code = $1")
                        .bind(code)
                        .fetch_optional(pool)
                        .await?
                        .with_context(|| format!("unknown vat code {code}"))?;
                rate_on(&rates, &rate_class, doc_date)
                    .with_context(|| format!("no rate for {doc_date}"))?
            }
            None => 0,
        };
        inputs.push(regnmed_core::invoice::InvoiceLineInput {
            description: line.description.clone(),
            account_number: line.account_number.clone(),
            quantity_milli: line.quantity_milli,
            unit_price_ore: line.unit_price_ore,
            vat_code: line.vat_code.clone(),
            rate_bp,
            avdeling: None,
            prosjekt: None,
        });
    }
    let computed = compute(&inputs);
    for (line, amounts) in inputs.iter().zip(&computed.lines) {
        pdf_linjer.push(PdfLinje {
            beskrivelse: line.description.clone(),
            antall_milli: line.quantity_milli,
            enhetspris_ore: line.unit_price_ore,
            mva_sats_bp: line.vat_code.as_ref().map(|_| line.rate_bp),
            netto_ore: amounts.net_ore,
            mva_ore: amounts.vat_ore,
        });
    }

    let orgform: Option<String> = doc.get("orgform");
    let dokumenttype = if kind == "tilbud" {
        Dokumenttype::Tilbud
    } else {
        Dokumenttype::Ordrebekreftelse
    };
    let pdf = render_faktura_pdf(&FakturaPdfInput {
        dokumenttype,
        krediterer_nr: None,
        selger_navn: doc.get("company_name"),
        selger_orgnr: doc.get("orgnr"),
        selger_adresse: doc.get("address"),
        selger_mva_registrert: computed.vat_ore != 0,
        selger_foretaksregistrert: matches!(orgform.as_deref(), Some("AS") | Some("ASA")),
        selger_kontonummer: None,
        kjoper_navn: doc.get("party_name"),
        kjoper_nr: doc.get("party_no"),
        kjoper_orgnr: doc.get("party_orgnr"),
        kjoper_adresse: doc.get("party_address"),
        fakturanr: doc.get("doc_no"),
        fakturadato: doc_date,
        forfallsdato: doc_date,
        kid: String::new(),
        linjer: pdf_linjer,
    });
    let filename = format!("{kind}-{}.pdf", doc.get::<i64, _>("doc_no"));
    Ok((filename, pdf))
}

/// Guard used by tests: an avslått or fakturert dokument never moves
/// again except ordre→fakturert via ordre_to_invoice.
pub async fn salgsdokument_status(
    pool: &PgPool,
    company_id: Uuid,
    dokument_id: Uuid,
) -> Result<String> {
    let status: Option<String> =
        sqlx::query_scalar("select status from salgsdokument where id = $1 and company_id = $2")
            .bind(dokument_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    match status {
        Some(status) => Ok(status),
        None => bail!("no such dokument"),
    }
}
