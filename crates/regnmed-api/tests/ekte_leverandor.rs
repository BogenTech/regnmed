//! Hand-run proving ground for the hosted agent against a REAL model
//! provider (docs/agent.md §8.3). NOT a CI test: it costs money and
//! needs a key, so it is a no-op unless OPEN_ROUTER_KEY is set.
//!
//! It exists because the end-to-end test in `grupper/agent.rs` uses a
//! fake provider whose payloads were written from the providers'
//! documentation — and the Stripe work (CLAUDE.md point 56) taught that
//! payloads written from what the code READS only confirm the code's
//! own assumption. This is the other half: a real provider, reading our
//! real tool schemas, calling our real router.
//!
//!   OPEN_ROUTER_KEY=… cargo test -p regnmed-api --test ekte_leverandor -- --nocapture
//!
//! Override the model with REGNMED_TESTMODELL.
//!
//! Its own binary rather than a module in a group, for the same reason
//! `seed_browser.rs` is: the grouping exists to stop 33 one-test
//! binaries being linked on every build, and a file that is a no-op in
//! CI and run by hand is the exception that precedent already names.

mod common;

use common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use uuid::Uuid;

#[tokio::test]
async fn a_real_model_can_use_our_tools_against_our_router() {
    let Ok(key) = std::env::var("OPEN_ROUTER_KEY") else {
        eprintln!("hopper over: OPEN_ROUTER_KEY er ikke satt");
        return;
    };
    let model_name =
        std::env::var("REGNMED_TESTMODELL").unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let eier_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &eier_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Ekte Agent AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let client_id = format!("ekte-agent-{}", Uuid::new_v4());
    let integration_id = regnmed_db::grant_integration(
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
    sqlx::query("update integration set rate_limit_min = 10000 where id = $1")
        .bind(integration_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_state = state.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, regnmed_api::router(serve_state)).await;
    });
    let base = format!("http://{addr}");

    let server = regnmed_mcp::Server::new(regnmed_mcp::client::Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    let model =
        regnmed_agent::llm::Llm::new("https://openrouter.ai/api/v1", &model_name, &key).unwrap();

    let outcome = regnmed_agent::run_loop(
        &model,
        &server,
        "bokforing",
        None,
        "Hent saldobalansen for 2026 og fortell meg kort hva den viser. \
         Hovedboken er tom, så si det hvis den er tom.",
        &[],
    )
    .await
    .expect("sløyfen skal ikke feile mot en ekte leverandør");

    println!("modell:  {model_name}");
    println!("status:  {}", outcome.status());
    println!("notat:   {}", outcome.note());

    // The point is not WHAT it concluded — a model may word an empty
    // ledger many ways — but that the whole wire held: our tool schemas
    // were accepted, a tool_call came back in the shape we parse, the
    // call reached the real router, and the answer got home.
    assert_ne!(
        outcome.status(),
        "avvist",
        "en ekte leverandør avviste hele oppgaven: {}",
        outcome.note()
    );
}

/// The other half of the promise: an agent that can do a bokfører's
/// WORK, not just read. A real model is asked to post a voucher, and
/// the ledger is read afterwards -- the only honest place to check,
/// since the model's own report is a claim.
#[tokio::test]
async fn a_real_model_can_post_a_voucher_through_the_guards() {
    let Ok(key) = std::env::var("OPEN_ROUTER_KEY") else {
        eprintln!("hopper over: OPEN_ROUTER_KEY er ikke satt");
        return;
    };
    let model_name =
        std::env::var("REGNMED_TESTMODELL").unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let eier_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &eier_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company =
        regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Bokforende Agent AS")
            .await
            .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    // A company has a chart of accounts; without these the agent is
    // right to refuse, and the test would be measuring the fixture.
    for (number, name) in [("6800", "Kontorrekvisita"), ("1920", "Bankinnskudd")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let client_id = format!("ekte-bokforer-{}", Uuid::new_v4());
    let integration_id = regnmed_db::grant_integration(
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
    sqlx::query("update integration set rate_limit_min = 10000 where id = $1")
        .bind(integration_id)
        .execute(&state.pool)
        .await
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_state = state.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, regnmed_api::router(serve_state)).await;
    });

    let server = regnmed_mcp::Server::new(regnmed_mcp::client::Client::new(
        &format!("http://{addr}"),
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    let model =
        regnmed_agent::llm::Llm::new("https://openrouter.ai/api/v1", &model_name, &key).unwrap();

    let outcome = regnmed_agent::run_loop(
        &model,
        &server,
        "bokforing",
        None,
        "Bokfør et bilag datert 2026-03-15 i journalen GL: kontorrekvisita kjøpt \
         kontant for 500 kroner. Debet konto 6800, kredit konto 1920. \
         Beløpene er uten mva. Bruk teksten «Kontorrekvisita».",
        &[],
    )
    .await
    .expect("sløyfen skal ikke feile mot en ekte leverandør");

    println!("modell:  {model_name}");
    println!("status:  {}", outcome.status());
    println!("notat:   {}", outcome.note());

    // The ledger is the only honest witness. A model that says it
    // posted and did not must fail here, which is exactly the claim an
    // agent's own report cannot settle.
    let voucher: i64 = sqlx::query_scalar("select count(*) from voucher where company_id = $1")
        .bind(company)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let debet: Option<i64> = sqlx::query_scalar(
        "select sum(amount_ore)::bigint from entry e join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.amount_ore > 0",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    println!("bilag i hovedboken: {voucher}, debetsum: {debet:?} øre");
    assert_eq!(
        voucher,
        1,
        "agenten bokførte ikke bilaget: {}",
        outcome.note()
    );
    assert_eq!(debet, Some(50_000), "feil beløp bokført");
}
