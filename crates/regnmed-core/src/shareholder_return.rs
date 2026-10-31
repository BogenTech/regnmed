//! The aksjonærregisteroppgave RF-1086 (docs/aksjonaer.md, #43) —
//! rendered hand-rolled and deterministically, like SAF-T, the
//! mva-melding, pain.001 and EHF.
//!
//! The oppgave is filed in two parts, and the API takes them separately:
//! the **main form** (RF-1086) with the company's figures, and one
//! **sub-form** (RF-1086-U) per shareholder. The format is Altinn's
//! `Skjema` dialect: every group carries its `gruppeid`, every field its
//! `orid`, and the order is the XSD's own sequence — anything else is
//! rejected. Both forms are validated against Skatteetaten's official
//! XSDs (vendored in docs/aksjonaer/) in tests and in CI.
//!
//! Amounts are integer øre right up to the closing formatting; no float
//! is ever involved. Dates are `xs:dateTime` at midnight, the way the
//! agency's own example writes them.
//!
//! **The honest limitation** is the transaction type codes: see
//! [`crate::share_register::Transaksjonstype::kode`]. We do not render a code
//! we have not verified against an official source — [`render_underskjema`]
//! fails loudly instead.

use chrono::NaiveDate;

use crate::share_register::{FormItem, SHARE_CLASS_ORDINARY, TransactionType};

/// Skatteetatens etatid, fast i begge skjemaene.
const ETATID: &str = "974761076";

#[derive(Debug, PartialEq, Eq)]
pub enum ReturnError {
    /// A movement whose RF-1086 code we have not verified. Filing it
    /// with a guessed code would corrupt the shareholder's own
    /// aksjeoppgave, so we refuse.
    UnverifiedCode(TransactionType),
    /// Neither fødselsnummer, organisasjonsnummer nor utenlandsk
    /// aksjonær-ID — the oppgave has no way to name this shareholder.
    ManglerIdentitet(String),
}

impl std::fmt::Display for ReturnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnverifiedCode(t) => write!(
                f,
                "transaksjonstypen «{}» kan ikke leveres: RF-1086-koden for den er ikke \
                 publisert i XSD-en eller rettledningen, og regnmed gjetter den ikke \
                 (se docs/aksjonaer.md)",
                t.name()
            ),
            Self::ManglerIdentitet(name) => write!(
                f,
                "aksjonæren «{name}» mangler fødselsnummer, organisasjonsnummer og \
                 utenlandsk aksjonær-ID — oppgaven kan ikke identifisere eieren"
            ),
        }
    }
}

impl std::error::Error for ReturnError {}

#[derive(Debug, Clone)]
pub struct Company {
    pub orgnumber: String,
    pub name: String,
    pub address: Option<String>,
    pub postal_code: Option<String>,
    pub postal_city: Option<String>,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
}

/// A pair of "last year / this year" figures, which is how nearly every
/// number in the hovedskjema is reported.
#[derive(Debug, Clone, Copy, Default)]
pub struct PriorYear {
    pub last_year: i64,
    pub this_year: i64,
}

/// One utbytte decision during the year (generalforsamlingens vedtak).
#[derive(Debug, Clone)]
pub struct Distribution {
    pub date: NaiveDate,
    pub per_share_ore: i64,
    pub total_ore: i64,
}

/// Shares issued during the year, reported at company level.
#[derive(Debug, Clone)]
pub struct NewIssue {
    pub date: NaiveDate,
    pub type_: TransactionType,
    pub new_shares: i64,
    /// Total share count after the event.
    pub shares_after: i64,
    pub nominal_value_ore: i64,
    pub overkurs_ore: i64,
}

#[derive(Debug, Clone)]
pub struct MainForm {
    pub company: Company,
    pub inntektsar: i32,
    pub share_capital: PriorYear,
    pub nominal_value_ore: PriorYear,
    pub share_count: PriorYear,
    pub paid_in_share_capital: PriorYear,
    pub overkurs: PriorYear,
    pub dividend: Vec<Distribution>,
    pub new_issues: Vec<NewIssue>,
}

#[derive(Debug, Clone)]
pub enum ShareholderId {
    Nin(String),
    Organisasjonsnummer(String),
    /// `UTLxxxxxxxxx`, assigned by Aksjonærregisteret to shareholders
    /// with no Norwegian identifier.
    Foreign(String),
}

#[derive(Debug, Clone)]
pub struct DividendEntry {
    pub date: NaiveDate,
    pub amount_ore: i64,
    pub share_count: i64,
}

/// One movement on one shareholder, as the underskjema reports it.
#[derive(Debug, Clone)]
pub struct ShareMovement {
    pub date: NaiveDate,
    pub type_: TransactionType,
    pub quantity: i64,
    /// Acquisition value (anskaffelsesverdi) on an acquisition, or the
    /// consideration (vederlag) on a disposal, when known.
    pub amount_ore: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SubForm {
    pub orgnumber: String,
    pub inntektsar: i32,
    pub id: Option<ShareholderId>,
    pub name: String,
    pub address: Option<String>,
    pub postal_code: Option<String>,
    pub postal_city: Option<String>,
    pub country_code: Option<String>,
    pub share_count: PriorYear,
    pub dividend: Vec<DividendEntry>,
    pub movements: Vec<ShareMovement>,
}

// ---------------------------------------------------------------- writer

struct Writer {
    out: String,
    depth: usize,
}

impl Writer {
    fn new() -> Self {
        Writer {
            out: String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"),
            depth: 0,
        }
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
    }

    fn escape(&mut self, value: &str) {
        for c in value.chars() {
            match c {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                '"' => self.out.push_str("&quot;"),
                _ => self.out.push(c),
            }
        }
    }

    fn open_attr(&mut self, tag: &str, attrs: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        if !attrs.is_empty() {
            self.out.push(' ');
            self.out.push_str(attrs);
        }
        self.out.push_str(">\n");
        self.depth += 1;
    }

    /// Opens an Altinn group, which always carries its own gruppeid.
    fn group(&mut self, tag: &str, gruppeid: u32) {
        self.open_attr(tag, &format!("gruppeid=\"{gruppeid}\""));
    }

    fn close(&mut self, tag: &str) {
        self.depth -= 1;
        self.indent();
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    /// A leaf field, which always carries its own orid.
    fn felt(&mut self, tag: &str, orid: u32, value: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push_str(&format!(" orid=\"{orid}\">"));
        self.escape(value);
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    fn felt_opt(&mut self, tag: &str, orid: u32, value: Option<&String>) {
        if let Some(v) = value.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            self.felt(tag, orid, v);
        }
    }
}

/// Øre as the decimal the XSD patterns accept (`[0-9]+(\.[0-9]{1,2})?`).
/// Always two decimals so output is byte-identical for equal input.
fn amount(ore: i64) -> String {
    format!("{}.{:02}", ore / 100, (ore % 100).abs())
}

/// `xs:dateTime` at midnight, matching Skatteetatens own example.
fn timestamp(date: NaiveDate) -> String {
    format!("{}T00:00:00", date.format("%Y-%m-%d"))
}

/// Truncates to the field's maximum length on a char boundary.
///
/// The oppgave caps AksjonarNavn at 35 characters. A long company name
/// must not block a filing that is otherwise correct, so it is shortened
/// rather than refused — the identity comes from the orgnr, not the name.
fn kutt(text: &str, maks: usize) -> String {
    text.chars().take(maks).collect()
}

// ------------------------------------------------------------ hovedskjema

pub fn render_main_form(h: &MainForm) -> Result<String, ReturnError> {
    for n in &h.new_issues {
        if n.type_.code().is_none() {
            return Err(ReturnError::UnverifiedCode(n.type_));
        }
    }

    let mut w = Writer::new();
    w.open_attr(
        "Skjema",
        &format!(
            "skjemanummer=\"890\" spesifikasjonsnummer=\"12144\" blankettnummer=\"RF-1086\" \
             gruppeid=\"2586\" etatid=\"{ETATID}\""
        ),
    );

    w.group("GenerellInformasjon-grp-2587", 2587);
    w.group("Selskap-grp-2588", 2588);
    w.felt(
        "EnhetOrganisasjonsnummer-datadef-18",
        18,
        &h.company.orgnumber,
    );
    w.felt("EnhetNavn-datadef-1", 1, &kutt(&h.company.name, 175));
    w.felt_opt("EnhetAdresse-datadef-15", 15, h.company.address.as_ref());
    w.felt_opt(
        "EnhetPostnummer-datadef-6673",
        6673,
        h.company.postal_code.as_ref(),
    );
    w.felt_opt(
        "EnhetPoststed-datadef-6674",
        6674,
        h.company.postal_city.as_ref(),
    );
    w.felt("AksjeType-datadef-17659", 17659, SHARE_CLASS_ORDINARY);
    w.felt("Inntektsar-datadef-692", 692, &h.inntektsar.to_string());
    w.close("Selskap-grp-2588");
    if h.company.contact_name.is_some() || h.company.contact_email.is_some() {
        w.group("Kontaktperson-grp-3442", 3442);
        w.felt_opt(
            "KontaktpersonSkjemaNavn-datadef-33918",
            33918,
            h.company.contact_name.as_ref(),
        );
        w.felt_opt(
            "KontaktpersonSkjemaEPost-datadef-30533",
            30533,
            h.company.contact_email.as_ref(),
        );
        w.close("Kontaktperson-grp-3442");
    }
    w.close("GenerellInformasjon-grp-2587");

    w.group("Selskapsopplysninger-grp-2589", 2589);
    w.group("AksjekapitalForHeleSelskapet-grp-3443", 3443);
    w.felt(
        "AksjekapitalFjoraret-datadef-7129",
        7129,
        &amount(h.share_capital.last_year),
    );
    w.felt(
        "Aksjekapital-datadef-87",
        87,
        &amount(h.share_capital.this_year),
    );
    w.close("AksjekapitalForHeleSelskapet-grp-3443");
    // One share class in v1: the class's figures are the company's.
    w.group("AksjekapitalIDenneAksjeklassen-grp-3444", 3444);
    w.felt(
        "AksjekapitalISINAksjetypeFjoraret-datadef-17663",
        17663,
        &amount(h.share_capital.last_year),
    );
    w.felt(
        "AksjekapitalISINAksjetype-datadef-17664",
        17664,
        &amount(h.share_capital.this_year),
    );
    w.close("AksjekapitalIDenneAksjeklassen-grp-3444");
    w.group("PalydendePerAksje-grp-3447", 3447);
    w.felt(
        "AksjeMvPalydendeFjoraret-datadef-23944",
        23944,
        &amount(h.nominal_value_ore.last_year),
    );
    w.felt(
        "AksjeMvPalydende-datadef-23945",
        23945,
        &amount(h.nominal_value_ore.this_year),
    );
    w.close("PalydendePerAksje-grp-3447");
    w.group("AntallAksjerIDenneAksjeklassen-grp-3445", 3445);
    w.felt(
        "AksjerMvAntallFjoraret-datadef-29166",
        29166,
        &h.share_count.last_year.to_string(),
    );
    w.felt(
        "AksjerMvAntall-datadef-29167",
        29167,
        &h.share_count.this_year.to_string(),
    );
    w.close("AntallAksjerIDenneAksjeklassen-grp-3445");
    w.group("InnbetaltAksjekapitalIDenneAksjeklassen-grp-3446", 3446);
    w.felt(
        "AksjekapitalInnbetaltFjoraret-datadef-8020",
        8020,
        &amount(h.paid_in_share_capital.last_year),
    );
    w.felt(
        "AksjekapitalInnbetalt-datadef-5867",
        5867,
        &amount(h.paid_in_share_capital.this_year),
    );
    w.close("InnbetaltAksjekapitalIDenneAksjeklassen-grp-3446");
    w.group("InnbetaltOverkursIDenneAksjeklassen-grp-3448", 3448);
    w.felt(
        "AksjeOverkursISINAksjetypeFjoraret-datadef-17662",
        17662,
        &amount(h.overkurs.last_year),
    );
    w.felt(
        "AksjeOverkursISINAksjetype-datadef-17661",
        17661,
        &amount(h.overkurs.this_year),
    );
    w.close("InnbetaltOverkursIDenneAksjeklassen-grp-3448");
    w.close("Selskapsopplysninger-grp-2589");

    if !h.dividend.is_empty() {
        w.group("Utbytte-grp-3449", 3449);
        for u in &h.dividend {
            w.group(
                "UtdeltSkatterettsligUtbytteILopetAvInntektsaret-grp-3451",
                3451,
            );
            w.felt(
                "AksjeUtbytteISINAksjetype-datadef-17665",
                17665,
                &amount(u.total_ore),
            );
            w.felt(
                "AksjeUtbyttePerAksje-datadef-23946",
                23946,
                &amount(u.per_share_ore),
            );
            w.felt(
                "AksjeUtbytteTidspunkt-datadef-17667",
                17667,
                &timestamp(u.date),
            );
            w.close("UtdeltSkatterettsligUtbytteILopetAvInntektsaret-grp-3451");
        }
        w.close("Utbytte-grp-3449");
    }

    if !h.new_issues.is_empty() {
        w.group("UtstedelseAvAksjerIfmStiftelseNyemisjonMv-grp-3452", 3452);
        for n in &h.new_issues {
            let code = n.type_.code().ok_or(ReturnError::UnverifiedCode(n.type_))?;
            w.group("AntallNyutstedteAksjer-grp-3453", 3453);
            w.felt(
                "AksjerNyutstedteStiftelseMvAntall-datadef-17668",
                17668,
                &n.new_shares.to_string(),
            );
            w.felt(
                "AksjerStiftelseMvAntall-datadef-17669",
                17669,
                &n.shares_after.to_string(),
            );
            w.felt("AksjerNyutstedteStiftelseMvType-datadef-17670", 17670, code);
            w.felt(
                "AksjerNyutstedteStiftelseMvTidspunkt-datadef-17671",
                17671,
                &timestamp(n.date),
            );
            w.felt(
                "AksjerNyutstedteStiftelseMvPalydende-datadef-23947",
                23947,
                &amount(n.nominal_value_ore),
            );
            w.felt(
                "AksjerNyutstedteStiftelseMvOverkurs-datadef-23948",
                23948,
                &amount(n.overkurs_ore),
            );
            w.close("AntallNyutstedteAksjer-grp-3453");
        }
        w.close("UtstedelseAvAksjerIfmStiftelseNyemisjonMv-grp-3452");
    }

    w.close("Skjema");
    Ok(w.out)
}

// ----------------------------------------------------------- underskjema

pub fn render_sub_form(u: &SubForm) -> Result<String, ReturnError> {
    let Some(id) = &u.id else {
        return Err(ReturnError::ManglerIdentitet(u.name.clone()));
    };
    for b in &u.movements {
        if b.type_.code().is_none() {
            return Err(ReturnError::UnverifiedCode(b.type_));
        }
    }

    let mut w = Writer::new();
    w.open_attr(
        "Skjema",
        &format!(
            "skjemanummer=\"923\" spesifikasjonsnummer=\"12232\" blankettnummer=\"RF-1086-U\" \
             gruppeid=\"3983\" etatid=\"{ETATID}\""
        ),
    );

    w.group("SelskapsOgAksjonaropplysninger-grp-3987", 3987);
    w.group("Selskapsidentifikasjon-grp-3986", 3986);
    w.felt("EnhetOrganisasjonsnummer-datadef-18", 18, &u.orgnumber);
    w.felt("AksjeType-datadef-17659", 17659, SHARE_CLASS_ORDINARY);
    w.felt("Inntektsar-datadef-692", 692, &u.inntektsar.to_string());
    w.close("Selskapsidentifikasjon-grp-3986");
    w.group("NorskUtenlandskAksjonar-grp-3988", 3988);
    match id {
        ShareholderId::Nin(f) => w.felt("AksjonarFodselsnummer-datadef-1156", 1156, f),
        ShareholderId::Organisasjonsnummer(o) => {
            w.felt("AksjonarOrganisasjonsnummer-datadef-7597", 7597, o)
        }
        ShareholderId::Foreign(x) => w.felt(
            "AksjonarUtenlandskIdenifikasjonsnummer-datadef-26626",
            26626,
            x,
        ),
    }
    w.felt("AksjonarNavn-datadef-1153", 1153, &kutt(&u.name, 35));
    w.felt_opt(
        "AksjonarPostnummer-datadef-7598",
        7598,
        u.postal_code.as_ref(),
    );
    w.felt_opt(
        "AksjonarPoststed-datadef-7599",
        7599,
        u.postal_city.as_ref(),
    );
    w.felt_opt(
        "AksjonarLandkode-datadef-17740",
        17740,
        u.country_code.as_ref(),
    );
    if let Some(address) = u.address.as_ref().filter(|a| !a.trim().is_empty()) {
        w.group("Adresse-grp-7722", 7722);
        w.felt("AksjonarAdresse-datadef-1154", 1154, &kutt(address, 105));
        w.close("Adresse-grp-7722");
    }
    w.close("NorskUtenlandskAksjonar-grp-3988");
    w.close("SelskapsOgAksjonaropplysninger-grp-3987");

    w.group(
        "AntallAksjerUtbytteOgTilbakebetalingAvTidligereInnbetaltKapit-grp-3990",
        3990,
    );
    w.group("AntallAksjerPerAksjonar-grp-3989", 3989);
    w.felt(
        "AksjerAntallFjoraret-datadef-29168",
        29168,
        &u.share_count.last_year.to_string(),
    );
    w.felt(
        "AksjonarAksjerAntall-datadef-17741",
        17741,
        &u.share_count.this_year.to_string(),
    );
    w.close("AntallAksjerPerAksjonar-grp-3989");
    for post in &u.dividend {
        w.group("UtdeltUtbyttePerAksjonar-grp-3991", 3991);
        w.felt(
            "Aksjeutbytte-datadef-29169",
            29169,
            &amount(post.amount_ore),
        );
        w.felt(
            "AksjerUtbytteAntall-datadef-17742",
            17742,
            &post.share_count.to_string(),
        );
        w.felt(
            "AksjerUtbytteTidspunkt-datadef-17769",
            17769,
            &timestamp(post.date),
        );
        w.close("UtdeltUtbyttePerAksjonar-grp-3991");
    }
    w.close("AntallAksjerUtbytteOgTilbakebetalingAvTidligereInnbetaltKapit-grp-3990");

    let acquisitions: Vec<_> = u
        .movements
        .iter()
        .filter(|b| b.type_.post() == FormItem::Acquisition)
        .collect();
    if !acquisitions.is_empty() {
        w.group("Transaksjoner-grp-3992", 3992);
        w.group("KjopArvGaveStiftelseNyemisjonMv-grp-3993", 3993);
        for b in acquisitions {
            let code = b.type_.code().ok_or(ReturnError::UnverifiedCode(b.type_))?;
            w.group("AntallAksjerITilgang-grp-3998", 3998);
            w.felt(
                "AksjerKjopAntall-datadef-12153",
                12153,
                &b.quantity.to_string(),
            );
            w.felt("AksjeErvervType-datadef-17745", 17745, code);
            w.felt("AksjerErvervsdato-datadef-17746", 17746, &timestamp(b.date));
            if let Some(ore) = b.amount_ore {
                w.felt("AksjeAnskaffelsesverdi-datadef-17636", 17636, &amount(ore));
            }
            w.close("AntallAksjerITilgang-grp-3998");
        }
        w.close("KjopArvGaveStiftelseNyemisjonMv-grp-3993");
        w.close("Transaksjoner-grp-3992");
    }

    let disposals: Vec<_> = u
        .movements
        .iter()
        .filter(|b| b.type_.post() == FormItem::Disposal)
        .collect();
    if !disposals.is_empty() {
        w.group("SalgArvGaveLikvidasjonPartiellLikvidasjonMv-grp-3995", 3995);
        for b in disposals {
            let code = b.type_.code().ok_or(ReturnError::UnverifiedCode(b.type_))?;
            w.group("AksjerIAvgang-grp-4002", 4002);
            w.felt(
                "AksjerArvMvOmsattAntall-datadef-17752",
                17752,
                &b.quantity.to_string(),
            );
            w.felt("AksjerArvMvOmsattType-datadef-17753", 17753, code);
            w.felt(
                "AksjerArvMvOmsattTidspunkt-datadef-17754",
                17754,
                &timestamp(b.date),
            );
            if let Some(ore) = b.amount_ore {
                w.felt("AksjerArvMvOmsatt-datadef-17755", 17755, &amount(ore));
            }
            w.close("AksjerIAvgang-grp-4002");
        }
        w.close("SalgArvGaveLikvidasjonPartiellLikvidasjonMv-grp-3995");
    }

    w.close("Skjema");
    Ok(w.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn company() -> Company {
        Company {
            orgnumber: "314259521".into(),
            name: "Mosegrodd Oransje Tiger AS".into(),
            address: Some("Haråsveien 13E".into()),
            postal_code: Some("0283".into()),
            postal_city: Some("OSLO".into()),
            contact_name: Some("Kari Styreleder".into()),
            contact_email: Some("kari@example.no".into()),
        }
    }

    fn main_form() -> MainForm {
        MainForm {
            company: company(),
            inntektsar: 2026,
            share_capital: PriorYear {
                last_year: 0,
                this_year: 10_000_000,
            },
            nominal_value_ore: PriorYear {
                last_year: 0,
                this_year: 100_000,
            },
            share_count: PriorYear {
                last_year: 0,
                this_year: 100,
            },
            paid_in_share_capital: PriorYear {
                last_year: 0,
                this_year: 10_000_000,
            },
            overkurs: PriorYear::default(),
            dividend: vec![Distribution {
                date: d(2026, 5, 20),
                per_share_ore: 50_000,
                total_ore: 5_000_000,
            }],
            new_issues: vec![NewIssue {
                date: d(2026, 1, 2),
                type_: TransactionType::Stiftelse,
                new_shares: 100,
                shares_after: 100,
                nominal_value_ore: 100_000,
                overkurs_ore: 0,
            }],
        }
    }

    fn sub_form() -> SubForm {
        SubForm {
            orgnumber: "314259521".into(),
            inntektsar: 2026,
            id: Some(ShareholderId::Nin("26829398612".into())),
            name: "Kari Nordmann".into(),
            address: Some("Haråsveien 13E".into()),
            postal_code: Some("0283".into()),
            postal_city: Some("OSLO".into()),
            country_code: None,
            share_count: PriorYear {
                last_year: 0,
                this_year: 100,
            },
            dividend: vec![DividendEntry {
                date: d(2026, 5, 20),
                amount_ore: 5_000_000,
                share_count: 100,
            }],
            movements: vec![ShareMovement {
                date: d(2026, 1, 2),
                type_: TransactionType::Stiftelse,
                quantity: 100,
                amount_ore: Some(10_000_000),
            }],
        }
    }

    /// Runs xmllint against a vendored XSD, skipping when it is absent.
    fn valider(xml: &str, xsd_name: &str, tag: &str) {
        let xsd = format!(
            "{}/../../docs/aksjonaer/{xsd_name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let dir = std::env::temp_dir().join("regnmed-aksjonaer-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{tag}.xml"));
        std::fs::write(&file, xml).unwrap();
        let output = match std::process::Command::new("xmllint")
            .args(["--noout", "--schema", &xsd])
            .arg(&file)
            .output()
        {
            Ok(output) => output,
            Err(_) => {
                eprintln!("xmllint ikke installert — hopper over skjemavalidering");
                return;
            }
        };
        assert!(
            output.status.success(),
            "XSD-validering feilet for {tag}:\n{}\n\n{xml}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn the_main_form_validates_against_the_official_xsd() {
        let xml = render_main_form(&main_form()).unwrap();
        valider(&xml, "aksjonaerregisteroppgaveHovedskjema.xsd", "hoved");
    }

    #[test]
    fn the_sub_form_validates_against_the_official_xsd() {
        let xml = render_sub_form(&sub_form()).unwrap();
        valider(&xml, "aksjonaerregisteroppgaveUnderskjema.xsd", "under");
    }

    #[test]
    fn a_year_without_transactions_is_still_a_valid_return() {
        // The common case: the same owners as last year, nothing
        // happened. Then there is no code to be unsure about.
        let mut h = main_form();
        h.new_issues.clear();
        h.dividend.clear();
        valider(
            &render_main_form(&h).unwrap(),
            "aksjonaerregisteroppgaveHovedskjema.xsd",
            "hoved-stille",
        );
        let mut u = sub_form();
        u.movements.clear();
        u.dividend.clear();
        valider(
            &render_sub_form(&u).unwrap(),
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-stille",
        );
    }

    #[test]
    fn a_corporate_shareholder_and_a_foreign_id_both_validate() {
        let mut u = sub_form();
        u.id = Some(ShareholderId::Organisasjonsnummer("923609016".into()));
        u.name = "Equinor ASA".into();
        valider(
            &render_sub_form(&u).unwrap(),
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-selskap",
        );

        let mut u = sub_form();
        u.id = Some(ShareholderId::Foreign("UTL000000123".into()));
        u.country_code = Some("SE".into());
        valider(
            &render_sub_form(&u).unwrap(),
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-utl",
        );
    }

    /// The name field is 35 characters. A long company name should be
    /// truncated, not stop an otherwise correct filing.
    #[test]
    fn a_long_shareholder_name_is_truncated_rather_than_failing() {
        let mut u = sub_form();
        u.id = Some(ShareholderId::Organisasjonsnummer("923609016".into()));
        u.name = "Æ".repeat(60);
        let xml = render_sub_form(&u).unwrap();
        valider(
            &xml,
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-langt-navn",
        );
        assert!(xml.contains(&"Æ".repeat(35)));
        assert!(!xml.contains(&"Æ".repeat(36)));
    }

    /// The core of the honest limitation: we cannot file a sale, because
    /// we do not know its code — and then we say so instead of guessing.
    #[test]
    fn an_unverified_transaction_type_is_refused_loudly() {
        let mut u = sub_form();
        u.movements = vec![ShareMovement {
            date: d(2026, 6, 1),
            type_: TransactionType::Salg,
            quantity: 10,
            amount_ore: Some(1_000_000),
        }];
        let error = render_sub_form(&u).unwrap_err();
        assert_eq!(error, ReturnError::UnverifiedCode(TransactionType::Salg));
        assert!(error.to_string().contains("salg"), "{error}");
        assert!(error.to_string().contains("gjetter"), "{error}");
    }

    #[test]
    fn a_shareholder_without_an_identity_is_refused() {
        let mut u = sub_form();
        u.id = None;
        assert!(matches!(
            render_sub_form(&u).unwrap_err(),
            ReturnError::ManglerIdentitet(_)
        ));
    }

    #[test]
    fn the_rendering_is_deterministic() {
        assert_eq!(
            render_main_form(&main_form()).unwrap(),
            render_main_form(&main_form()).unwrap()
        );
        assert_eq!(
            render_sub_form(&sub_form()).unwrap(),
            render_sub_form(&sub_form()).unwrap()
        );
    }

    #[test]
    fn amounts_are_written_with_two_decimals() {
        assert_eq!(amount(10_000_000), "100000.00");
        assert_eq!(amount(12_345), "123.45");
        assert_eq!(amount(0), "0.00");
        assert_eq!(timestamp(d(2026, 1, 2)), "2026-01-02T00:00:00");
    }
}
