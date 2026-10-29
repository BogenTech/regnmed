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
//! `tests/verktoy.rs` is the guard — it asks a running regnmed for every
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
pub enum Kropp {
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

pub struct Verktoy {
    pub navn: &'static str,
    pub beskrivelse: &'static str,
    pub metode: &'static str,
    pub sti: &'static str,
    pub kropp: Kropp,
    /// JSON Schema, written out rather than built: the wording is the
    /// prompt a small model reads before choosing, and a builder would
    /// hide it.
    pub skjema: &'static str,
}

const SELSKAP: &str = r#""selskap": {"type": "string", "description": "Selskapets id (uuid). Kan utelates hvis serveren er startet for ett selskap."}"#;

pub const VERKTOY: &[Verktoy] = &[
    // ---------- Orientering ----------
    Verktoy {
        navn: "selskaper",
        beskrivelse: "Hvilke selskaper du kan arbeide for, hvilken rolle du har i hvert, \
                      hvilke rettigheter rollen gir, og abonnementsstatus. Kall denne FØRST \
                      i en ny samtale: den sier hva du har lov til, så du ikke prøver noe som blir nektet.",
        metode: "GET",
        sti: "/me",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "firmaopplysninger",
        beskrivelse: "Selskapets navn, organisasjonsnummer, adresse, kontonummer, selskapsform, \
                      om det har KID-avtale med banken, OG om det er mva-registrert og \
                      registrert i Foretaksregisteret (med historikk). \
                      KALL DENNE FØR du bruker en mva-kode: et selskap som ikke er \
                      mva-registrert kan ikke legge mva på noe, og forsøket blir avvist.",
        metode: "GET",
        sti: "/companies/{selskap}/settings",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "kontoplan",
        beskrivelse: "Selskapets kontoer med beregnet saldo, og standardkatalogen (NS 4102) \
                      å velge fra. Bruk denne for å finne riktig kontonummer før du foreslår \
                      et bilag — ikke gjett et kontonummer.",
        metode: "GET",
        sti: "/companies/{selskap}/accounts",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "periodestatus",
        beskrivelse: "Til og med hvilken dato regnskapet er låst. Et bilag med dato i en låst \
                      periode blir avvist; be et menneske om å åpne perioden, gjør det ikke selv.",
        metode: "GET",
        sti: "/companies/{selskap}/period-lock",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "attesteringspolicy",
        beskrivelse: "Om selskapet krever at et menneske attesterer før bokføring, og over \
                      hvilket beløp. Hvis krev_for_maskin er true må ALT du bokfører attesteres \
                      av et menneske først.",
        metode: "GET",
        sti: "/companies/{selskap}/attestering/policy",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    // ---------- Lese hovedboken ----------
    Verktoy {
        navn: "bilag_liste",
        beskrivelse: "Bokførte bilag med linjer, nyeste først. `sok` søker fritt i bilagsnummer, \
                      dato, tekst og kontonummer/-navn.",
        metode: "GET",
        sti: "/companies/{selskap}/vouchers",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP,
            "from":{"type":"string","description":"Fra dato, YYYY-MM-DD"},
            "to":{"type":"string","description":"Til dato, YYYY-MM-DD"},
            "sok":{"type":"string","description":"Fritekst"},
            "limit":{"type":"integer","description":"Maks antall bilag, standard 100"},
            "offset":{"type":"integer"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "saldobalanse",
        beskrivelse: "Saldo per konto for en periode — bruk denne til å avstemme mot et \
                      godkjent årsregnskap eller en tidligere rapport.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/saldobalanse",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["from","to"],"properties":{SELSKAP,
            "from":{"type":"string","description":"YYYY-MM-DD"},
            "to":{"type":"string","description":"YYYY-MM-DD"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "kontospesifikasjon",
        beskrivelse: "Alle posteringer på én konto i en periode, med løpende saldo og \
                      bilagshenvisning. Dette er rapporten du bruker når et tall ser rart ut.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/kontospesifikasjon",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["from","to"],"properties":{SELSKAP,
            "from":{"type":"string","description":"YYYY-MM-DD"},
            "to":{"type":"string","description":"YYYY-MM-DD"},
            "account":{"type":"string","description":"Kontonummer, f.eks. 1920. Utelates = alle kontoer."}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "resultat",
        beskrivelse: "Resultatregnskap gruppert etter NS 4102 for en periode, med \
                      presentasjonsfortegn (inntekt positiv).",
        metode: "GET",
        sti: "/companies/{selskap}/reports/resultat",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["from","to"],"properties":{SELSKAP,
            "from":{"type":"string","description":"YYYY-MM-DD"},
            "to":{"type":"string","description":"YYYY-MM-DD"},
            "avdeling":{"type":"string"},
            "prosjekt":{"type":"string"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "balanse",
        beskrivelse: "Balanse per en dato, gruppert etter NS 4102.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/balanse",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["date"],"properties":{SELSKAP,
            "date":{"type":"string","description":"YYYY-MM-DD"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "mva_rapport",
        beskrivelse: "Mva-spesifikasjonen for én termin: utgående, inngående og netto, per sats. \
                      Terminnummereringen følger selskapets terminordning — oppgi feil nummer og \
                      du får en tydelig feilmelding, ikke et galt tall.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/mva",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["year","termin"],"properties":{SELSKAP,
            "year":{"type":"integer"},
            "termin":{"type":"integer","description":"Terminnummer i selskapets ordning"}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "nokkeltall",
        beskrivelse: "Resultat hittil i år mot samme dato i fjor, månedskolonner, likviditet \
                      og de to neste mva-fristene. Bruk denne når noen spør «hvordan går det».",
        metode: "GET",
        sti: "/companies/{selskap}/reports/nokkeltall",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP,
            "year":{"type":"integer"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "kundespesifikasjon",
        beskrivelse: "Kundereskontro: hva hver kunde skylder, med åpne poster.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/kundespesifikasjon",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["from","to"],"properties":{SELSKAP,
            "from":{"type":"string"},"to":{"type":"string"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "leverandorspesifikasjon",
        beskrivelse: "Leverandørreskontro: hva selskapet skylder hver leverandør.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/leverandorspesifikasjon",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["from","to"],"properties":{SELSKAP,
            "from":{"type":"string"},"to":{"type":"string"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "revisjonsrapport",
        beskrivelse: "Åtte kontroller mot den levende hovedboken: hashkjeden fra genesis, \
                      vedleggshasher, forankring, reskontroavstemming, balanse, periodelås, \
                      regelverkssatser og importert historikk. KJØR DENNE etter at du har \
                      bokført noe større — den sier om arbeidet ditt holder, og avvik kommer \
                      som AVVIK-linjer, ikke som skjulte feil.",
        metode: "GET",
        sti: "/companies/{selskap}/reports/revisjon",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    // ---------- Bilag: foreslå, så bokfør ----------
    Verktoy {
        navn: "innboks",
        beskrivelse: "Dokumenter som venter på å bli bokført, med attesteringsstatus. \
                      Dette er arbeidskøen din.",
        metode: "GET",
        sti: "/companies/{selskap}/inbox",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP,
            "status":{"type":"string","enum":["ny","bokfort","avvist"],"description":"Standard: alle"}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "innboks_forslag",
        beskrivelse: "Regnmeds EGET bokføringsforslag for et dokument, med begrunnelse per felt \
                      og `kilde`: ehf (eksakt, lest av fakturaens XML), pdf-tekst eller tekst \
                      (heuristikk), eller ingen (skann uten tekstlag). START HER før du foreslår \
                      noe selv: feltene er utledet av kontrollsifre vi har validert \
                      (orgnr MOD11, KID, kontonummer), og kontoforslaget er kontoen samme \
                      leverandør sist ble bokført på. Er kilden «ingen», ikke dikt opp tall — \
                      si at dokumentet må leses av et menneske.",
        metode: "GET",
        sti: "/companies/{selskap}/inbox/{dokument}/forslag",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["dokument"],"properties":{SELSKAP,
            "dokument":{"type":"string","description":"document_id fra innboks"}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "bilag_last_opp",
        beskrivelse: "Laster en fil fra disk opp i bilagsinnboksen (kvittering, faktura, EHF). \
                      Filen hashes før sending, så en ny forsøk etter et tapt svar ikke blir to bilag.",
        metode: "POST",
        sti: "/companies/{selskap}/inbox",
        kropp: Kropp::FilBinaer,
        skjema: r#"{"type":"object","required":["fil"],"properties":{SELSKAP,
            "fil":{"type":"string","description":"Full sti til filen på denne maskinen"},
            "filename":{"type":"string","description":"Navnet dokumentet skal lagres med. Standard: filnavnet."}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "bilag_bokfor",
        beskrivelse: "Bokfører et innboksdokument som bilag. Summen av linjene MÅ være null \
                      (debet positiv, kredit negativ), ellers avvises bilaget. Dokumentet blir \
                      vedlegg på bilaget i samme transaksjon. Krever godkjent attestering når \
                      policyen sier det — sjekk attesteringspolicy først. \
                      BOKFØR IKKE noe du er usikker på: svar på oppgaven med status «sporsmal» i stedet.",
        metode: "POST",
        sti: "/companies/{selskap}/inbox/{dokument}/bokfor",
        kropp: Kropp::Json,
        skjema: r#"{"type":"object","required":["dokument","journal_code","date","description","lines"],
            "properties":{SELSKAP,
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
    Verktoy {
        navn: "bilag_for",
        beskrivelse: "Fører et bilag MANUELT, uten noe dokument i innboksen — en kontant \
                      utgift, en omføring, en avsetning. Summen av linjene MÅ være null \
                      (debet positiv, kredit negativ). Bruk denne når oppgaven gir deg tallene \
                      direkte; har du et dokument, bruk bilag_bokfor så dokumentet blir vedlegg. \
                      En aktiv attesteringspolicy over beløpsgrensen NEKTER manuell føring og \
                      viser til innboksen — sjekk attesteringspolicy først. \
                      FØR IKKE noe du er usikker på: svar på oppgaven med status «sporsmal».",
        metode: "POST",
        sti: "/companies/{selskap}/vouchers",
        kropp: Kropp::Json,
        skjema: r#"{"type":"object","required":["journal_code","date","description","lines"],
            "properties":{SELSKAP,
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
    Verktoy {
        navn: "bilag_avvis",
        beskrivelse: "Avviser et innboksdokument. Begrunnelsen er obligatorisk og blir stående: \
                      bruk denne når dokumentet ikke hører i dette regnskapet, ikke når du bare \
                      er usikker på konteringen.",
        metode: "POST",
        sti: "/companies/{selskap}/inbox/{dokument}/avvis",
        kropp: Kropp::Json,
        skjema: r#"{"type":"object","required":["dokument","note"],"properties":{SELSKAP,
            "dokument":{"type":"string"},
            "note":{"type":"string","description":"Hvorfor dokumentet avvises"}},
            "additionalProperties":false}"#,
    },
    // ---------- Salg ----------
    Verktoy {
        navn: "kunder",
        beskrivelse: "Kunder og leverandører med nummer — `party_no` du trenger på en faktura \
                      eller en reskontrolinje.",
        metode: "GET",
        sti: "/companies/{selskap}/parties",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP,
            "kind":{"type":"string","enum":["kunde","leverandor"]}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "fakturaer",
        beskrivelse: "Utstedte fakturaer med restbeløp. `open: true` gir bare de med noe utestående.",
        metode: "GET",
        sti: "/companies/{selskap}/invoices",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP,
            "open":{"type":"boolean"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "forfalte_fakturaer",
        beskrivelse: "Forfalte fakturaer med antall dager over forfall og foreslått neste \
                      purreskritt etter inkassoregelverket.",
        metode: "GET",
        sti: "/companies/{selskap}/invoices/overdue",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "faktura_opprett",
        beskrivelse: "Utsteder en faktura. Fakturaen er UFORANDERLIG straks den er utstedt — en \
                      feil rettes med kreditnota og ny faktura, aldri ved endring. Kontroller \
                      derfor beløp, kunde og mva FØR du kaller denne. Er selskapet ikke \
                      mva-registrert, utelat vat_code helt.",
        metode: "POST",
        sti: "/companies/{selskap}/invoices",
        kropp: Kropp::Json,
        skjema: r#"{"type":"object","required":["party_no","invoice_date","due_date","lines"],
            "properties":{SELSKAP,
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
    Verktoy {
        navn: "saft_analyser",
        beskrivelse: "Leser en SAF-T-fil fra disk UTEN å importere noe, og foreslår hvordan \
                      kontoene bør avstemmes mot NS 4102. Kall ALLTID denne først, vis forslaget \
                      til et menneske, og importer deretter. Svaret sier needs_mapping hvis \
                      kontoplanen må avstemmes.",
        metode: "POST",
        sti: "/companies/{selskap}/import/saft/analyze",
        kropp: Kropp::Fil,
        skjema: r#"{"type":"object","required":["fil"],"properties":{SELSKAP,
            "fil":{"type":"string","description":"Full sti til SAF-T XML-filen"}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "saft_importer",
        beskrivelse: "Importerer en SAF-T-fil inn i hovedboken. Alt-eller-ingenting. Døren står \
                      åpen bare så lenge hovedboken utelukkende inneholder importerte bilag — \
                      det første ordinære bilaget lukker den for godt, så importer HELE \
                      historikken, ett år per fil i rekkefølge, før noe annet bokføres. Samme \
                      fil kan ikke importeres to ganger (innholdet hashes).",
        metode: "POST",
        sti: "/companies/{selskap}/import/saft",
        kropp: Kropp::Fil,
        skjema: r#"{"type":"object","required":["fil"],"properties":{SELSKAP,
            "fil":{"type":"string","description":"Full sti til SAF-T XML-filen"},
            "mapping":{"type":"object","description":"Kontoavstemming fra saft_analyser: {\"gammel konto\": \"NS 4102-konto\"}. Utelates = filens egne kontonummer brukes.",
                "additionalProperties":{"type":"string"}}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "saft_logg",
        beskrivelse: "Hvilke SAF-T-filer historikken er bygget av, med innholds-hash og hvem som \
                      importerte. Sjekk denne før du importerer, så du ikke importerer det samme igjen.",
        metode: "GET",
        sti: "/companies/{selskap}/import/saft/log",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "kontakter_importer",
        beskrivelse: "Importerer kunder eller leverandører fra en CSV-fil fra et annet system. \
                      Layouten leses av overskriftene; ukjente overskrifter gir en tydelig feil \
                      som lister det som ble funnet. Idempotent på organisasjonsnummer.",
        metode: "POST",
        sti: "/companies/{selskap}/import/contacts",
        kropp: Kropp::Fil,
        skjema: r#"{"type":"object","required":["fil","kind"],"properties":{SELSKAP,
            "fil":{"type":"string","description":"Full sti til CSV-filen"},
            "kind":{"type":"string","enum":["kunde","leverandor"]}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "apne_poster_importer",
        beskrivelse: "Importerer åpne poster (ubetalte fakturaer) fra en CSV-fil. Kall den FØRST \
                      med preview: true — svaret sier om reskontrokontoen har en saldo som er i \
                      veien, og hvilke kunder som vil bli opprettet.",
        metode: "POST",
        sti: "/companies/{selskap}/import/open-items",
        kropp: Kropp::Fil,
        skjema: r#"{"type":"object","required":["fil","kind"],"properties":{SELSKAP,
            "fil":{"type":"string","description":"Full sti til CSV-filen"},
            "kind":{"type":"string","enum":["kunde","leverandor"]},
            "preview":{"type":"boolean","description":"true = regn ut og vis, bokfør ingenting"},
            "konto":{"type":"string","description":"Reskontrokonto; standard 1500 for kunde, 2400 for leverandør"},
            "motkonto":{"type":"string","description":"Motkonto for samlebilaget; standard 2050"},
            "dato":{"type":"string","description":"Bilagsdato, YYYY-MM-DD"}},
            "additionalProperties":false}"#,
    },
    // ---------- Oppgaver ----------
    Verktoy {
        navn: "oppgaver",
        beskrivelse: "Oppgavene eieren har gitt deg. Kall denne når du starter en arbeidsøkt, \
                      og arbeid deg gjennom køen. Du ser bare oppgaver som er stilt til deg \
                      eller til ingen bestemt.",
        metode: "GET",
        sti: "/companies/{selskap}/agent/oppgaver",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","properties":{SELSKAP,
            "apne":{"type":"boolean","description":"Standard true. false gir også de avsluttede."}},
            "additionalProperties":false}"#,
    },
    Verktoy {
        navn: "oppgave",
        beskrivelse: "Én oppgave med hele sporet av hva som er svart på den.",
        metode: "GET",
        sti: "/companies/{selskap}/agent/oppgaver/{oppgave}",
        kropp: Kropp::Ingen,
        skjema: r#"{"type":"object","required":["oppgave"],"properties":{SELSKAP,
            "oppgave":{"type":"string","description":"oppgave_id"}},"additionalProperties":false}"#,
    },
    Verktoy {
        navn: "oppgave_svar",
        beskrivelse: "Rapporterer på en oppgave. `pagar` = jeg har begynt. `sporsmal` = jeg \
                      trenger en avgjørelse av et menneske før jeg kan fortsette — BRUK DENNE \
                      I STEDET FOR Å GJETTE. `utfort` = ferdig, med hva du gjorde. `avvist` = \
                      dette kan eller bør jeg ikke gjøre, med hvorfor. `utfort` og `avvist` er \
                      ENDELIGE og kan ikke trekkes tilbake. Begrunnelse er obligatorisk for \
                      `sporsmal` og `avvist`.",
        metode: "POST",
        sti: "/companies/{selskap}/agent/oppgaver/{oppgave}/svar",
        kropp: Kropp::Json,
        skjema: r#"{"type":"object","required":["oppgave","status"],"properties":{SELSKAP,
            "oppgave":{"type":"string","description":"oppgave_id"},
            "status":{"type":"string","enum":["pagar","sporsmal","utfort","avvist"]},
            "notat":{"type":"string","description":"Hva du gjorde, eller hva du trenger svar på"}},
            "additionalProperties":false}"#,
    },
];

/// The schema with the `SELSKAP` placeholder expanded — it is in every
/// company-scoped tool and is the longest, least interesting property in
/// the file, so it is written once.
pub fn skjema(v: &Verktoy) -> String {
    v.skjema.replace("SELSKAP", SELSKAP)
}

pub fn finn(navn: &str) -> Option<&'static Verktoy> {
    VERKTOY.iter().find(|v| v.navn == navn)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every schema must be valid JSON after expansion. A malformed one
    /// would reach the agent as a broken tool list, and an agent cannot
    /// debug that.
    #[test]
    fn every_schema_is_valid_json() {
        for v in VERKTOY {
            let utvidet = skjema(v);
            serde_json::from_str::<serde_json::Value>(&utvidet)
                .unwrap_or_else(|e| panic!("{}: {e}\n{utvidet}", v.navn));
        }
    }

    /// A description is the only thing a model has to choose by. An
    /// empty or terse one is a defect, not a style issue.
    #[test]
    fn every_tool_is_described_and_uniquely_named() {
        let mut navn: Vec<&str> = VERKTOY.iter().map(|v| v.navn).collect();
        let antall = navn.len();
        navn.sort_unstable();
        navn.dedup();
        assert_eq!(navn.len(), antall, "to verktøy med samme navn");
        for v in VERKTOY {
            assert!(
                v.beskrivelse.len() > 40,
                "{} har en beskrivelse ingen modell kan velge etter",
                v.navn
            );
            assert!(
                matches!(v.metode, "GET" | "POST" | "PUT" | "DELETE"),
                "{}: {}",
                v.navn,
                v.metode
            );
        }
    }

    /// Path placeholders must be declared in the schema, or the agent
    /// has no way to fill them.
    #[test]
    fn every_path_placeholder_is_a_declared_property() {
        for v in VERKTOY {
            let utvidet = skjema(v);
            let mut rest = v.sti;
            while let Some(start) = rest.find('{') {
                let end = rest[start..].find('}').expect("ubalansert sti") + start;
                let felt = &rest[start + 1..end];
                assert!(
                    utvidet.contains(&format!("\"{felt}\"")),
                    "{}: stien krever «{felt}», men skjemaet nevner den ikke",
                    v.navn
                );
                rest = &rest[end + 1..];
            }
        }
    }
}
