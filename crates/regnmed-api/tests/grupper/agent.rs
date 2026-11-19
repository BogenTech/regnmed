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

use crate::common::{
    TestIdp, give_parties_address, make_invoiceable, test_state, unique_orgnumber,
};
use serde_json::{Value, json};
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_mcp::{Server, client::Client};

/// Serves the router on an ephemeral port and returns its base URL.
/// The task is left running for the test's lifetime; the process ends
/// when the test does.
async fn serve(state: AppState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(state)).await;
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
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Agentverktoy AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
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
    for (number, name) in [("6800", "Kontorrekvisita"), ("1920", "Bankinnskudd")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }

    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    // A task to answer, so the task tools have a real target.
    let task = regnmed_db::agent_task::create_task(
        &state.pool,
        company,
        Some(integration_id),
        "Importer regnskapet fra Conta og avstem mot årsregnskapet.",
        None,
        "Eier Eiersen",
        None,
    )
    .await
    .unwrap();

    // Arguments good enough to reach the endpoint. Tools whose action
    // needs a document, a file or a party are exercised by name below
    // instead — reaching them here would mean inventing a bilag.
    let year = json!({"from": "2026-01-01", "to": "2026-12-31"});
    let argumenter: BTreeMap<&str, Value> = BTreeMap::from([
        ("selskaper", json!({})),
        ("firmaopplysninger", json!({})),
        ("kontoplan", json!({})),
        ("periodestatus", json!({})),
        ("attesteringspolicy", json!({})),
        ("bilag_liste", json!({"limit": 5})),
        ("saldobalanse", year.clone()),
        ("kontospesifikasjon", year.clone()),
        ("resultat", year.clone()),
        ("balanse", json!({"date": "2026-12-31"})),
        ("mva_rapport", json!({"year": 2026, "termin": 1})),
        ("nokkeltall", json!({"year": 2026})),
        ("kundespesifikasjon", year.clone()),
        ("leverandorspesifikasjon", year.clone()),
        ("revisjonsrapport", json!({})),
        ("innboks", json!({})),
        ("kunder", json!({})),
        ("fakturaer", json!({})),
        ("forfalte_fakturaer", json!({})),
        ("saft_logg", json!({})),
        ("oppgaver", json!({})),
        ("oppgave", json!({"oppgave": task.to_string()})),
        ("utlegg_mottakere", json!({})),
        ("utlegg_liste", json!({"status": "apne"})),
        ("prosjekter", json!({})),
        ("timer_liste", year.clone()),
        ("timer_sammendrag", year.clone()),
        ("timer_ufakturert", json!({})),
        ("bank_kontoer", json!({})),
        ("bank_avstemming", json!({"account": "1920"})),
        ("bank_poster_sok", json!({"account": "1920"})),
        ("bankkoblinger", json!({})),
    ]);

    for (name, args) in &argumenter {
        let (failed, text) = calls(&server, name, args.clone()).await;
        assert!(
            !failed,
            "{name} nådde ikke fram eller ble nektet — verktøytabellen har sklidt fra API-et: {text}"
        );
    }

    // Manual posting (#70's route, added to the table 2026-10-29 after a
    // real model could not do a bokfører's job without it: the only
    // posting tool needed an inbox document). Exercised rather than
    // exempted — it writes to the ledger, which is the one place a
    // drifted tool does damage instead of just failing.
    let (failed, text) = calls(
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
    assert!(!failed, "bilag_for nådde ikke fram: {text}");
    let posted: i64 = sqlx::query_scalar("select count(*) from voucher where company_id = $1")
        .bind(company)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(posted, 1, "bilaget havnet ikke i hovedboken");

    // ---- The bank, end to end through the tools ----
    // A statement with the 500 kr just posted (matched by the import), a
    // 200 kr payment posted outside the matching window (matched by hand),
    // and a fee with no bilag (posted from the line).
    let (failed, text) = calls(
        &server,
        "bilag_for",
        json!({
            "journal_code": "GL", "date": "2026-03-01", "description": "Kontorrekvisita",
            "lines": [{"account": "6800", "amount_ore": 20000}, {"account": "1920", "amount_ore": -20000}]
        }),
    )
    .await;
    assert!(!failed, "{text}");
    regnmed_db::ensure_account(&state.pool, company, "7770", "Bankgebyr")
        .await
        .unwrap();
    let csv = "Dato;Forklaring;Beløp\n15.03.2026;Kontorrekvisita;-500,00\n\
               20.03.2026;Kontorrekvisita;-200,00\n31.03.2026;Gebyr;-35,00\n";
    let (failed, text) = calls(
        &server,
        "bank_importer",
        json!({"fil": skriv_midlertidig("utdrag.csv", csv.as_bytes()), "account": "1920"}),
    )
    .await;
    assert!(!failed, "bank_importer nådde ikke fram: {text}");
    let imported: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(imported["transactions"], 3, "{imported}");
    assert_eq!(imported["auto_matched"], 1, "{imported}");

    let (_, text) = calls(&server, "bank_avstemming", json!({"account": "1920"})).await;
    let recon: Value = serde_json::from_str(&text).unwrap();
    let line = |ore: i64| {
        recon["unmatched_bank"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["amount_ore"] == ore)
            .unwrap_or_else(|| panic!("ingen banklinje på {ore}: {recon}"))["bank_transaction_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let entry = recon["unmatched_entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["amount_ore"] == -20000)
        .unwrap_or_else(|| panic!("ingen bilagslinje på 200 kr: {recon}"))["entry_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (failed, text) = calls(
        &server,
        "bank_koble",
        json!({"bank_transaction_id": line(-20000), "entry_id": entry}),
    )
    .await;
    assert!(!failed, "bank_koble nådde ikke fram: {text}");
    let (failed, text) = calls(
        &server,
        "bank_bokfor_linje",
        json!({"linje": line(-3500), "konto": "7770"}),
    )
    .await;
    assert!(!failed, "bank_bokfor_linje nådde ikke fram: {text}");
    let (_, text) = calls(&server, "bank_avstemming", json!({"account": "1920"})).await;
    let recon: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(recon["unmatched_bank"], json!([]), "{recon}");

    // No provider in this installation: the route answers, and says so.
    let (failed, text) = calls(
        &server,
        "bank_hent",
        json!({"kobling": Uuid::new_v4().to_string(), "fra": "2026-01-01"}),
    )
    .await;
    assert!(failed && text.contains("ikke satt opp"), "{text}");

    // ---- Projects and hours, end to end through the tools ----
    //
    // «før 8 timer på utvikling man–fre» is the request that found this
    // gap: the employee prompt promised hours and the table had no way
    // to log them. Every write here is real, because a drifted field
    // name (`minutter` vs `minutes`) would be a 422 the agent cannot read.
    let ok = |name: &'static str, reply: &(bool, String)| {
        assert!(!reply.0, "{name} nådde ikke fram: {}", reply.1);
        serde_json::from_str::<Value>(&reply.1).unwrap_or(Value::Null)
    };
    ok(
        "prosjekt_opprett",
        &calls(
            &server,
            "prosjekt_opprett",
            json!({"kind": "prosjekt", "code": "UTV", "name": "Utvikling"}),
        )
        .await,
    );
    ok(
        "prosjekt_endre",
        &calls(
            &server,
            "prosjekt_endre",
            json!({"kind": "prosjekt", "kode": "UTV", "fakturerbar_default": true}),
        )
        .await,
    );
    ok(
        "prosjekt_sats_sett",
        &calls(
            &server,
            "prosjekt_sats_sett",
            json!({"prosjekt": "UTV", "timesats_ore": 120000, "valid_from": "2026-01-01"}),
        )
        .await,
    );
    let satser = ok(
        "prosjekt_satser",
        &calls(&server, "prosjekt_satser", json!({"prosjekt": "UTV"})).await,
    );
    assert_eq!(satser["satser"][0]["timesats_ore"], 120000, "{satser}");
    for day in 2..=6 {
        ok(
            "timer_registrer",
            &calls(
                &server,
                "timer_registrer",
                json!({"dato": format!("2026-03-0{day}"), "minutter": 480,
                       "prosjekt": "UTV", "beskrivelse": "Utvikling"}),
            )
            .await,
        );
    }
    let week = json!({"from": "2026-03-02", "to": "2026-03-08"});
    let listed = ok(
        "timer_liste",
        &calls(&server, "timer_liste", week.clone()).await,
    );
    let entries = listed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 5, "{listed}");
    // Whose hours: the agent's own. An agent is its own person (#101),
    // and the API writes for the person the token proves.
    let (owners, kind): (i64, String) = sqlx::query_as(
        "select count(distinct t.person_id), min(p.kind)
         from time_entry t join person p on p.id = t.person_id
         where t.company_id = $1",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!((owners, kind.as_str()), (1, "integrasjon"));
    assert!(
        entries
            .iter()
            .all(|e| e["own"] == true && e["fakturerbar"] == true)
    );
    let first = entries[0]["entry_id"].as_str().unwrap().to_string();
    let last = entries[4]["entry_id"].as_str().unwrap().to_string();
    ok(
        "timer_endre",
        &calls(
            &server,
            "timer_endre",
            json!({"timeforing": first, "dato": entries[0]["dato"], "minutter": 450,
                   "prosjekt": "UTV", "beskrivelse": "Utvikling"}),
        )
        .await,
    );
    ok(
        "timer_slett",
        &calls(&server, "timer_slett", json!({"timeforing": last})).await,
    );
    let summary = ok(
        "timer_sammendrag",
        &calls(&server, "timer_sammendrag", week.clone()).await,
    );
    assert_eq!(
        summary["prosjekter"][0]["minutter"],
        450 + 3 * 480,
        "{summary}"
    );
    // The lock is real too: once set, the agent cannot log into it.
    ok(
        "timer_las",
        &calls(
            &server,
            "timer_las",
            json!({"locked_through": "2026-03-31", "note": "Lønn mars"}),
        )
        .await,
    );
    let (failed, reply) = calls(
        &server,
        "timer_registrer",
        json!({"dato": "2026-03-09", "minutter": 60, "prosjekt": "UTV"}),
    )
    .await;
    assert!(failed, "en låst dag tar ikke imot timer: {reply}");

    // Billing is the one hours tool that writes to the ledger, so it is
    // exercised for real: locked hours may still be billed (docs/timer.md).
    let (_, customer) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde AS", None, None)
            .await
            .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    give_parties_address(&state.pool, company).await;
    let unbilled = ok(
        "timer_ufakturert",
        &calls(&server, "timer_ufakturert", json!({"prosjekt": "UTV"})).await,
    );
    let ids = &unbilled["groups"][0]["personer"][0]["entry_ids"];
    assert_eq!(ids.as_array().map(Vec::len), Some(4), "{unbilled}");
    let billed = ok(
        "timer_fakturer",
        &calls(
            &server,
            "timer_fakturer",
            json!({"party_no": customer, "prosjekt": "UTV", "entry_ids": ids,
                   "invoice_date": "2026-03-31"}),
        )
        .await,
    );
    // 31,5 t × 1 200 kr at the register's rate (the agent sent none),
    // plus 25 % mva: 37 800 + 9 450 kr.
    assert_eq!(billed["gross_ore"], 4_725_000, "{billed}");

    // Every READ tool in the table must be covered above. A tool added
    // without a line here is a tool nothing checks.
    let mut covered: Vec<&str> = argumenter.keys().copied().collect();
    covered.extend([
        "bilag_for",
        "prosjekt_opprett",
        "prosjekt_endre",
        "prosjekt_sats_sett",
        "prosjekt_satser",
        "timer_registrer",
        "timer_endre",
        "timer_slett",
        "timer_las",
    ]);
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
        "timer_fakturer",
        "oppgave_svar",
        // In an_agent_files_a_claim_and_a_human_decides_it.
        "utlegg_fra_innboks",
        "utlegg_registrer",
        "utlegg_kvittering",
        "utlegg_forslag",
        "utlegg_oppdater",
        "utlegg_send_inn",
        // In an_agent_files_a_mileage_claim_without_a_receipt.
        "kjoring_registrer",
        "kjoring_sats",
        // In an_agent_reads_only_listed_web_pages: the real fetch goes
        // to the internet, and this test runs offline.
        "nettside",
        // In «The bank, end to end» above.
        "bank_importer",
        "bank_koble",
        "bank_bokfor_linje",
        "bank_hent",
    ];
    for v in regnmed_mcp::tool::TOOLS {
        assert!(
            covered.contains(&v.name) || senere.contains(&v.name),
            "verktøyet «{}» er ikke dekket av denne testen",
            v.name
        );
    }

    // ---- The writing path, end to end through the tools ----
    let (failed, reply) = calls(
        &server,
        "bilag_last_opp",
        json!({"fil": skriv_midlertidig("kvittering.txt", b"Kaffe 49,00")}),
    )
    .await;
    assert!(!failed, "{reply}");
    let document = serde_json::from_str::<Value>(&reply).unwrap()["document_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (failed, reply) = calls(&server, "innboks_forslag", json!({"dokument": document})).await;
    assert!(!failed, "forslaget svarer for ethvert dokument: {reply}");

    for (number, name) in [("1920", "Bank"), ("7790", "Annen kostnad")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let (failed, reply) = calls(
        &server,
        "bilag_bokfor",
        json!({
            "dokument": document,
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
    assert!(!failed, "{reply}");

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
    let (failed, reply) = calls(
        &server,
        "oppgave_svar",
        json!({"oppgave": task.to_string(), "status": "sporsmal",
               "notat": "Kontoplanen må avstemmes mot NS 4102 — bekreft forslaget."}),
    )
    .await;
    assert!(!failed, "{reply}");
    let (failed, reply) = calls(
        &server,
        "oppgave_svar",
        json!({"oppgave": task.to_string(), "status": "utfort",
               "notat": "Importert og avstemt."}),
    )
    .await;
    assert!(!failed, "{reply}");
    // Terminal means terminal: migration 0064's trigger refuses a
    // further event, and the agent is TOLD so rather than silently
    // overwriting its own answer.
    let (failed, reply) = calls(
        &server,
        "oppgave_svar",
        json!({"oppgave": task.to_string(), "status": "pagar"}),
    )
    .await;
    assert!(failed, "en avsluttet oppgave kan ikke gjenåpnes: {reply}");
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
        ("timer_fakturer", json!({"party_no": "finnes-ikke"})),
    ] {
        let (failed, reply) = calls(&server, name, args).await;
        assert!(failed, "{name} skulle feilet på innholdet: {reply}");
        assert!(
            reply.contains("400"),
            "{name}: forventet 400 (ruten finnes, innholdet er tull). \
             404/405 betyr at stien i verktøytabellen er gal: {reply}"
        );
    }

    // ---- A bad call comes back as prose the model can act on ----
    let (failed, reply) = calls(
        &server,
        "bilag_bokfor",
        json!({
            "dokument": Uuid::new_v4().to_string(),
            "journal_code": "GL", "date": "2026-03-04", "description": "Tull",
            "lines": [{"account": "1920", "amount_ore": 100}],
        }),
    )
    .await;
    assert!(failed);
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
struct FakeSupplier {
    sett: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

async fn serve_supplier(mock: FakeSupplier, reply: Vec<Value>) -> String {
    let reply = std::sync::Arc::new(std::sync::Mutex::new(reply.into_iter()));
    let app =
        axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let mock = mock.clone();
                let reply = reply.clone();
                async move {
                    mock.sett.lock().unwrap().push(body);
                    let next = reply.lock().unwrap().next();
                    axum::Json(next.unwrap_or_else(
                        || json!({"choices":[{"message":{"content":"FERDIG: tom"}}]}),
                    ))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
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
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Vertsagent AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
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

    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let mock = FakeSupplier::default();
    let supplier = serve_supplier(
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

    // The catalogue the model was offered is the ROLE's menu: the real
    // table, less what `bokforing` does not cover (migration, the lock).
    let tools: Vec<&str> = sett[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert!(tools.contains(&"bilag_bokfor") && tools.contains(&"saldobalanse"));
    assert!(!tools.contains(&"saft_importer") && !tools.contains(&"timer_las"));
    // The worker writes the report from the closing marker; offering the
    // reporting tool too made the model post each question twice.
    assert!(!tools.contains(&"oppgave_svar"));

    // The role prompt went with it — a `bokforing` agent must not be
    // told it is something else, or it spends its turns being refused.
    let system = sett[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.starts_with(&regnmed_mcp::role::prompt("bokforer").unwrap()),
        "en bokforing-agent fikk ikke bokfører-prompten"
    );
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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Trådagent AS")
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
    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let mock = FakeSupplier::default();
    let supplier = serve_supplier(
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

/// A reply without a marker is never read as done (docs/agent.md §8.4):
/// the model is asked once to mark it, and a reply that stays unmarked
/// is a question -- the one status that is not terminal.
#[tokio::test]
async fn an_unmarked_reply_is_sent_back_to_be_marked() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Merkeagent AS")
        .await
        .unwrap();
    let client_id = format!("merkeagent-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Mona Merke",
        None,
        "bokforing",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    let reply = |text: &str| json!({"choices":[{"message":{"role":"assistant","content":text}}]});
    // Verbatim shape from nemotron, 2026-11-17: a question with no «?».
    let asked = "For å sette en timesats må jeg ha en `valid_from`-dato. Oppgi datoen";

    // The model marks it when reminded: its own marker decides.
    let mock = FakeSupplier::default();
    let supplier = serve_supplier(
        mock.clone(),
        vec![
            reply(asked),
            reply("SPØRSMÅL: Hvilken dato skal satsen gjelde fra?"),
        ],
    )
    .await;
    let model = regnmed_agent::llm::Llm::new(&supplier, "testmodell", "sk-test").unwrap();
    let outcome = regnmed_agent::run_loop(&model, &server, "bokforing", None, "Sett sats.", &[])
        .await
        .unwrap();
    assert_eq!(
        outcome,
        regnmed_agent::Outcome::Question("Hvilken dato skal satsen gjelde fra?".into())
    );
    let sett = mock.sett.lock().unwrap().clone();
    assert_eq!(sett.len(), 2, "én påminnelse, så svaret");
    let messages = sett[1]["messages"].as_array().unwrap();
    let n = messages.len();
    assert_eq!(messages[n - 2]["role"], "assistant");
    assert_eq!(
        messages[n - 2]["content"],
        asked,
        "modellen må se sitt eget svar"
    );
    assert!(
        messages[n - 1]["content"]
            .as_str()
            .unwrap()
            .contains("mangler merke"),
        "påminnelsen ble ikke sendt: {:?}",
        messages[n - 1]
    );

    // A model that ignores the reminder too gets a question, never `utfort`.
    let mock = FakeSupplier::default();
    let supplier = serve_supplier(mock.clone(), vec![reply(asked), reply(asked)]).await;
    let model = regnmed_agent::llm::Llm::new(&supplier, "testmodell", "sk-test").unwrap();
    let outcome = regnmed_agent::run_loop(&model, &server, "bokforing", None, "Sett sats.", &[])
        .await
        .unwrap();
    assert_eq!(outcome, regnmed_agent::Outcome::Question(asked.into()));
    assert_eq!(mock.sett.lock().unwrap().len(), 2, "bare én påminnelse");
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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Køselskapet AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let task = regnmed_db::agent_task::create_task(
        &state.pool,
        company,
        None,
        "Avstem banken.",
        None,
        "Aud Admin",
        None,
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

/// An agent files an employee's receipt as a claim and sends it in; a
/// human decides it (docs/utlegg.md «Agenten»). Both rails: the MCP tool
/// hands the receipt to the model as a picture, and the hosted worker
/// passes that picture on to the provider. The agent's role has the
/// rights to approve, and is still refused: someone's money is decided
/// by a person.
#[tokio::test]
async fn an_agent_files_a_claim_and_a_human_decides_it() {
    use base64::Engine;
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Agentutlegg AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("7790", "Annen kostnad"), ("2910", "Gjeld til ansatte")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
        .await
        .unwrap();
    let ola = regnmed_db::ensure_person(
        &state.pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Ola Ansatt"),
        None,
    )
    .await
    .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, ola, "ansatt")
        .await
        .unwrap();
    let client_id = format!("utleggsagent-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Bokføringsagenten",
        None,
        "bokforing",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    let agent_token = idp.token(&client_id, "");
    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(&base, Some(company.to_string()), &agent_token));

    // Ola mailed the taxi receipt in; it sits in the innboks.
    let photo = [&[0xFF, 0xD8, 0xFF, 0xE0][..], Uuid::new_v4().as_bytes()].concat();
    let (failed, reply) = calls(
        &server,
        "bilag_last_opp",
        json!({"fil": skriv_midlertidig("taxi.jpg", &photo)}),
    )
    .await;
    assert!(!failed, "{reply}");
    let document = serde_json::from_str::<Value>(&reply).unwrap()["document_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (failed, reply) = calls(&server, "utlegg_mottakere", json!({})).await;
    assert!(!failed && reply.contains("Ola Ansatt"), "{reply}");
    // Without a person the claim would be the agent's own: refused.
    let (failed, reply) = calls(
        &server,
        "utlegg_registrer",
        json!({"fil": skriv_midlertidig("egen.jpg", Uuid::new_v4().as_bytes())}),
    )
    .await;
    assert!(failed && reply.contains("person_id"), "{reply}");
    let (failed, reply) = calls(
        &server,
        "utlegg_fra_innboks",
        json!({"dokument": document, "person_id": ola.to_string()}),
    )
    .await;
    assert!(!failed, "{reply}");
    let claim = serde_json::from_str::<Value>(&reply).unwrap()["expense_id"]
        .as_str()
        .unwrap()
        .to_string();
    // The document left the innboks for the claim, one way.
    let (status, linked): (String, Option<Uuid>) =
        sqlx::query_as("select status, expense_id from inbox_document where id = $1")
            .bind(Uuid::parse_str(&document).unwrap())
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(status, "utlegg");
    assert_eq!(linked.unwrap().to_string(), claim);
    let (failed, _) = calls(
        &server,
        "utlegg_fra_innboks",
        json!({"dokument": document, "person_id": ola.to_string()}),
    )
    .await;
    assert!(failed, "a document becomes one claim");

    // The model SEES the receipt: the picture itself, byte for byte.
    let picture = server
        .handle(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "utlegg_kvittering", "arguments": { "utlegg": claim } },
        }))
        .await
        .unwrap();
    let content = picture["result"]["content"].as_array().unwrap();
    assert_eq!(content[1]["type"], "image", "{picture}");
    assert_eq!(content[1]["mimeType"], "image/jpeg");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(content[1]["data"].as_str().unwrap())
            .unwrap(),
        photo
    );
    let (failed, _) = calls(&server, "utlegg_forslag", json!({"utlegg": claim})).await;
    assert!(!failed);

    // Filled in and sent in by the agent.
    let (failed, reply) = calls(
        &server,
        "utlegg_oppdater",
        json!({"utlegg": claim, "dato": "2026-03-10", "belop_ore": 34_900, "beskrivelse": "Taxi til kundemøte"}),
    )
    .await;
    assert!(!failed, "{reply}");
    let (failed, reply) = calls(&server, "utlegg_send_inn", json!({"utlegg": claim})).await;
    assert!(!failed, "{reply}");
    let (failed, reply) = calls(&server, "utlegg_liste", json!({"status": "apne"})).await;
    assert!(!failed && reply.contains(&claim), "{reply}");

    // A PDF is not shown as a picture; the tool says what to use instead.
    let (failed, reply) = calls(
        &server,
        "utlegg_registrer",
        json!({"fil": skriv_midlertidig("hotell.pdf", format!("%PDF-1.4 {}", Uuid::new_v4()).as_bytes()), "person_id": ola.to_string()}),
    )
    .await;
    assert!(!failed, "{reply}");
    let pdf_claim = serde_json::from_str::<Value>(&reply).unwrap()["expense_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pdf = server
        .handle(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "utlegg_kvittering", "arguments": { "utlegg": pdf_claim } },
        }))
        .await
        .unwrap();
    assert_eq!(pdf["result"]["content"].as_array().unwrap().len(), 1);
    assert!(
        pdf["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("utlegg_forslag")
    );

    // The agent's role may approve claims — and is refused anyway.
    let http = reqwest::Client::new();
    let refused = http
        .post(format!(
            "{base}/companies/{company}/expenses/{claim}/approve"
        ))
        .bearer_auth(&agent_token)
        .json(&json!({"konto": "7790"}))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 400);
    assert!(refused.text().await.unwrap().contains("menneske"));
    let refused = http
        .post(format!(
            "{base}/companies/{company}/expenses/{claim}/reject"
        ))
        .bearer_auth(&agent_token)
        .json(&json!({"note": "nei"}))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 403);
    // A person decides it.
    let approved = http
        .post(format!(
            "{base}/companies/{company}/expenses/{claim}/approve"
        ))
        .bearer_auth(idp.token(&owner_sub, "Eier Eiersen"))
        .json(&json!({"konto": "7790"}))
        .send()
        .await
        .unwrap();
    assert_eq!(approved.status(), 200, "{}", approved.text().await.unwrap());

    // The hosted rail: the worker hands the same picture to the provider.
    let mock = FakeSupplier::default();
    let supplier = serve_supplier(
        mock.clone(),
        vec![
            json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":"call_1","type":"function","function":{
                    "name":"utlegg_kvittering",
                    "arguments": json!({"utlegg": claim}).to_string()
            }}]}}]}),
            json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":"call_2","type":"function","function":{
                    "name":"utlegg_kvittering",
                    "arguments": json!({"utlegg": claim}).to_string()
            }}]}}]}),
            json!({"choices":[{"message":{"role":"assistant","content":"FERDIG: lest"}}]}),
        ],
    )
    .await;
    let model = regnmed_agent::llm::Llm::new(&supplier, "testmodell", "sk-test").unwrap();
    regnmed_agent::run_loop(&model, &server, "bokforing", None, "Les kvitteringen.", &[])
        .await
        .unwrap();
    let sett = mock.sett.lock().unwrap().clone();
    let messages = sett[1]["messages"].as_array().unwrap();
    let last = messages.last().unwrap();
    assert_eq!(last["role"], "user", "the picture follows the tool results");
    let url = last["content"][1]["image_url"]["url"].as_str().unwrap();
    assert!(url.starts_with("data:image/jpeg;base64,"), "{url}");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&url["data:image/jpeg;base64,".len()..])
            .unwrap(),
        photo
    );
    // Read twice: only the newest picture rides along, the older one is
    // a note — every turn resends the conversation.
    let pictures = sett[2]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["content"].is_array())
        .count();
    assert_eq!(pictures, 1, "{}", sett[2]);
}

/// A model that reports through `oppgave_svar` anyway (the role prompt
/// it shares with external agents tells it to) is turned away without
/// touching the trail, and its marked reply is the one report. In test
/// 2026-11-19 every such turn posted the question twice.
#[tokio::test]
async fn the_hosted_worker_reports_once_even_when_the_model_calls_the_report_tool() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Ettsvar AS")
        .await
        .unwrap();
    let client_id = format!("ettsvar-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Agenten",
        None,
        "bokforing",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    let mock = FakeSupplier::default();
    let supplier = serve_supplier(
        mock.clone(),
        vec![
            json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
            "id":"call_1","type":"function","function":{
                "name":"oppgave_svar",
                "arguments":"{\"oppgave\":\"00000000-0000-0000-0000-000000000000\",\"status\":\"sporsmal\",\"notat\":\"Hvor mange km?\"}"
            }}]}}]}),
            json!({"choices":[{"message":{"role":"assistant",
                "content":"SPØRSMÅL: Hvor mange km?"}}]}),
        ],
    )
    .await;
    let model = regnmed_agent::llm::Llm::new(&supplier, "testmodell", "sk-test").unwrap();
    let outcome = regnmed_agent::run_loop(
        &model,
        &server,
        "bokforing",
        None,
        "Lag en reiseregning.",
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        outcome,
        regnmed_agent::Outcome::Question("Hvor mange km?".into())
    );
    let sett = mock.sett.lock().unwrap().clone();
    let told = sett[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(told.contains("SPØRSMÅL:"), "{told}");
}

/// The web tool reads only what the platform listed, and the refusal
/// names the list so the agent can tell the human. Both refusals happen
/// before any network traffic, so this runs offline.
#[tokio::test]
async fn an_agent_reads_only_listed_web_pages() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Nettagent AS")
        .await
        .unwrap();
    let client_id = format!("nettagent-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Agenten",
        None,
        "les",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    for url in [
        "https://example.com/",
        "https://skatteetaten.no.example.com/",
        "http://www.skatteetaten.no/",
    ] {
        let (failed, reply) = calls(&server, "nettside", json!({ "url": url })).await;
        assert!(failed && reply.contains("ikke på listen"), "{url}: {reply}");
    }
}

/// A reiseregning for a drive: no receipt, no rate from the model — the
/// register's statens sats on the date — billed on to the customer's
/// project, with the toll hanging under the drive and inheriting both.
#[tokio::test]
async fn an_agent_files_a_mileage_claim_without_a_receipt() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Kjoreagent AS")
        .await
        .unwrap();
    regnmed_db::create_dimension(
        &state.pool,
        company,
        "prosjekt",
        "P1",
        "Kundeprosjekt",
        None,
    )
    .await
    .unwrap();
    let ola = regnmed_db::ensure_person(
        &state.pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Ola Ansatt"),
        None,
    )
    .await
    .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, ola, "ansatt")
        .await
        .unwrap();
    let client_id = format!("kjoreagent-{}", Uuid::new_v4());
    regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Bokføringsagenten",
        None,
        "bokforing",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    let base = serve(state.clone()).await;
    let server = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));

    let (failed, reply) = calls(&server, "kjoring_sats", json!({"dato": "2026-03-10"})).await;
    assert!(!failed, "{reply}");
    let rate = serde_json::from_str::<Value>(&reply).unwrap()["sats_ore_per_km"]
        .as_i64()
        .unwrap();

    let (failed, reply) = calls(
        &server,
        "kjoring_registrer",
        json!({"kind": "kjoring", "person_id": ola.to_string(), "dato": "2026-03-10",
               "km": 200, "route_from": "Drammen", "route_to": "Oslo", "round_trip": true,
               "beskrivelse": "Butikkbesøk", "project": "P1", "billable": true}),
    )
    .await;
    assert!(!failed, "{reply}");
    let drive = serde_json::from_str::<Value>(&reply).unwrap()["expense_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (failed, reply) = calls(
        &server,
        "utlegg_registrer",
        json!({"fil": skriv_midlertidig("bom.jpg", &[&[0xFF, 0xD8, 0xFF, 0xE0][..], Uuid::new_v4().as_bytes()].concat()),
               "trip_expense_id": drive}),
    )
    .await;
    assert!(!failed, "{reply}");
    let toll: Uuid = serde_json::from_str::<Value>(&reply).unwrap()["expense_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let (person, project, billable): (Uuid, Option<String>, bool) =
        sqlx::query_as("select person_id, project, billable from expense where id = $1")
            .bind(toll)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        (person, project.as_deref(), billable),
        (ola, Some("P1"), true),
        "the toll belongs to the drive's person and project"
    );
    let (failed, reply) = calls(
        &server,
        "utlegg_oppdater",
        json!({"utlegg": toll.to_string(), "dato": "2026-03-10", "belop_ore": 7_500,
               "beskrivelse": "Bompenger", "project": "P1", "billable": true}),
    )
    .await;
    assert!(!failed, "{reply}");

    let (failed, reply) = calls(&server, "utlegg_send_inn", json!({"utlegg": drive})).await;
    assert!(!failed, "{reply}");
    let done = serde_json::from_str::<Value>(&reply).unwrap();
    assert_eq!(done["belop_ore"], 200 * rate, "{done}");
    assert_eq!(done["costs_submitted"], 1, "{done}");
}

/// Each tool declares the right its endpoint checks, and an agent is
/// only offered the tools whose right it holds (`Server::offered_tools`).
/// A wrong declaration either hides a tool the role could use or offers
/// one it will be refused -- and nothing else notices, because the API
/// still decides every call.
///
/// So the declaration is checked against the real guard, per built-in
/// role a machine can hold: EVERY tool that declares a right is called,
/// writes included, straight past the menu (`execute_tool`), and a hidden
/// tool must be refused with a 403 naming exactly the declared right,
/// while an offered one must not be. The arguments are only
/// well-formed -- random ids, an unknown project -- because axum runs the
/// body and query extractors BEFORE the guard (docs/auth.md): a body that
/// does not parse would be a 400 for everyone and measure nothing. What
/// an offered write then does (a 404 on the random id, a 400 on the
/// unknown project, or real work in this throwaway company) is beside
/// the point.
#[tokio::test]
async fn each_role_is_offered_exactly_the_tools_its_guard_lets_through() {
    for t in regnmed_mcp::tool::TOOLS {
        if let Some(r) = t.right {
            assert!(
                regnmed_api::access::Right::from_slug(r).is_some(),
                "{}: «{r}» er ingen rettighet",
                t.name
            );
        }
    }

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Verktoymeny AS")
        .await
        .unwrap();
    let base = serve(state.clone()).await;
    let any_id = Uuid::new_v4().to_string();
    let csv = skriv_midlertidig("menu.csv", b"Navn;Orgnr\nKunde AS;\n");
    let well_formed = json!({
        // reads
        "from": "2026-01-01", "to": "2026-12-31", "date": "2026-12-31", "year": 2026, "termin": 1,
        // ids in paths
        "dokument": any_id, "utlegg": any_id, "timeforing": any_id, "oppgave": any_id,
        "linje": any_id, "kobling": any_id, "account": "1920",
        "prosjekt": "MENY", "kind": "prosjekt", "kode": "MENY",
        // files
        "fil": csv,
        // bodies
        "journal_code": "GL", "description": "x", "beskrivelse": "x", "note": "x",
        "lines": [], "code": "MENY", "name": "Meny",
        "party_no": "10000", "invoice_date": "2026-03-02", "due_date": "2026-03-16",
        "person_id": any_id, "dato": "2026-03-02", "belop_ore": 100, "minutter": 60,
        "timesats_ore": 100, "locked_through": "2025-12-31",
        "bank_transaction_id": any_id, "entry_id": any_id, "konto": "7770",
    });

    for role in ["ansatt", "les", "bokforing", "admin"] {
        let client_id = format!("agent-{role}-{}", Uuid::new_v4());
        let integration_id = regnmed_db::grant_integration(
            &state.pool,
            company,
            &client_id,
            &format!("Agent {role}"),
            None,
            role,
            "Eier Eiersen",
        )
        .await
        .unwrap();
        sqlx::query("update integration set rate_limit_min = 10000 where id = $1")
            .bind(integration_id)
            .execute(&state.pool)
            .await
            .unwrap();
        let server = Server::new(Client::new(
            &base,
            Some(company.to_string()),
            &idp.token(&client_id, ""),
        ));
        let offered: Vec<&str> = server
            .offered_tools()
            .await
            .iter()
            .map(|t| t.name)
            .collect();
        // The fallback (whole table when `/me` fails) must not be what
        // this test is measuring.
        assert!(
            offered.len() < regnmed_mcp::tool::TOOLS.len() || role == "admin",
            "{role}: fikk hele tabellen -- leste offered_tools /me?"
        );
        // MCP lists the same menu the worker builds its catalogue from.
        let listed = server
            .handle(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .await
            .unwrap();
        assert_eq!(
            listed["result"]["tools"].as_array().unwrap().len(),
            offered.len(),
            "{role}"
        );
        // And a tool outside the menu is refused by the menu, in words,
        // without a round trip to be told the same by the API.
        if !offered.contains(&"saft_importer") {
            let (failed, text) =
                calls(&server, "saft_importer", json!({"fil": "/finnes/ikke"})).await;
            assert!(
                failed && text.contains("hører ikke til rollen din"),
                "{role}: {text}"
            );
        }

        for t in regnmed_mcp::tool::TOOLS {
            let Some(right) = t.right else { continue };
            let mut args = well_formed.as_object().unwrap().clone();
            // Leftover arguments become the QUERY of a body-less call, and
            // a query struct reads a stray field (`lines`) as its own.
            if t.method == "GET" || t.method == "DELETE" {
                args.retain(|k, _| {
                    ["from", "to", "date", "dato", "year", "termin", "account"]
                        .contains(&k.as_str())
                        || t.path.contains(&format!("{{{k}}}"))
                });
            }
            // The imports' `kind` is the party kind, not a dimension's.
            if t.path.contains("/import/") {
                args.insert("kind".into(), json!("kunde"));
            }
            let text = server.execute_tool(t, args).await.text;
            let guard_said = |slug: &str| {
                text.contains("svarte 403") && text.contains(&format!(r#""error":"{slug}""#))
            };
            if offered.contains(&t.name) {
                // Only the declared right: a SECOND, argument-dependent
                // check (a claim for another person needs UTLEGG_GODKJENN)
                // is the endpoint's own, and the random ids trip it.
                assert!(
                    !guard_said(right),
                    "{role}: {} tilbys (krever {right}) men vakten nektet: {text}",
                    t.name
                );
            } else {
                assert!(
                    guard_said(right),
                    "{role}: {} skjules (krever {right}), men vakten sa noe annet: {text}",
                    t.name
                );
            }
        }
    }
}

/// An agent reaches its OWN queue on company access alone. Until
/// 2026-11-17 reading and answering took `INTEGRASJON_LES`, which the
/// `ansatt` bundle lacks, so an employee agent over MCP could not see
/// what it had been asked to do. Pinned both ways: the narrow agent now
/// gets its own task, still never another agent's, and a HUMAN with the
/// same narrow role is still refused the queue.
#[tokio::test]
async fn an_agent_reaches_its_own_tasks_whatever_its_role() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Egen ko AS")
        .await
        .unwrap();
    let base = serve(state.clone()).await;

    let mut agents = Vec::new();
    for name in ["Timeagent", "Annen agent"] {
        let client_id = format!("agent-{}", Uuid::new_v4());
        let id = regnmed_db::grant_integration(
            &state.pool,
            company,
            &client_id,
            name,
            None,
            "ansatt",
            "Eier Eiersen",
        )
        .await
        .unwrap();
        let task = regnmed_db::agent_task::create_task(
            &state.pool,
            company,
            Some(id),
            &format!("Oppgave til {name}"),
            None,
            "Eier Eiersen",
            None,
        )
        .await
        .unwrap();
        let server = Server::new(Client::new(
            &base,
            Some(company.to_string()),
            &idp.token(&client_id, ""),
        ));
        agents.push((server, task));
    }
    let (me, mine) = &agents[0];
    let (_, theirs) = &agents[1];

    let offered: Vec<&str> = me.offered_tools().await.iter().map(|t| t.name).collect();
    assert!(offered.contains(&"oppgaver") && offered.contains(&"oppgave_svar"));

    let (failed, text) = calls(me, "oppgaver", json!({})).await;
    assert!(!failed, "{text}");
    assert!(text.contains(&mine.to_string()), "{text}");
    assert!(
        !text.contains(&theirs.to_string()),
        "så en annen agents oppgave: {text}"
    );

    let (failed, text) = calls(me, "oppgave", json!({"oppgave": theirs.to_string()})).await;
    assert!(failed && text.contains("svarte 404"), "{text}");
    let (failed, text) = calls(
        me,
        "oppgave_svar",
        json!({"oppgave": theirs.to_string(), "status": "utfort", "notat": "ikke min"}),
    )
    .await;
    assert!(failed && text.contains("svarte 404"), "{text}");

    let (failed, text) = calls(
        me,
        "oppgave_svar",
        json!({"oppgave": mine.to_string(), "status": "utfort", "notat": "Timene er ført."}),
    )
    .await;
    assert!(!failed, "{text}");

    // A person with the same narrow role is not an agent answering its
    // own orders, and still needs INTEGRASJON_LES.
    let sub = format!("test|{}", Uuid::new_v4());
    let ola = regnmed_db::ensure_person(&state.pool, &sub, Some("Ola Ansatt"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, ola, "ansatt")
        .await
        .unwrap();
    let human = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&sub, ""),
    ));
    let (failed, text) = calls(&human, "oppgaver", json!({})).await;
    assert!(failed && text.contains("svarte 403"), "{text}");

    // An agent whose role grants nothing -- how an owner cuts one off by
    // deactivating its custom role -- does not keep its queue.
    let client_id = format!("agent-{}", Uuid::new_v4());
    let off = regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Avskrudd agent",
        None,
        "avskrudd",
        "Eier Eiersen",
    )
    .await
    .unwrap();
    regnmed_db::agent_task::create_task(
        &state.pool,
        company,
        Some(off),
        "Les denne",
        None,
        "Eier Eiersen",
        None,
    )
    .await
    .unwrap();
    let off = Server::new(Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    let (failed, text) = calls(&off, "oppgaver", json!({})).await;
    assert!(failed && text.contains("svarte 404"), "{text}");
}
