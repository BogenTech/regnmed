//! The tool table (#102).
//!
//! **Curated, not generated.** docs/api.md lists around two hundred
//! endpoints, and #102 suggested generating the tool list from the same
//! route table so it cannot rot. That was tried and rejected for one
//! reason: a tool list is a PROMPT. Two hundred tools is the surest way
//! to make a small model pick the wrong one, and the brief here is that
//! the agent must do a good job on a small model. So this is a chosen
//! set — the tools a bookkeeper or regnskapsfører actually reaches for,
//! each with a description written to be read by a model rather than by
//! a developer.
//!
//! The honest cost of that choice: this table CAN drift from the API.
//! `tests/tool.rs` is the guard — it asks a running regnmed for every
//! path here and fails on anything that no longer exists.
//!
//! Conventions a small model must not have to infer, so each schema
//! repeats them where they apply:
//!
//! - Amounts are integer ØRE. Positive is debit, negative is credit.
//! - Dates are `YYYY-MM-DD`.
//! - `selskap` may be omitted when the server was started with
//!   `REGNMED_COMPANY`.

/// How the arguments become a request body.
pub enum Body {
    /// No body; leftover arguments become the query string.
    Ingen,
    /// Leftover arguments become the JSON body.
    Json,
    /// `fil` is read from disk and sent as the body (text formats:
    /// SAF-T XML, CSV). With `mapping` present it is sent as the JSON
    /// envelope the import endpoint accepts instead.
    Fil,
    /// `fil` is read as bytes and uploaded, with `filename` and the
    /// content hash in the query — so a retry after a lost reply cannot
    /// turn one bilag into two (#102's idempotence rule).
    FilBinaer,
}

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub body: Body,
    /// JSON Schema, written out rather than built: the wording is the
    /// prompt a small model reads before choosing, and a builder would
    /// hide it.
    pub schema: &'static str,
}

const COMPANY: &str = r#""selskap": {"type": "string", "description": "Selskapets id (uuid). Kan utelates hvis serveren er startet for ett selskap."}"#;

pub const TOOLS: &[Tool] = &[
    // ---------- Orientering ----------
    Tool {
        name: "selskaper",
        description: "Hvilke selskaper du kan arbeide for, hvilken rolle du har i hvert, \
                      hvilke rettigheter rollen gir, og abonnementsstatus. Kall denne FØRST \
                      i en ny samtale: den sier hva du har lov til, så du ikke prøver noe som blir nektet.",
        method: "GET",
        path: "/me",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{},"additionalProperties":false}"#,
    },
    Tool {
        name: "firmaopplysninger",
        description: "Selskapets navn, organisasjonsnummer, adresse, kontonummer, selskapsform, \
                      om det har KID-avtale med banken, OG om det er mva-registrert og \
                      registrert i Foretaksregisteret (med historikk). \
                      KALL DENNE FØR du bruker en mva-kode: et selskap som ikke er \
                      mva-registrert kan ikke legge mva på noe, og forsøket blir avvist.",
        method: "GET",
        path: "/companies/{selskap}/settings",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    Tool {
        name: "kontoplan",
        description: "Selskapets kontoer med beregnet saldo, og standardkatalogen (NS 4102) \
                      å velge fra. Bruk denne for å finne riktig kontonummer før du foreslår \
                      et bilag — ikke gjett et kontonummer.",
        method: "GET",
        path: "/companies/{selskap}/accounts",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    Tool {
        name: "periodestatus",
        description: "Til og med hvilken dato regnskapet er låst. Et bilag med dato i en låst \
                      periode blir avvist; be et menneske om å åpne perioden, gjør det ikke selv.",
        method: "GET",
        path: "/companies/{selskap}/period-lock",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    Tool {
        name: "attesteringspolicy",
        description: "Om selskapet krever at et menneske attesterer før bokføring, og over \
                      hvilket beløp. Hvis krev_for_maskin er true må ALT du bokfører attesteres \
                      av et menneske først.",
        method: "GET",
        path: "/companies/{selskap}/attestering/policy",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    // ---------- Lese hovedboken ----------
    Tool {
        name: "bilag_liste",
        description: "Bokførte bilag med linjer, nyeste først. `sok` søker fritt i bilagsnummer, \
                      dato, tekst og kontonummer/-navn.",
        method: "GET",
        path: "/companies/{selskap}/vouchers",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY,
            "from":{"type":"string","description":"Fra dato, YYYY-MM-DD"},
            "to":{"type":"string","description":"Til dato, YYYY-MM-DD"},
            "sok":{"type":"string","description":"Fritekst"},
            "limit":{"type":"integer","description":"Maks antall bilag, standard 100"},
            "offset":{"type":"integer"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "saldobalanse",
        description: "Saldo per konto for en periode — bruk denne til å avstemme mot et \
                      godkjent årsregnskap eller en tidligere rapport.",
        method: "GET",
        path: "/companies/{selskap}/reports/saldobalanse",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["from","to"],"properties":{COMPANY,
            "from":{"type":"string","description":"YYYY-MM-DD"},
            "to":{"type":"string","description":"YYYY-MM-DD"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "kontospesifikasjon",
        description: "Alle posteringer på én konto i en periode, med løpende saldo og \
                      bilagshenvisning. Dette er rapporten du bruker når et tall ser rart ut.",
        method: "GET",
        path: "/companies/{selskap}/reports/kontospesifikasjon",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["from","to"],"properties":{COMPANY,
            "from":{"type":"string","description":"YYYY-MM-DD"},
            "to":{"type":"string","description":"YYYY-MM-DD"},
            "account":{"type":"string","description":"Kontonummer, f.eks. 1920. Utelates = alle kontoer."}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "resultat",
        description: "Resultatregnskap gruppert etter NS 4102 for en periode, med \
                      presentasjonsfortegn (inntekt positiv).",
        method: "GET",
        path: "/companies/{selskap}/reports/resultat",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["from","to"],"properties":{COMPANY,
            "from":{"type":"string","description":"YYYY-MM-DD"},
            "to":{"type":"string","description":"YYYY-MM-DD"},
            "avdeling":{"type":"string"},
            "prosjekt":{"type":"string"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "balanse",
        description: "Balanse per en dato, gruppert etter NS 4102.",
        method: "GET",
        path: "/companies/{selskap}/reports/balanse",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["date"],"properties":{COMPANY,
            "date":{"type":"string","description":"YYYY-MM-DD"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "mva_rapport",
        description: "Mva-spesifikasjonen for én termin: utgående, inngående og netto, per sats. \
                      Terminnummereringen følger selskapets terminordning — oppgi feil nummer og \
                      du får en tydelig feilmelding, ikke et galt tall.",
        method: "GET",
        path: "/companies/{selskap}/reports/mva",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["year","termin"],"properties":{COMPANY,
            "year":{"type":"integer"},
            "termin":{"type":"integer","description":"Terminnummer i selskapets ordning"}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "nokkeltall",
        description: "Resultat hittil i år mot samme dato i fjor, månedskolonner, likviditet \
                      og de to neste mva-fristene. Bruk denne når noen spør «hvordan går det».",
        method: "GET",
        path: "/companies/{selskap}/reports/nokkeltall",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY,
            "year":{"type":"integer"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "kundespesifikasjon",
        description: "Kundereskontro: hva hver kunde skylder, med åpne poster.",
        method: "GET",
        path: "/companies/{selskap}/reports/kundespesifikasjon",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["from","to"],"properties":{COMPANY,
            "from":{"type":"string"},"to":{"type":"string"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "leverandorspesifikasjon",
        description: "Leverandørreskontro: hva selskapet skylder hver leverandør.",
        method: "GET",
        path: "/companies/{selskap}/reports/leverandorspesifikasjon",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["from","to"],"properties":{COMPANY,
            "from":{"type":"string"},"to":{"type":"string"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "revisjonsrapport",
        description: "Åtte kontroller mot den levende hovedboken: hashkjeden fra genesis, \
                      vedleggshasher, forankring, reskontroavstemming, balanse, periodelås, \
                      regelverkssatser og importert historikk. KJØR DENNE etter at du har \
                      bokført noe større — den sier om arbeidet ditt holder, og avvik kommer \
                      som AVVIK-linjer, ikke som skjulte feil.",
        method: "GET",
        path: "/companies/{selskap}/reports/revisjon",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    // ---------- Bilag: foreslå, så bokfør ----------
    Tool {
        name: "innboks",
        description: "Dokumenter som venter på å bli bokført, med attesteringsstatus. \
                      Dette er arbeidskøen din.",
        method: "GET",
        path: "/companies/{selskap}/inbox",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY,
            "status":{"type":"string","enum":["ny","bokfort","avvist"],"description":"Standard: alle"}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "innboks_forslag",
        description: "Regnmeds EGET bokføringsforslag for et dokument, med begrunnelse per felt \
                      og `kilde`: ehf (eksakt, lest av fakturaens XML), pdf-tekst eller tekst \
                      (heuristikk), eller ingen (skann uten tekstlag). START HER før du foreslår \
                      noe selv: feltene er utledet av kontrollsifre vi har validert \
                      (orgnr MOD11, KID, kontonummer), og kontoforslaget er kontoen samme \
                      leverandør sist ble bokført på. Er kilden «ingen», ikke dikt opp tall — \
                      si at dokumentet må leses av et menneske.",
        method: "GET",
        path: "/companies/{selskap}/inbox/{dokument}/forslag",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["dokument"],"properties":{COMPANY,
            "dokument":{"type":"string","description":"document_id fra innboks"}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "bilag_last_opp",
        description: "Laster en fil fra disk opp i bilagsinnboksen (kvittering, faktura, EHF). \
                      Filen hashes før sending, så en ny forsøk etter et tapt svar ikke blir to bilag.",
        method: "POST",
        path: "/companies/{selskap}/inbox",
        body: Body::FilBinaer,
        schema: r#"{"type":"object","required":["fil"],"properties":{COMPANY,
            "fil":{"type":"string","description":"Full sti til filen på denne maskinen"},
            "filename":{"type":"string","description":"Navnet dokumentet skal lagres med. Standard: filnavnet."}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "bilag_bokfor",
        description: "Bokfører et innboksdokument som bilag. Summen av linjene MÅ være null \
                      (debet positiv, kredit negativ), ellers avvises bilaget. Dokumentet blir \
                      vedlegg på bilaget i samme transaksjon. Krever godkjent attestering når \
                      policyen sier det — sjekk attesteringspolicy først. \
                      BOKFØR IKKE noe du er usikker på: svar på oppgaven med status «sporsmal» i stedet.",
        method: "POST",
        path: "/companies/{selskap}/inbox/{dokument}/bokfor",
        body: Body::Json,
        schema: r#"{"type":"object","required":["dokument","journal_code","date","description","lines"],
            "properties":{COMPANY,
            "dokument":{"type":"string","description":"document_id fra innboks"},
            "journal_code":{"type":"string","description":"Vanligvis GL"},
            "date":{"type":"string","description":"Bilagsdato, YYYY-MM-DD — dokumentets dato, ikke dagens"},
            "description":{"type":"string","description":"Kort bilagstekst"},
            "lines":{"type":"array","minItems":2,"items":{"type":"object",
                "required":["account","amount_ore"],
                "properties":{
                    "account":{"type":"string","description":"Kontonummer"},
                    "amount_ore":{"type":"integer","description":"Heltall øre. Positivt = debet, negativt = kredit."},
                    "vat_code":{"type":"string","description":"Mva-kode. Utelat helt hvis selskapet ikke er mva-registrert."},
                    "description":{"type":"string"},
                    "party_no":{"type":"string","description":"Kunde-/leverandørnummer. Påkrevd på reskontrokontoer."},
                    "avdeling":{"type":"string"},
                    "prosjekt":{"type":"string"}}}}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "bilag_for",
        description: "Fører et bilag MANUELT, uten noe dokument i innboksen — en kontant \
                      utgift, en omføring, en avsetning. Summen av linjene MÅ være null \
                      (debet positiv, kredit negativ). Bruk denne når oppgaven gir deg tallene \
                      direkte; har du et dokument, bruk bilag_bokfor så dokumentet blir vedlegg. \
                      En aktiv attesteringspolicy over beløpsgrensen NEKTER manuell føring og \
                      viser til innboksen — sjekk attesteringspolicy først. \
                      FØR IKKE noe du er usikker på: svar på oppgaven med status «sporsmal».",
        method: "POST",
        path: "/companies/{selskap}/vouchers",
        body: Body::Json,
        schema: r#"{"type":"object","required":["journal_code","date","description","lines"],
            "properties":{COMPANY,
            "journal_code":{"type":"string","description":"Vanligvis GL"},
            "date":{"type":"string","description":"Bilagsdato, YYYY-MM-DD — bilagets dato, ikke dagens"},
            "description":{"type":"string","description":"Kort bilagstekst"},
            "lines":{"type":"array","minItems":2,"items":{"type":"object",
                "required":["account","amount_ore"],
                "properties":{
                    "account":{"type":"string","description":"Kontonummer"},
                    "amount_ore":{"type":"integer","description":"Heltall øre. Positivt = debet, negativt = kredit."},
                    "vat_code":{"type":"string","description":"Mva-kode. Utelat helt hvis selskapet ikke er mva-registrert."},
                    "description":{"type":"string"},
                    "party_no":{"type":"string","description":"Kunde-/leverandørnummer. Påkrevd på reskontrokontoer."},
                    "avdeling":{"type":"string"},
                    "prosjekt":{"type":"string"}}}}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "bilag_avvis",
        description: "Avviser et innboksdokument. Begrunnelsen er obligatorisk og blir stående: \
                      bruk denne når dokumentet ikke hører i dette regnskapet, ikke når du bare \
                      er usikker på konteringen.",
        method: "POST",
        path: "/companies/{selskap}/inbox/{dokument}/avvis",
        body: Body::Json,
        schema: r#"{"type":"object","required":["dokument","note"],"properties":{COMPANY,
            "dokument":{"type":"string"},
            "note":{"type":"string","description":"Hvorfor dokumentet avvises"}},
            "additionalProperties":false}"#,
    },
    // ---------- Salg ----------
    Tool {
        name: "kunder",
        description: "Kunder og leverandører med nummer — `party_no` du trenger på en faktura \
                      eller en reskontrolinje.",
        method: "GET",
        path: "/companies/{selskap}/parties",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY,
            "kind":{"type":"string","enum":["kunde","leverandor"]}},"additionalProperties":false}"#,
    },
    Tool {
        name: "fakturaer",
        description: "Utstedte fakturaer med restbeløp. `open: true` gir bare de med noe utestående.",
        method: "GET",
        path: "/companies/{selskap}/invoices",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY,
            "open":{"type":"boolean"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "forfalte_fakturaer",
        description: "Forfalte fakturaer med antall dager over forfall og foreslått neste \
                      purreskritt etter inkassoregelverket.",
        method: "GET",
        path: "/companies/{selskap}/invoices/overdue",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    Tool {
        name: "faktura_opprett",
        description: "Utsteder en faktura. Fakturaen er UFORANDERLIG straks den er utstedt — en \
                      feil rettes med kreditnota og ny faktura, aldri ved endring. Kontroller \
                      derfor beløp, kunde og mva FØR du kaller denne. Er selskapet ikke \
                      mva-registrert, utelat `vat_code` helt.",
        method: "POST",
        path: "/companies/{selskap}/invoices",
        body: Body::Json,
        schema: r#"{"type":"object","required":["party_no","invoice_date","due_date","lines"],
            "properties":{COMPANY,
            "party_no":{"type":"string","description":"Kundenummer fra «kunder»"},
            "invoice_date":{"type":"string","description":"YYYY-MM-DD"},
            "due_date":{"type":"string","description":"YYYY-MM-DD"},
            "leveringsdato":{"type":"string","description":"Utelates = fakturadato"},
            "lines":{"type":"array","minItems":1,"items":{"type":"object","properties":{
                "description":{"type":"string"},
                "account":{"type":"string","description":"Inntektskonto, standard 3000"},
                "quantity_milli":{"type":"integer","description":"Antall i tusendeler: 1 stk = 1000, 2,5 = 2500"},
                "unit_price_ore":{"type":"integer","description":"Enhetspris i heltall øre"},
                "vat_code":{"type":"string"},
                "produkt":{"type":"string","description":"Produktnummer; fyller konto og pris fra registeret"},
                "prosjekt":{"type":"string"}}}}},
            "additionalProperties":false}"#,
    },
    // ---------- Migrering: den første oppgaven ----------
    Tool {
        name: "saft_analyser",
        description: "Leser en SAF-T-fil fra disk UTEN å importere noe, og foreslår hvordan \
                      kontoene bør avstemmes mot NS 4102. Kall ALLTID denne først, vis forslaget \
                      til et menneske, og importer deretter. Svaret sier needs_mapping hvis \
                      kontoplanen må avstemmes.",
        method: "POST",
        path: "/companies/{selskap}/import/saft/analyze",
        body: Body::Fil,
        schema: r#"{"type":"object","required":["fil"],"properties":{COMPANY,
            "fil":{"type":"string","description":"Full sti til SAF-T XML-filen"}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "saft_importer",
        description: "Importerer en SAF-T-fil inn i hovedboken. Alt-eller-ingenting. Døren står \
                      åpen bare så lenge hovedboken utelukkende inneholder importerte bilag — \
                      det første ordinære bilaget lukker den for godt, så importer HELE \
                      historikken, ett år per fil i rekkefølge, før noe annet bokføres. Samme \
                      fil kan ikke importeres to ganger (innholdet hashes).",
        method: "POST",
        path: "/companies/{selskap}/import/saft",
        body: Body::Fil,
        schema: r#"{"type":"object","required":["fil"],"properties":{COMPANY,
            "fil":{"type":"string","description":"Full sti til SAF-T XML-filen"},
            "mapping":{"type":"object","description":"Kontoavstemming fra saft_analyser: {\"gammel konto\": \"NS 4102-konto\"}. Utelates = filens egne kontonummer brukes.",
                "additionalProperties":{"type":"string"}}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "saft_logg",
        description: "Hvilke SAF-T-filer historikken er bygget av, med innholds-hash og hvem som \
                      importerte. Sjekk denne før du importerer, så du ikke importerer det samme igjen.",
        method: "GET",
        path: "/companies/{selskap}/import/saft/log",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY},"additionalProperties":false}"#,
    },
    Tool {
        name: "kontakter_importer",
        description: "Importerer kunder eller leverandører fra en CSV-fil fra et annet system. \
                      Layouten leses av overskriftene; ukjente overskrifter gir en tydelig feil \
                      som lister det som ble funnet. Idempotent på organisasjonsnummer.",
        method: "POST",
        path: "/companies/{selskap}/import/contacts",
        body: Body::Fil,
        schema: r#"{"type":"object","required":["fil","kind"],"properties":{COMPANY,
            "fil":{"type":"string","description":"Full sti til CSV-filen"},
            "kind":{"type":"string","enum":["kunde","leverandor"]}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "apne_poster_importer",
        description: "Importerer åpne poster (ubetalte fakturaer) fra en CSV-fil. Kall den FØRST \
                      med preview: true — svaret sier om reskontrokontoen har en saldo som er i \
                      veien, og hvilke kunder som vil bli opprettet.",
        method: "POST",
        path: "/companies/{selskap}/import/open-items",
        body: Body::Fil,
        schema: r#"{"type":"object","required":["fil","kind"],"properties":{COMPANY,
            "fil":{"type":"string","description":"Full sti til CSV-filen"},
            "kind":{"type":"string","enum":["kunde","leverandor"]},
            "preview":{"type":"boolean","description":"true = regn ut og vis, bokfør ingenting"},
            "konto":{"type":"string","description":"Reskontrokonto; standard 1500 for kunde, 2400 for leverandør"},
            "motkonto":{"type":"string","description":"Motkonto for samlebilaget; standard 2050"},
            "dato":{"type":"string","description":"Bilagsdato, YYYY-MM-DD"}},
            "additionalProperties":false}"#,
    },
    // ---------- Oppgaver ----------
    Tool {
        name: "oppgaver",
        description: "Oppgavene eieren har gitt deg. Kall denne når du starter en arbeidsøkt, \
                      og arbeid deg gjennom køen. Du ser bare oppgaver som er stilt til deg \
                      eller til ingen bestemt.",
        method: "GET",
        path: "/companies/{selskap}/agent/oppgaver",
        body: Body::Ingen,
        schema: r#"{"type":"object","properties":{COMPANY,
            "apne":{"type":"boolean","description":"Standard true. false gir også de avsluttede."}},
            "additionalProperties":false}"#,
    },
    Tool {
        name: "oppgave",
        description: "Én oppgave med hele sporet av hva som er svart på den.",
        method: "GET",
        path: "/companies/{selskap}/agent/oppgaver/{oppgave}",
        body: Body::Ingen,
        schema: r#"{"type":"object","required":["oppgave"],"properties":{COMPANY,
            "oppgave":{"type":"string","description":"oppgave_id"}},"additionalProperties":false}"#,
    },
    Tool {
        name: "oppgave_svar",
        description: "Rapporterer på en oppgave. `pagar` = jeg har begynt. `sporsmal` = jeg \
                      trenger en avgjørelse av et menneske før jeg kan fortsette — BRUK DENNE \
                      I STEDET FOR Å GJETTE. `utfort` = ferdig, med hva du gjorde. `avvist` = \
                      dette kan eller bør jeg ikke gjøre, med hvorfor. `utfort` og `avvist` er \
                      ENDELIGE og kan ikke trekkes tilbake. Begrunnelse er obligatorisk for \
                      `sporsmal` og `avvist`.",
        method: "POST",
        path: "/companies/{selskap}/agent/oppgaver/{oppgave}/svar",
        body: Body::Json,
        schema: r#"{"type":"object","required":["oppgave","status"],"properties":{COMPANY,
            "oppgave":{"type":"string","description":"oppgave_id"},
            "status":{"type":"string","enum":["pagar","sporsmal","utfort","avvist"]},
            "notat":{"type":"string","description":"Hva du gjorde, eller hva du trenger svar på"}},
            "additionalProperties":false}"#,
    },
];

/// The schema with the `COMPANY` placeholder expanded — it is in every
/// company-scoped tool and is the longest, least interesting property in
/// the file, so it is written once.
/// Expands the `COMPANY` placeholder a schema carries into the shared
/// company property: one definition behind thirty-four schemas.
pub fn schema(v: &Tool) -> String {
    v.schema.replace("COMPANY", COMPANY)
}

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|v| v.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every schema must be valid JSON after expansion. A malformed one
    /// would reach the agent as a broken tool list, and an agent cannot
    /// debug that.
    #[test]
    fn every_schema_is_valid_json() {
        for v in TOOLS {
            let utvidet = schema(v);
            serde_json::from_str::<serde_json::Value>(&utvidet)
                .unwrap_or_else(|e| panic!("{}: {e}\n{utvidet}", v.name));
        }
    }

    /// A description is the only thing a model has to choose by. An
    /// empty or terse one is a defect, not a style issue.
    #[test]
    fn every_tool_is_described_and_uniquely_named() {
        let mut name: Vec<&str> = TOOLS.iter().map(|v| v.name).collect();
        let count = name.len();
        name.sort_unstable();
        name.dedup();
        assert_eq!(name.len(), count, "to verktøy med samme navn");
        for v in TOOLS {
            assert!(
                v.description.len() > 40,
                "{} har en beskrivelse ingen modell kan velge etter",
                v.name
            );
            assert!(
                matches!(v.method, "GET" | "POST" | "PUT" | "DELETE"),
                "{}: {}",
                v.name,
                v.method
            );
        }
    }

    /// Path placeholders must be declared in the schema, or the agent
    /// has no way to fill them.
    #[test]
    fn every_path_placeholder_is_a_declared_property() {
        for v in TOOLS {
            let utvidet = schema(v);
            let mut rest = v.path;
            while let Some(start) = rest.find('{') {
                let end = rest[start..].find('}').expect("ubalansert sti") + start;
                let felt = &rest[start + 1..end];
                assert!(
                    utvidet.contains(&format!("\"{felt}\"")),
                    "{}: stien krever «{felt}», men skjemaet nevner den ikke",
                    v.name
                );
                rest = &rest[end + 1..];
            }
        }
    }
}
