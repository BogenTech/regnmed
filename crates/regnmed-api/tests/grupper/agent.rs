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
    // A chart to post against: `bilag_for` is exercised for real below,
    // and without accounts it would fail for a reason that says nothing
    // about whether the tool matches the API.
    for (nr, navn) in [("6800", "Kontorrekvisita"), ("1920", "Bankinnskudd")] {
        regnmed_db::ensure_account(&state.pool, company, nr, navn)
            .await
            .unwrap();
    }

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

    // Manual posting (#70's route, added to the table 2026-10-29 after a
    // real model could not do a bokfører's job without it: the only
    // posting tool needed an inbox document). Exercised rather than
    // exempted — it writes to the ledger, which is the one place a
    // drifted tool does damage instead of just failing.
    let (feilet, tekst) = kall(
        &server,
        "bilag_for",
        json!({
            "journal_code": "GL",
            "date": "2026-03-15",
            "description": "Kontorrekvisita",
            "lines": [
                {"account": "6800", "amount_ore": 50000},
                {"account": "1920", "amount_ore": -50000}
            ]
        }),
    )
    .await;
    assert!(!feilet, "bilag_for nådde ikke fram: {tekst}");
    let bokfort: i64 = sqlx::query_scalar("select count(*) from voucher where company_id = $1")
        .bind(company)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(bokfort, 1, "bilaget havnet ikke i hovedboken");

    // Every READ tool in the table must be covered above. A tool added
    // without a line here is a tool nothing checks.
    let mut dekket: Vec<&str> = argumenter.keys().copied().collect();
    dekket.push("bilag_for");
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

// ---- Vertsagenten: hele sløyfen mot en falsk leverandør (§8.3) ----

/// A stand-in for the customer's model provider. It answers
/// `/chat/completions` from a script and records what it was sent, so
/// the test can assert that the tool actually ran against the real API
/// rather than that the loop merely claimed it did.
#[derive(Clone, Default)]
struct FalskLeverandor {
    sett: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

async fn tjen_leverandor(mock: FalskLeverandor, svar: Vec<Value>) -> String {
    let svar = std::sync::Arc::new(std::sync::Mutex::new(svar.into_iter()));
    let app =
        axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(kropp): axum::Json<Value>| {
                let mock = mock.clone();
                let svar = svar.clone();
                async move {
                    mock.sett.lock().unwrap().push(kropp);
                    let neste = svar.lock().unwrap().next();
                    axum::Json(neste.unwrap_or_else(
                        || json!({"choices":[{"message":{"content":"FERDIG: tom"}}]}),
                    ))
                }
            }),
        );
    let lytter = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = lytter.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(lytter, app).await;
    });
    format!("http://{addr}")
}

/// The hosted rail end to end: a model asks for a tool, the worker runs
/// it against the REAL router with the agent's own token, feeds the
/// answer back, and records the outcome the task queue understands.
///
/// What this is really testing is that the two rails cannot drift: the
/// worker builds its tool catalogue from the same `VERKTOY` table the
/// MCP server serves, and executes through the same dispatcher.
#[tokio::test]
async fn the_hosted_worker_runs_a_real_tool_and_reports_back() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let eier_sub = format!("test|{}", Uuid::new_v4());
    let eier = regnmed_db::ensure_person(&state.pool, &eier_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Vertsagent AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, eier, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let client_id = format!("vertsagent-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Kari Bokfører",
        None,
        "bokforing",
        "Eier Eiersen",
    )
    .await
    .unwrap();

    let base = tjen(state.clone()).await;
    let server = Server::new(Klient::ny(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let mock = FalskLeverandor::default();
    let leverandor = tjen_leverandor(
        mock.clone(),
        vec![
            // Turn 1: ask for a real tool with real arguments.
            json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
            "id":"call_1","type":"function","function":{
                "name":"saldobalanse",
                "arguments":"{\"from\":\"2026-01-01\",\"to\":\"2026-12-31\"}"
            }}]}}]}),
            // Turn 2: finish.
            json!({"choices":[{"message":{"role":"assistant",
                "content":"FERDIG: saldobalansen er hentet og avstemt"}}]}),
        ],
    )
    .await;

    let modell = regnmed_agent::llm::Llm::ny(&leverandor, "testmodell", "sk-test").unwrap();
    let utfall = regnmed_agent::kjor_sloyfe(
        &modell,
        &server,
        "bokforing",
        "Hent saldobalansen for 2026 og avstem den.",
        &[],
    )
    .await
    .unwrap();

    assert_eq!(
        utfall,
        regnmed_agent::Utfall::Utfort("saldobalansen er hentet og avstemt".into()),
        "sluttmarkøren avgjør statusen oppgavekøen får"
    );

    let sett = mock.sett.lock().unwrap().clone();
    assert_eq!(sett.len(), 2, "to turer: ett verktøykall, så svaret");

    // The catalogue the model was offered is the real one.
    let verktoy = sett[0]["tools"].as_array().unwrap();
    assert_eq!(verktoy.len(), regnmed_mcp::verktoy::VERKTOY.len());

    // The role prompt went with it — a `bokforing` agent must not be
    // told it is something else, or it spends its turns being refused.
    let system = sett[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("FERDIG:") && system.contains("SPØRSMÅL:"),
        "avslutningsreglene mangler i systemprompten"
    );

    // THE assertion: the second turn carries a tool result that came
    // from the real endpoint, not from the loop's imagination. An
    // unauthorised or missing route would show up here as an error text.
    let meldinger = sett[1]["messages"].as_array().unwrap();
    let verktoysvar = meldinger
        .iter()
        .find(|m| m["role"] == "tool")
        .expect("verktøysvaret er med videre");
    assert_eq!(verktoysvar["tool_call_id"], "call_1");
    let tekst = verktoysvar["content"].as_str().unwrap();
    assert!(
        tekst.contains("linjer") || tekst.contains("konto") || tekst.starts_with('{'),
        "verktøyet svarte ikke med data fra API-et: {tekst}"
    );
    assert!(
        !tekst.contains("404") && !tekst.to_lowercase().contains("not found"),
        "verktøyet traff en rute som ikke finnes: {tekst}"
    );
}

/// An agent that asked a question and is picked up again must SEE its
/// own question and the answer. Without this it asks the same thing
/// forever, and `sporsmal` being non-terminal (0064) would buy nothing.
#[tokio::test]
async fn the_thread_is_replayed_so_an_agent_does_not_ask_twice() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Trådagent AS")
        .await
        .unwrap();
    let client_id = format!("tradagent-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Kari Bokfører",
        None,
        "bokforing",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    let base = tjen(state.clone()).await;
    let server = Server::new(Klient::ny(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let mock = FalskLeverandor::default();
    let leverandor = tjen_leverandor(
        mock.clone(),
        vec![json!({"choices":[{"message":{"role":"assistant",
            "content":"FERDIG: brukte svaret ditt"}}]})],
    )
    .await;
    let modell = regnmed_agent::llm::Llm::ny(&leverandor, "testmodell", "sk-test").unwrap();

    let historikk = vec![
        regnmed_agent::Replikk {
            fra_agenten: true,
            tekst: "Hvilken konto skal kontorrekvisita på?".into(),
        },
        regnmed_agent::Replikk {
            fra_agenten: false,
            tekst: "Bruk 6800.".into(),
        },
    ];
    regnmed_agent::kjor_sloyfe(
        &modell,
        &server,
        "bokforing",
        "Bokfør kontorrekvisita.",
        &historikk,
    )
    .await
    .unwrap();

    let sett = mock.sett.lock().unwrap().clone();
    let meldinger = sett[0]["messages"].as_array().unwrap();
    // system, order, the agent's own question, the human's answer.
    assert_eq!(
        meldinger.len(),
        4,
        "hele tråden skal være med: {meldinger:?}"
    );
    assert_eq!(meldinger[1]["role"], "user");
    assert_eq!(meldinger[1]["content"], "Bokfør kontorrekvisita.");
    assert_eq!(
        meldinger[2]["role"], "assistant",
        "agentens eget spørsmål må komme tilbake som agentens, ikke som brukerens"
    );
    assert_eq!(
        meldinger[2]["content"],
        "Hvilken konto skal kontorrekvisita på?"
    );
    assert_eq!(meldinger[3]["role"], "user");
    assert_eq!(meldinger[3]["content"], "Bruk 6800.");
}
