//! Aksjonærregisteroppgaven RF-1086 (docs/aksjonaer.md, #43) — rendret
//! hand-rolled og deterministisk, som SAF-T, mva-meldingen, pain.001 og
//! EHF.
//!
//! Oppgaven leveres i to deler, og API-et tar dem hver for seg:
//! **hovedskjemaet** (RF-1086) med selskapets tall, og ett
//! **underskjema** (RF-1086-U) per aksjonær. Formatet er Altinns
//! `Skjema`-dialekt: hver gruppe bærer sin `gruppeid`, hvert felt sin
//! `orid`, og rekkefølgen er XSD-ens egen sekvens — noe annet avvises.
//! Begge skjemaene valideres mot Skatteetatens offisielle XSD-er
//! (vendored i docs/aksjonaer/) i tester og i CI.
//!
//! Beløp er heltall øre helt fram til den avsluttende formatteringen;
//! ingen flyttall er innom. Datoer er `xs:dateTime` ved midnatt, slik
//! etatens eget eksempel skriver dem.
//!
//! **Den ærlige begrensningen** er transaksjonstypekodene: se
//! [`crate::aksjebok::Transaksjonstype::kode`]. Vi rendrer ikke en kode
//! vi ikke har verifisert mot en offisiell kilde — [`render_underskjema`]
//! feiler høylytt i stedet.

use chrono::NaiveDate;

use crate::aksjebok::{AKSJETYPE_ORDINAERE, Post, Transaksjonstype};

/// Skatteetatens etatid, fast i begge skjemaene.
const ETATID: &str = "974761076";

#[derive(Debug, PartialEq, Eq)]
pub enum OppgaveError {
    /// A movement whose RF-1086 code we have not verified. Filing it
    /// with a guessed code would corrupt the shareholder's own
    /// aksjeoppgave, so we refuse.
    UverifisertKode(Transaksjonstype),
    /// Neither fødselsnummer, organisasjonsnummer nor utenlandsk
    /// aksjonær-ID — the oppgave has no way to name this shareholder.
    ManglerIdentitet(String),
}

impl std::fmt::Display for OppgaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UverifisertKode(t) => write!(
                f,
                "transaksjonstypen «{}» kan ikke leveres: RF-1086-koden for den er ikke \
                 publisert i XSD-en eller rettledningen, og regnmed gjetter den ikke \
                 (se docs/aksjonaer.md)",
                t.navn()
            ),
            Self::ManglerIdentitet(navn) => write!(
                f,
                "aksjonæren «{navn}» mangler fødselsnummer, organisasjonsnummer og \
                 utenlandsk aksjonær-ID — oppgaven kan ikke identifisere eieren"
            ),
        }
    }
}

impl std::error::Error for OppgaveError {}

#[derive(Debug, Clone)]
pub struct Selskap {
    pub orgnr: String,
    pub navn: String,
    pub adresse: Option<String>,
    pub postnummer: Option<String>,
    pub poststed: Option<String>,
    pub kontakt_navn: Option<String>,
    pub kontakt_epost: Option<String>,
}

/// A pair of "last year / this year" figures, which is how nearly every
/// number in the hovedskjema is reported.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fjoraret {
    pub fjoraret: i64,
    pub i_ar: i64,
}

/// One utbytte decision during the year (generalforsamlingens vedtak).
#[derive(Debug, Clone)]
pub struct Utdeling {
    pub dato: NaiveDate,
    pub per_aksje_ore: i64,
    pub totalt_ore: i64,
}

/// Shares issued during the year, reported at company level.
#[derive(Debug, Clone)]
pub struct Nyutstedelse {
    pub dato: NaiveDate,
    pub type_: Transaksjonstype,
    pub antall_nye: i64,
    /// Total share count after the event.
    pub antall_etter: i64,
    pub palydende_ore: i64,
    pub overkurs_ore: i64,
}

#[derive(Debug, Clone)]
pub struct Hovedskjema {
    pub selskap: Selskap,
    pub inntektsar: i32,
    pub aksjekapital: Fjoraret,
    pub palydende_ore: Fjoraret,
    pub antall_aksjer: Fjoraret,
    pub innbetalt_aksjekapital: Fjoraret,
    pub overkurs: Fjoraret,
    pub utbytte: Vec<Utdeling>,
    pub nyutstedelser: Vec<Nyutstedelse>,
}

#[derive(Debug, Clone)]
pub enum Aksjonaerid {
    Fodselsnummer(String),
    Organisasjonsnummer(String),
    /// `UTLxxxxxxxxx`, assigned by Aksjonærregisteret to shareholders
    /// with no Norwegian identifier.
    Utenlandsk(String),
}

#[derive(Debug, Clone)]
pub struct Utbyttepost {
    pub dato: NaiveDate,
    pub belop_ore: i64,
    pub antall_aksjer: i64,
}

/// One movement on one shareholder, as the underskjema reports it.
#[derive(Debug, Clone)]
pub struct Bevegelse {
    pub dato: NaiveDate,
    pub type_: Transaksjonstype,
    pub antall: i64,
    /// Anskaffelsesverdi (tilgang) or vederlag (avgang), when known.
    pub belop_ore: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Underskjema {
    pub orgnr: String,
    pub inntektsar: i32,
    pub id: Option<Aksjonaerid>,
    pub navn: String,
    pub adresse: Option<String>,
    pub postnummer: Option<String>,
    pub poststed: Option<String>,
    pub landkode: Option<String>,
    pub antall_aksjer: Fjoraret,
    pub utbytte: Vec<Utbyttepost>,
    pub bevegelser: Vec<Bevegelse>,
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
    fn gruppe(&mut self, tag: &str, gruppeid: u32) {
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
fn belop(ore: i64) -> String {
    format!("{}.{:02}", ore / 100, (ore % 100).abs())
}

/// `xs:dateTime` at midnight, matching Skatteetatens own example.
fn tidspunkt(dato: NaiveDate) -> String {
    format!("{}T00:00:00", dato.format("%Y-%m-%d"))
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

pub fn render_hovedskjema(h: &Hovedskjema) -> Result<String, OppgaveError> {
    for n in &h.nyutstedelser {
        if n.type_.kode().is_none() {
            return Err(OppgaveError::UverifisertKode(n.type_));
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

    w.gruppe("GenerellInformasjon-grp-2587", 2587);
    w.gruppe("Selskap-grp-2588", 2588);
    w.felt("EnhetOrganisasjonsnummer-datadef-18", 18, &h.selskap.orgnr);
    w.felt("EnhetNavn-datadef-1", 1, &kutt(&h.selskap.navn, 175));
    w.felt_opt("EnhetAdresse-datadef-15", 15, h.selskap.adresse.as_ref());
    w.felt_opt(
        "EnhetPostnummer-datadef-6673",
        6673,
        h.selskap.postnummer.as_ref(),
    );
    w.felt_opt(
        "EnhetPoststed-datadef-6674",
        6674,
        h.selskap.poststed.as_ref(),
    );
    w.felt("AksjeType-datadef-17659", 17659, AKSJETYPE_ORDINAERE);
    w.felt("Inntektsar-datadef-692", 692, &h.inntektsar.to_string());
    w.close("Selskap-grp-2588");
    if h.selskap.kontakt_navn.is_some() || h.selskap.kontakt_epost.is_some() {
        w.gruppe("Kontaktperson-grp-3442", 3442);
        w.felt_opt(
            "KontaktpersonSkjemaNavn-datadef-33918",
            33918,
            h.selskap.kontakt_navn.as_ref(),
        );
        w.felt_opt(
            "KontaktpersonSkjemaEPost-datadef-30533",
            30533,
            h.selskap.kontakt_epost.as_ref(),
        );
        w.close("Kontaktperson-grp-3442");
    }
    w.close("GenerellInformasjon-grp-2587");

    w.gruppe("Selskapsopplysninger-grp-2589", 2589);
    w.gruppe("AksjekapitalForHeleSelskapet-grp-3443", 3443);
    w.felt(
        "AksjekapitalFjoraret-datadef-7129",
        7129,
        &belop(h.aksjekapital.fjoraret),
    );
    w.felt("Aksjekapital-datadef-87", 87, &belop(h.aksjekapital.i_ar));
    w.close("AksjekapitalForHeleSelskapet-grp-3443");
    // Én aksjeklasse i v1: klassens tall er selskapets tall.
    w.gruppe("AksjekapitalIDenneAksjeklassen-grp-3444", 3444);
    w.felt(
        "AksjekapitalISINAksjetypeFjoraret-datadef-17663",
        17663,
        &belop(h.aksjekapital.fjoraret),
    );
    w.felt(
        "AksjekapitalISINAksjetype-datadef-17664",
        17664,
        &belop(h.aksjekapital.i_ar),
    );
    w.close("AksjekapitalIDenneAksjeklassen-grp-3444");
    w.gruppe("PalydendePerAksje-grp-3447", 3447);
    w.felt(
        "AksjeMvPalydendeFjoraret-datadef-23944",
        23944,
        &belop(h.palydende_ore.fjoraret),
    );
    w.felt(
        "AksjeMvPalydende-datadef-23945",
        23945,
        &belop(h.palydende_ore.i_ar),
    );
    w.close("PalydendePerAksje-grp-3447");
    w.gruppe("AntallAksjerIDenneAksjeklassen-grp-3445", 3445);
    w.felt(
        "AksjerMvAntallFjoraret-datadef-29166",
        29166,
        &h.antall_aksjer.fjoraret.to_string(),
    );
    w.felt(
        "AksjerMvAntall-datadef-29167",
        29167,
        &h.antall_aksjer.i_ar.to_string(),
    );
    w.close("AntallAksjerIDenneAksjeklassen-grp-3445");
    w.gruppe("InnbetaltAksjekapitalIDenneAksjeklassen-grp-3446", 3446);
    w.felt(
        "AksjekapitalInnbetaltFjoraret-datadef-8020",
        8020,
        &belop(h.innbetalt_aksjekapital.fjoraret),
    );
    w.felt(
        "AksjekapitalInnbetalt-datadef-5867",
        5867,
        &belop(h.innbetalt_aksjekapital.i_ar),
    );
    w.close("InnbetaltAksjekapitalIDenneAksjeklassen-grp-3446");
    w.gruppe("InnbetaltOverkursIDenneAksjeklassen-grp-3448", 3448);
    w.felt(
        "AksjeOverkursISINAksjetypeFjoraret-datadef-17662",
        17662,
        &belop(h.overkurs.fjoraret),
    );
    w.felt(
        "AksjeOverkursISINAksjetype-datadef-17661",
        17661,
        &belop(h.overkurs.i_ar),
    );
    w.close("InnbetaltOverkursIDenneAksjeklassen-grp-3448");
    w.close("Selskapsopplysninger-grp-2589");

    if !h.utbytte.is_empty() {
        w.gruppe("Utbytte-grp-3449", 3449);
        for u in &h.utbytte {
            w.gruppe(
                "UtdeltSkatterettsligUtbytteILopetAvInntektsaret-grp-3451",
                3451,
            );
            w.felt(
                "AksjeUtbytteISINAksjetype-datadef-17665",
                17665,
                &belop(u.totalt_ore),
            );
            w.felt(
                "AksjeUtbyttePerAksje-datadef-23946",
                23946,
                &belop(u.per_aksje_ore),
            );
            w.felt(
                "AksjeUtbytteTidspunkt-datadef-17667",
                17667,
                &tidspunkt(u.dato),
            );
            w.close("UtdeltSkatterettsligUtbytteILopetAvInntektsaret-grp-3451");
        }
        w.close("Utbytte-grp-3449");
    }

    if !h.nyutstedelser.is_empty() {
        w.gruppe("UtstedelseAvAksjerIfmStiftelseNyemisjonMv-grp-3452", 3452);
        for n in &h.nyutstedelser {
            let kode = n
                .type_
                .kode()
                .ok_or(OppgaveError::UverifisertKode(n.type_))?;
            w.gruppe("AntallNyutstedteAksjer-grp-3453", 3453);
            w.felt(
                "AksjerNyutstedteStiftelseMvAntall-datadef-17668",
                17668,
                &n.antall_nye.to_string(),
            );
            w.felt(
                "AksjerStiftelseMvAntall-datadef-17669",
                17669,
                &n.antall_etter.to_string(),
            );
            w.felt("AksjerNyutstedteStiftelseMvType-datadef-17670", 17670, kode);
            w.felt(
                "AksjerNyutstedteStiftelseMvTidspunkt-datadef-17671",
                17671,
                &tidspunkt(n.dato),
            );
            w.felt(
                "AksjerNyutstedteStiftelseMvPalydende-datadef-23947",
                23947,
                &belop(n.palydende_ore),
            );
            w.felt(
                "AksjerNyutstedteStiftelseMvOverkurs-datadef-23948",
                23948,
                &belop(n.overkurs_ore),
            );
            w.close("AntallNyutstedteAksjer-grp-3453");
        }
        w.close("UtstedelseAvAksjerIfmStiftelseNyemisjonMv-grp-3452");
    }

    w.close("Skjema");
    Ok(w.out)
}

// ----------------------------------------------------------- underskjema

pub fn render_underskjema(u: &Underskjema) -> Result<String, OppgaveError> {
    let Some(id) = &u.id else {
        return Err(OppgaveError::ManglerIdentitet(u.navn.clone()));
    };
    for b in &u.bevegelser {
        if b.type_.kode().is_none() {
            return Err(OppgaveError::UverifisertKode(b.type_));
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

    w.gruppe("SelskapsOgAksjonaropplysninger-grp-3987", 3987);
    w.gruppe("Selskapsidentifikasjon-grp-3986", 3986);
    w.felt("EnhetOrganisasjonsnummer-datadef-18", 18, &u.orgnr);
    w.felt("AksjeType-datadef-17659", 17659, AKSJETYPE_ORDINAERE);
    w.felt("Inntektsar-datadef-692", 692, &u.inntektsar.to_string());
    w.close("Selskapsidentifikasjon-grp-3986");
    w.gruppe("NorskUtenlandskAksjonar-grp-3988", 3988);
    match id {
        Aksjonaerid::Fodselsnummer(f) => w.felt("AksjonarFodselsnummer-datadef-1156", 1156, f),
        Aksjonaerid::Organisasjonsnummer(o) => {
            w.felt("AksjonarOrganisasjonsnummer-datadef-7597", 7597, o)
        }
        Aksjonaerid::Utenlandsk(x) => w.felt(
            "AksjonarUtenlandskIdenifikasjonsnummer-datadef-26626",
            26626,
            x,
        ),
    }
    w.felt("AksjonarNavn-datadef-1153", 1153, &kutt(&u.navn, 35));
    w.felt_opt(
        "AksjonarPostnummer-datadef-7598",
        7598,
        u.postnummer.as_ref(),
    );
    w.felt_opt("AksjonarPoststed-datadef-7599", 7599, u.poststed.as_ref());
    w.felt_opt("AksjonarLandkode-datadef-17740", 17740, u.landkode.as_ref());
    if let Some(adresse) = u.adresse.as_ref().filter(|a| !a.trim().is_empty()) {
        w.gruppe("Adresse-grp-7722", 7722);
        w.felt("AksjonarAdresse-datadef-1154", 1154, &kutt(adresse, 105));
        w.close("Adresse-grp-7722");
    }
    w.close("NorskUtenlandskAksjonar-grp-3988");
    w.close("SelskapsOgAksjonaropplysninger-grp-3987");

    w.gruppe(
        "AntallAksjerUtbytteOgTilbakebetalingAvTidligereInnbetaltKapit-grp-3990",
        3990,
    );
    w.gruppe("AntallAksjerPerAksjonar-grp-3989", 3989);
    w.felt(
        "AksjerAntallFjoraret-datadef-29168",
        29168,
        &u.antall_aksjer.fjoraret.to_string(),
    );
    w.felt(
        "AksjonarAksjerAntall-datadef-17741",
        17741,
        &u.antall_aksjer.i_ar.to_string(),
    );
    w.close("AntallAksjerPerAksjonar-grp-3989");
    for post in &u.utbytte {
        w.gruppe("UtdeltUtbyttePerAksjonar-grp-3991", 3991);
        w.felt("Aksjeutbytte-datadef-29169", 29169, &belop(post.belop_ore));
        w.felt(
            "AksjerUtbytteAntall-datadef-17742",
            17742,
            &post.antall_aksjer.to_string(),
        );
        w.felt(
            "AksjerUtbytteTidspunkt-datadef-17769",
            17769,
            &tidspunkt(post.dato),
        );
        w.close("UtdeltUtbyttePerAksjonar-grp-3991");
    }
    w.close("AntallAksjerUtbytteOgTilbakebetalingAvTidligereInnbetaltKapit-grp-3990");

    let tilgang: Vec<_> = u
        .bevegelser
        .iter()
        .filter(|b| b.type_.post() == Post::Tilgang)
        .collect();
    if !tilgang.is_empty() {
        w.gruppe("Transaksjoner-grp-3992", 3992);
        w.gruppe("KjopArvGaveStiftelseNyemisjonMv-grp-3993", 3993);
        for b in tilgang {
            let kode = b
                .type_
                .kode()
                .ok_or(OppgaveError::UverifisertKode(b.type_))?;
            w.gruppe("AntallAksjerITilgang-grp-3998", 3998);
            w.felt(
                "AksjerKjopAntall-datadef-12153",
                12153,
                &b.antall.to_string(),
            );
            w.felt("AksjeErvervType-datadef-17745", 17745, kode);
            w.felt("AksjerErvervsdato-datadef-17746", 17746, &tidspunkt(b.dato));
            if let Some(ore) = b.belop_ore {
                w.felt("AksjeAnskaffelsesverdi-datadef-17636", 17636, &belop(ore));
            }
            w.close("AntallAksjerITilgang-grp-3998");
        }
        w.close("KjopArvGaveStiftelseNyemisjonMv-grp-3993");
        w.close("Transaksjoner-grp-3992");
    }

    let avgang: Vec<_> = u
        .bevegelser
        .iter()
        .filter(|b| b.type_.post() == Post::Avgang)
        .collect();
    if !avgang.is_empty() {
        w.gruppe("SalgArvGaveLikvidasjonPartiellLikvidasjonMv-grp-3995", 3995);
        for b in avgang {
            let kode = b
                .type_
                .kode()
                .ok_or(OppgaveError::UverifisertKode(b.type_))?;
            w.gruppe("AksjerIAvgang-grp-4002", 4002);
            w.felt(
                "AksjerArvMvOmsattAntall-datadef-17752",
                17752,
                &b.antall.to_string(),
            );
            w.felt("AksjerArvMvOmsattType-datadef-17753", 17753, kode);
            w.felt(
                "AksjerArvMvOmsattTidspunkt-datadef-17754",
                17754,
                &tidspunkt(b.dato),
            );
            if let Some(ore) = b.belop_ore {
                w.felt("AksjerArvMvOmsatt-datadef-17755", 17755, &belop(ore));
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

    fn selskap() -> Selskap {
        Selskap {
            orgnr: "314259521".into(),
            navn: "Mosegrodd Oransje Tiger AS".into(),
            adresse: Some("Haråsveien 13E".into()),
            postnummer: Some("0283".into()),
            poststed: Some("OSLO".into()),
            kontakt_navn: Some("Kari Styreleder".into()),
            kontakt_epost: Some("kari@example.no".into()),
        }
    }

    fn hovedskjema() -> Hovedskjema {
        Hovedskjema {
            selskap: selskap(),
            inntektsar: 2026,
            aksjekapital: Fjoraret {
                fjoraret: 0,
                i_ar: 10_000_000,
            },
            palydende_ore: Fjoraret {
                fjoraret: 0,
                i_ar: 100_000,
            },
            antall_aksjer: Fjoraret {
                fjoraret: 0,
                i_ar: 100,
            },
            innbetalt_aksjekapital: Fjoraret {
                fjoraret: 0,
                i_ar: 10_000_000,
            },
            overkurs: Fjoraret::default(),
            utbytte: vec![Utdeling {
                dato: d(2026, 5, 20),
                per_aksje_ore: 50_000,
                totalt_ore: 5_000_000,
            }],
            nyutstedelser: vec![Nyutstedelse {
                dato: d(2026, 1, 2),
                type_: Transaksjonstype::Stiftelse,
                antall_nye: 100,
                antall_etter: 100,
                palydende_ore: 100_000,
                overkurs_ore: 0,
            }],
        }
    }

    fn underskjema() -> Underskjema {
        Underskjema {
            orgnr: "314259521".into(),
            inntektsar: 2026,
            id: Some(Aksjonaerid::Fodselsnummer("26829398612".into())),
            navn: "Kari Nordmann".into(),
            adresse: Some("Haråsveien 13E".into()),
            postnummer: Some("0283".into()),
            poststed: Some("OSLO".into()),
            landkode: None,
            antall_aksjer: Fjoraret {
                fjoraret: 0,
                i_ar: 100,
            },
            utbytte: vec![Utbyttepost {
                dato: d(2026, 5, 20),
                belop_ore: 5_000_000,
                antall_aksjer: 100,
            }],
            bevegelser: vec![Bevegelse {
                dato: d(2026, 1, 2),
                type_: Transaksjonstype::Stiftelse,
                antall: 100,
                belop_ore: Some(10_000_000),
            }],
        }
    }

    /// Runs xmllint against a vendored XSD, skipping when it is absent.
    fn valider(xml: &str, xsd_navn: &str, tag: &str) {
        let xsd = format!(
            "{}/../../docs/aksjonaer/{xsd_navn}",
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
    fn hovedskjemaet_validerer_mot_offisiell_xsd() {
        let xml = render_hovedskjema(&hovedskjema()).unwrap();
        valider(&xml, "aksjonaerregisteroppgaveHovedskjema.xsd", "hoved");
    }

    #[test]
    fn underskjemaet_validerer_mot_offisiell_xsd() {
        let xml = render_underskjema(&underskjema()).unwrap();
        valider(&xml, "aksjonaerregisteroppgaveUnderskjema.xsd", "under");
    }

    #[test]
    fn et_ar_uten_transaksjoner_er_fortsatt_en_gyldig_oppgave() {
        // Det vanligste tilfellet: samme eiere som i fjor, ingenting
        // skjedde. Da finnes det ingen kode å være usikker på.
        let mut h = hovedskjema();
        h.nyutstedelser.clear();
        h.utbytte.clear();
        valider(
            &render_hovedskjema(&h).unwrap(),
            "aksjonaerregisteroppgaveHovedskjema.xsd",
            "hoved-stille",
        );
        let mut u = underskjema();
        u.bevegelser.clear();
        u.utbytte.clear();
        valider(
            &render_underskjema(&u).unwrap(),
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-stille",
        );
    }

    #[test]
    fn selskapsaksjonaer_og_utenlandsk_id_validerer() {
        let mut u = underskjema();
        u.id = Some(Aksjonaerid::Organisasjonsnummer("923609016".into()));
        u.navn = "Equinor ASA".into();
        valider(
            &render_underskjema(&u).unwrap(),
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-selskap",
        );

        let mut u = underskjema();
        u.id = Some(Aksjonaerid::Utenlandsk("UTL000000123".into()));
        u.landkode = Some("SE".into());
        valider(
            &render_underskjema(&u).unwrap(),
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-utl",
        );
    }

    /// Navnefeltet er 35 tegn. Et langt selskapsnavn skal korte ned, ikke
    /// stoppe en ellers riktig levering.
    #[test]
    fn langt_aksjonaernavn_kortes_i_stedet_for_a_feile() {
        let mut u = underskjema();
        u.id = Some(Aksjonaerid::Organisasjonsnummer("923609016".into()));
        u.navn = "Æ".repeat(60);
        let xml = render_underskjema(&u).unwrap();
        valider(
            &xml,
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            "under-langt-navn",
        );
        assert!(xml.contains(&"Æ".repeat(35)));
        assert!(!xml.contains(&"Æ".repeat(36)));
    }

    /// Kjernen i den ærlige begrensningen: et salg kan vi ikke levere,
    /// fordi vi ikke vet koden — og da sier vi det i stedet for å gjette.
    #[test]
    fn uverifisert_transaksjonstype_nektes_hoylytt() {
        let mut u = underskjema();
        u.bevegelser = vec![Bevegelse {
            dato: d(2026, 6, 1),
            type_: Transaksjonstype::Salg,
            antall: 10,
            belop_ore: Some(1_000_000),
        }];
        let feil = render_underskjema(&u).unwrap_err();
        assert_eq!(feil, OppgaveError::UverifisertKode(Transaksjonstype::Salg));
        assert!(feil.to_string().contains("salg"), "{feil}");
        assert!(feil.to_string().contains("gjetter"), "{feil}");
    }

    #[test]
    fn aksjonaer_uten_identitet_nektes() {
        let mut u = underskjema();
        u.id = None;
        assert!(matches!(
            render_underskjema(&u).unwrap_err(),
            OppgaveError::ManglerIdentitet(_)
        ));
    }

    #[test]
    fn rendringen_er_deterministisk() {
        assert_eq!(
            render_hovedskjema(&hovedskjema()).unwrap(),
            render_hovedskjema(&hovedskjema()).unwrap()
        );
        assert_eq!(
            render_underskjema(&underskjema()).unwrap(),
            render_underskjema(&underskjema()).unwrap()
        );
    }

    #[test]
    fn belop_skrives_med_to_desimaler() {
        assert_eq!(belop(10_000_000), "100000.00");
        assert_eq!(belop(12_345), "123.45");
        assert_eq!(belop(0), "0.00");
        assert_eq!(tidspunkt(d(2026, 1, 2)), "2026-01-02T00:00:00");
    }
}
