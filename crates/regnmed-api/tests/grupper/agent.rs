//! Agenten som ansatt eller regnskapsfører (#101, #102, #103).
//!
//! This is the end-to-end test the MCP server's tool table promises:
//! `crates/regnmed-mcp/src/tools.rs` is CURATED rather than generated
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

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnr};
use serde_json::{Value, json};
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_mcp::{Server, client::Client};

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

async fn calls(server: &Server, name: &str, args: Value) -> (bool, String) {
    let reply = server
        .handle(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        }))
        .await
        .expect("et kall med id får alltid svar");
    assert!(
        reply.get("error").is_none(),
        "{name}: et verktøykall skal aldri bli en protokollfeil — modellen ser den ikke: {reply}"
    );
    let innhold = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    (
        reply["result"]["isError"].as_bool().unwrap_or(false),
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
    make_invoiceable(&state.pool, company).await;
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
    for (nr, name) in [("6800", "Kontorrekvisita"), ("1920", "Bankinnskudd")] {
        regnmed_db::ensure_account(&state.pool, company, nr, name)
            .await
            .unwrap();
    }

    let base = tjen(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    // A task to answer, so the task tools have a real target.
    let task = regnmed_db::agent_task::opprett_oppgave(
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
        ("oppgave", json!({"oppgave": task.to_string()})),
    ]);

    for (name, args) in &argumenter {
        let (feilet, text) = calls(&server, name, args.clone()).await;
        assert!(
            !feilet,
            "{name} nådde ikke fram eller ble nektet — verktøytabellen har sklidt fra API-et: {text}"
        );
    }

    // Manual posting (#70's route, added to the table 2026-10-29 after a
    // real model could not do a bokfører's job without it: the only
    // posting tool needed an inbox document). Exercised rather than
    // exempted — it writes to the ledger, which is the one place a
    // drifted tool does damage instead of just failing.
    let (feilet, text) = calls(
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
    assert!(!feilet, "bilag_for nådde ikke fram: {text}");
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
    for v in regnmed_mcp::tool::TOOLS {
        assert!(
            dekket.contains(&v.name) || senere.contains(&v.name),
            "verktøyet «{}» er ikke dekket av denne testen",
            v.name
        );
    }

    // ---- The writing path, end to end through the tools ----
    let (feilet, reply) = calls(
        &server,
        "bilag_last_opp",
        json!({"fil": skriv_midlertidig("kvittering.txt", b"Kaffe 49,00")}),
    )
    .await;
    assert!(!feilet, "{reply}");
    let dokument = serde_json::from_str::<Value>(&reply).unwrap()["document_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (feilet, reply) = calls(&server, "innboks_forslag", json!({"dokument": dokument})).await;
    assert!(!feilet, "forslaget svarer for ethvert dokument: {reply}");

    for (number, name) in [("1920", "Bank"), ("7790", "Annen kostnad")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let (feilet, reply) = calls(
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
    assert!(!feilet, "{reply}");

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
    let (feilet, reply) = calls(
        &server,
        "oppgave_svar",
        json!({"oppgave": task.to_string(), "status": "sporsmal",
               "notat": "Kontoplanen må avstemmes mot NS 4102 — bekreft forslaget."}),
    )
    .await;
    assert!(!feilet, "{reply}");
    let (feilet, reply) = calls(
        &server,
        "oppgave_svar",
        json!({"oppgave": task.to_string(), "status": "utfort",
               "notat": "Importert og avstemt."}),
    )
    .await;
    assert!(!feilet, "{reply}");
    // Terminal means terminal: migration 0064's trigger refuses a
    // further event, and the agent is TOLD so rather than silently
    // overwriting its own answer.
    let (feilet, reply) = calls(
        &server,
        "oppgave_svar",
        json!({"oppgave": task.to_string(), "status": "pagar"}),
    )
    .await;
    assert!(feilet, "en avsluttet oppgave kan ikke gjenåpnes: {reply}");
    assert!(reply.contains("avsluttet"), "{reply}");

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
    for (name, args) in [
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
        let (feilet, reply) = calls(&server, name, args).await;
        assert!(feilet, "{name} skulle feilet på innholdet: {reply}");
        assert!(
            reply.contains("400"),
            "{name}: forventet 400 (ruten finnes, innholdet er tull). \
             404/405 betyr at stien i verktøytabellen er gal: {reply}"
        );
    }

    // ---- A bad call comes back as prose the model can act on ----
    let (feilet, reply) = calls(
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
        reply.contains("400") || reply.contains("404"),
        "feilen bærer statusen: {reply}"
    );
}

/// Writes a file for the upload tools to read. The MCP server takes a
/// PATH rather than base64 on purpose: it runs on the same machine as
/// the agent, and a base64 PDF through a model's context is both
/// expensive and pointless.
fn skriv_midlertidig(name: &str, innhold: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("regnmed-{}-{name}", Uuid::new_v4()));
    std::fs::write(&path, innhold).unwrap();
    path.to_string_lossy().to_string()
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

async fn tjen_leverandor(mock: FalskLeverandor, reply: Vec<Value>) -> String {
    let reply = std::sync::Arc::new(std::sync::Mutex::new(reply.into_iter()));
    let app =
        axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let mock = mock.clone();
                let reply = reply.clone();
                async move {
                    mock.sett.lock().unwrap().push(body);
                    let neste = reply.lock().unwrap().next();
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
/// worker builds its tool catalogue from the same `TOOLS` table the
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
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let mock = FalskLeverandor::default();
    let supplier = tjen_leverandor(
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

    let model = regnmed_agent::llm::Llm::new(&supplier, "testmodell", "sk-test").unwrap();
    let outcome = regnmed_agent::run_loop(
        &model,
        &server,
        "bokforing",
        // The agent's own extra instructions (docs/agent.md §8.6).
        Some("Strøm bokføres på 6340."),
        "Hent saldobalansen for 2026 og avstem den.",
        &[],
    )
    .await
    .unwrap();

    assert_eq!(
        outcome,
        regnmed_agent::Outcome::Done("saldobalansen er hentet og avstemt".into()),
        "sluttmarkøren avgjør statusen oppgavekøen får"
    );

    let sett = mock.sett.lock().unwrap().clone();
    assert_eq!(sett.len(), 2, "to turer: ett verktøykall, så svaret");

    // The catalogue the model was offered is the real one.
    let tools = sett[0]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), regnmed_mcp::tool::TOOLS.len());

    // The role prompt went with it — a `bokforing` agent must not be
    // told it is something else, or it spends its turns being refused.
    let system = sett[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("FERDIG:") && system.contains("SPØRSMÅL:"),
        "avslutningsreglene mangler i systemprompten"
    );
    // And so did this agent's own instructions — between the role and
    // the protocol, so they specialise without displacing either.
    let role = system
        .find("Strøm bokføres på 6340.")
        .expect("instruksene mangler");
    assert!(
        role < system.find("FERDIG:").unwrap(),
        "instruksene må stå FØR avslutningsreglene: {system}"
    );

    // THE assertion: the second turn carries a tool result that came
    // from the real endpoint, not from the loop's imagination. An
    // unauthorised or missing route would show up here as an error text.
    let messages = sett[1]["messages"].as_array().unwrap();
    let tool_response = messages
        .iter()
        .find(|m| m["role"] == "tool")
        .expect("verktøysvaret er med videre");
    assert_eq!(tool_response["tool_call_id"], "call_1");
    let text = tool_response["content"].as_str().unwrap();
    assert!(
        text.contains("linjer") || text.contains("konto") || text.starts_with('{'),
        "verktøyet svarte ikke med data fra API-et: {text}"
    );
    assert!(
        !text.contains("404") && !text.to_lowercase().contains("not found"),
        "verktøyet traff en rute som ikke finnes: {text}"
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
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let mock = FalskLeverandor::default();
    let supplier = tjen_leverandor(
        mock.clone(),
        vec![json!({"choices":[{"message":{"role":"assistant",
            "content":"FERDIG: brukte svaret ditt"}}]})],
    )
    .await;
    let model = regnmed_agent::llm::Llm::new(&supplier, "testmodell", "sk-test").unwrap();

    let history = vec![
        regnmed_agent::HistoryLine {
            from_agent: true,
            text: "Hvilken konto skal kontorrekvisita på?".into(),
        },
        regnmed_agent::HistoryLine {
            from_agent: false,
            text: "Bruk 6800.".into(),
        },
    ];
    regnmed_agent::run_loop(
        &model,
        &server,
        "bokforing",
        None,
        "Bokfør kontorrekvisita.",
        &history,
    )
    .await
    .unwrap();

    let sett = mock.sett.lock().unwrap().clone();
    let messages = sett[0]["messages"].as_array().unwrap();
    // system, order, the agent's own question, the human's answer.
    assert_eq!(messages.len(), 4, "hele tråden skal være med: {messages:?}");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "Bokfør kontorrekvisita.");
    assert_eq!(
        messages[2]["role"], "assistant",
        "agentens eget spørsmål må komme tilbake som agentens, ikke som brukerens"
    );
    assert_eq!(
        messages[2]["content"],
        "Hvilken konto skal kontorrekvisita på?"
    );
    assert_eq!(messages[3]["role"], "user");
    assert_eq!(messages[3]["content"], "Bruk 6800.");
}

/// Exactly one worker gets a task, however many ask at once
/// (docs/agent.md §8.7).
///
/// This is the guard that replaces `concurrencyPolicy: Forbid`. The old
/// CronJob could not run two tasks at all, which was a capacity ceiling
/// of one for the whole platform but did mean nothing could race. With
/// N workers on a queue that accident is gone, and this takes its place
/// — because the thing two workers would race on is posting vouchers.
#[tokio::test]
async fn only_one_worker_can_hold_a_task() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Aud Admin"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Køselskapet AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let task = regnmed_db::agent_task::opprett_oppgave(
        &state.pool,
        company,
        None,
        "Avstem banken.",
        None,
        "Aud Admin",
    )
    .await
    .unwrap();

    // Ten workers reaching for the same task in the same instant. The
    // claim is one statement, so no interleaving of readers can let two
    // of them both conclude they won.
    let forsok = futures_util::future::join_all((0..10).map(|i| {
        let pool = state.pool.clone();
        async move {
            regnmed_db::agent_task::claim_task(&pool, task, &format!("worker-{i}"))
                .await
                .unwrap()
        }
    }))
    .await;
    assert_eq!(
        forsok.iter().filter(|v| **v).count(),
        1,
        "nøyaktig én vinner: {forsok:?}"
    );

    // And a second pass while the first holder is still working gets
    // nothing — this is the redelivery case, where JetStream hands the
    // nudge out again after ack_wait on a task that is still running.
    assert!(
        !regnmed_db::agent_task::claim_task(&state.pool, task, "sent-ute")
            .await
            .unwrap(),
        "en levende lås slipper ingen andre inn"
    );

    // Releasing opens it again: a failed run must not lock the task out
    // of a retry, so the lease is dropped whatever the outcome was.
    regnmed_db::agent_task::release_task(&state.pool, task)
        .await
        .unwrap();
    assert!(
        regnmed_db::agent_task::claim_task(&state.pool, task, "neste")
            .await
            .unwrap(),
        "etter slipp kan neste worker ta den"
    );

    // A claim older than its lifetime is abandoned, not permanent: a
    // worker killed mid-task would otherwise park its task forever.
    sqlx::query(
        "update agent_task_claim set claimed_at = now() - ($2 || ' minutes')::interval
         where oppgave_id = $1",
    )
    .bind(task)
    .bind((regnmed_db::agent_task::CLAIM_TTL_MINUTES + 1).to_string())
    .execute(&state.pool)
    .await
    .unwrap();
    assert!(
        regnmed_db::agent_task::claim_task(&state.pool, task, "overtar")
            .await
            .unwrap(),
        "en forlatt lås kan overtas"
    );
}
