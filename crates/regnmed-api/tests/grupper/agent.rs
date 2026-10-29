//! Agenten som ansatt eller regnskapsfører (#101, #102, #103).
//!
//! This is the end-to-end test the MCP server's tool table promises:
//! `crates/regnmed-mcp/src/verktoy.rs` is CURATED rather than generated
//! from the route table, which buys a tool list a small model can
//! actually choose from — and costs the guarantee that it still matches
//! the API. This test is that guarantee. It serves the real router on a
//! real socket, points a real MCP client at it, and calls every tool in
//! the table with a real machine token.
//!
//! The assertion is deliberately not "everything answers 200": several
//! tools need arguments the test has no sensible value for. It is that
//! **nothing answers 404 for a path that does not exist, and no call
//! returns a transport error** — the two failures a drifted table
//! produces and an agent cannot diagnose.
//!
//! Requires DATABASE_URL (skips otherwise).

use std::collections::BTreeMap;

use crate::common::{TestIdp, gjor_fakturaklar, test_state, unique_orgnr};
use serde_json::{Value, json};
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_mcp::{Server, klient::Klient};

/// Serves the router on an ephemeral port and returns its base URL.
/// The task is left running for the test's lifetime; the process ends
/// when the test does.
async fn tjen(state: AppState) -> String {
    let lytter = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = lytter.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(lytter, router(state)).await;
    });
    format!("http://{addr}")
}

async fn kall(server: &Server, navn: &str, args: Value) -> (bool, String) {
    let svar = server
        .handter(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": navn, "arguments": args },
        }))
        .await
        .expect("et kall med id får alltid svar");
    assert!(
        svar.get("error").is_none(),
        "{navn}: et verktøykall skal aldri bli en protokollfeil — modellen ser den ikke: {svar}"
    );
    let innhold = svar["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    (
        svar["result"]["isError"].as_bool().unwrap_or(false),
        innhold,
    )
}

#[tokio::test]
async fn every_tool_in_the_table_still_matches_the_api() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let eier_sub = format!("test|{}", Uuid::new_v4());
    let eier = regnmed_db::ensure_person(&state.pool, &eier_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Agentverktoy AS")
        .await
        .unwrap();
    gjor_fakturaklar(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, eier, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();

    // The agent gets `admin`, which #101 made grantable — so every tool
    // in the table is reachable and a 403 here would be a real finding.
    let client_id = format!("agent-{}", Uuid::new_v4());
    let integration_id = regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Regnskapsagenten",
        None,
        "admin",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    // The table has 30-odd tools and the default budget is 120 calls a
    // minute; this test spends them in a burst.
    sqlx::query("update integration set rate_limit_min = 10000 where id = $1")
        .bind(integration_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let base = tjen(state.clone()).await;
    let server = Server::new(Klient::ny(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    // A task to answer, so the oppgave tools have a real target.
    let oppgave = regnmed_db::agentoppgave::opprett_oppgave(
        &state.pool,
        company,
        Some(integration_id),
        "Importer regnskapet fra Conta og avstem mot årsregnskapet.",
        None,
        "Eier Eiersen",
    )
    .await
    .unwrap();

    // Arguments good enough to reach the endpoint. Tools whose action
    // needs a document, a file or a party are exercised by name below
    // instead — reaching them here would mean inventing a bilag.
    let aar = json!({"from": "2026-01-01", "to": "2026-12-31"});
    let argumenter: BTreeMap<&str, Value> = BTreeMap::from([
        ("selskaper", json!({})),
        ("firmaopplysninger", json!({})),
        ("kontoplan", json!({})),
        ("periodestatus", json!({})),
        ("attesteringspolicy", json!({})),
        ("bilag_liste", json!({"limit": 5})),
        ("saldobalanse", aar.clone()),
        ("kontospesifikasjon", aar.clone()),
        ("resultat", aar.clone()),
        ("balanse", json!({"date": "2026-12-31"})),
        ("mva_rapport", json!({"year": 2026, "termin": 1})),
        ("nokkeltall", json!({"year": 2026})),
        ("kundespesifikasjon", aar.clone()),
        ("leverandorspesifikasjon", aar.clone()),
        ("revisjonsrapport", json!({})),
        ("innboks", json!({})),
        ("kunder", json!({})),
        ("fakturaer", json!({})),
        ("forfalte_fakturaer", json!({})),
        ("saft_logg", json!({})),
        ("oppgaver", json!({})),
        ("oppgave", json!({"oppgave": oppgave.to_string()})),
    ]);

    for (navn, args) in &argumenter {
        let (feilet, tekst) = kall(&server, navn, args.clone()).await;
        assert!(
            !feilet,
            "{navn} nådde ikke fram eller ble nektet — verktøytabellen har sklidt fra API-et: {tekst}"
        );
    }

    // Every READ tool in the table must be covered above. A tool added
    // without a line here is a tool nothing checks.
    let dekket: Vec<&str> = argumenter.keys().copied().collect();
    // Exercised further down instead, where there is a real document,
    // a real file or a real task to act on.
    let senere = [
        "innboks_forslag",
        "bilag_last_opp",
        "bilag_bokfor",
        "bilag_avvis",
        "faktura_opprett",
        "saft_analyser",
        "saft_importer",
        "kontakter_importer",
        "apne_poster_importer",
        "oppgave_svar",
    ];
    for v in regnmed_mcp::verktoy::VERKTOY {
        assert!(
            dekket.contains(&v.navn) || senere.contains(&v.navn),
            "verktøyet «{}» er ikke dekket av denne testen",
            v.navn
        );
    }

    // ---- The writing path, end to end through the tools ----
    let (feilet, svar) = kall(
        &server,
        "bilag_last_opp",
        json!({"fil": skriv_midlertidig("kvittering.txt", b"Kaffe 49,00")}),
    )
    .await;
    assert!(!feilet, "{svar}");
    let dokument = serde_json::from_str::<Value>(&svar).unwrap()["document_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (feilet, svar) = kall(&server, "innboks_forslag", json!({"dokument": dokument})).await;
    assert!(!feilet, "forslaget svarer for ethvert dokument: {svar}");

    for (nummer, navn) in [("1920", "Bank"), ("7790", "Annen kostnad")] {
        regnmed_db::ensure_account(&state.pool, company, nummer, navn)
            .await
            .unwrap();
    }
    let (feilet, svar) = kall(
        &server,
        "bilag_bokfor",
        json!({
            "dokument": dokument,
            "journal_code": "GL",
            "date": "2026-03-04",
            "description": "Kaffe",
            "lines": [
                {"account": "7790", "amount_ore": 4900},
                {"account": "1920", "amount_ore": -4900},
            ],
        }),
    )
    .await;
    assert!(!feilet, "{svar}");

    // The bilag names the AGENT, not an anonymous subject — the whole
    // reason an integration is a person row (#45).
    let created_by: String = sqlx::query_scalar(
        "select created_by from voucher where company_id = $1 order by created_at desc limit 1",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(created_by, "Regnskapsagenten");

    // ---- And the agent reports back on the task it was given ----
    let (feilet, svar) = kall(
        &server,
        "oppgave_svar",
        json!({"oppgave": oppgave.to_string(), "status": "sporsmal",
               "notat": "Kontoplanen må avstemmes mot NS 4102 — bekreft forslaget."}),
    )
    .await;
    assert!(!feilet, "{svar}");
    let (feilet, svar) = kall(
        &server,
        "oppgave_svar",
        json!({"oppgave": oppgave.to_string(), "status": "utfort",
               "notat": "Importert og avstemt."}),
    )
    .await;
    assert!(!feilet, "{svar}");
    // Terminal means terminal: migration 0064's trigger refuses a
    // further event, and the agent is TOLD so rather than silently
    // overwriting its own answer.
    let (feilet, svar) = kall(
        &server,
        "oppgave_svar",
        json!({"oppgave": oppgave.to_string(), "status": "pagar"}),
    )
    .await;
    assert!(feilet, "en avsluttet oppgave kan ikke gjenåpnes: {svar}");
    assert!(svar.contains("avsluttet"), "{svar}");

    // ---- The remaining write paths: do they EXIST? ----
    //
    // These are called with deliberately useless input, so the only
    // thing asserted is the failure's shape: 400 means the route is
    // there and the handler rejected the content, which is the drift
    // guarantee this test exists for. 404 or 405 would mean the path in
    // the tool table is wrong — and an agent reading "404" has no way
    // to tell that from "no access".
    let tomfil = skriv_midlertidig("tom.xml", b"ikke saft");
    let tomcsv = skriv_midlertidig("tom.csv", b"ingen;overskrifter\n");
    for (navn, args) in [
        (
            "bilag_avvis",
            json!({"dokument": Uuid::new_v4().to_string(), "note": "nei"}),
        ),
        (
            "faktura_opprett",
            json!({"party_no": "finnes-ikke",
            "invoice_date": "2026-03-04", "due_date": "2026-04-04",
            "lines": [{"description": "x", "unit_price_ore": 100}]}),
        ),
        ("saft_analyser", json!({"fil": tomfil})),
        ("saft_importer", json!({"fil": tomfil})),
        (
            "kontakter_importer",
            json!({"fil": tomcsv, "kind": "kunde"}),
        ),
        (
            "apne_poster_importer",
            json!({"fil": tomcsv, "kind": "kunde", "preview": true}),
        ),
    ] {
        let (feilet, svar) = kall(&server, navn, args).await;
        assert!(feilet, "{navn} skulle feilet på innholdet: {svar}");
        assert!(
            svar.contains("400"),
            "{navn}: forventet 400 (ruten finnes, innholdet er tull). \
             404/405 betyr at stien i verktøytabellen er gal: {svar}"
        );
    }

    // ---- A bad call comes back as prose the model can act on ----
    let (feilet, svar) = kall(
        &server,
        "bilag_bokfor",
        json!({
            "dokument": Uuid::new_v4().to_string(),
            "journal_code": "GL", "date": "2026-03-04", "description": "Tull",
            "lines": [{"account": "1920", "amount_ore": 100}],
        }),
    )
    .await;
    assert!(feilet);
    assert!(
        svar.contains("400") || svar.contains("404"),
        "feilen bærer statusen: {svar}"
    );
}

/// Writes a file for the upload tools to read. The MCP server takes a
/// PATH rather than base64 on purpose: it runs on the same machine as
/// the agent, and a base64 PDF through a model's context is both
/// expensive and pointless.
fn skriv_midlertidig(navn: &str, innhold: &[u8]) -> String {
    let sti = std::env::temp_dir().join(format!("regnmed-{}-{navn}", Uuid::new_v4()));
    std::fs::write(&sti, innhold).unwrap();
    sti.to_string_lossy().to_string()
}
