//! Hand-run proving ground for the hosted agent against a REAL model
//! provider (docs/agent.md §8.3). NOT a CI test: it costs money and
//! needs a key. Both tests are `#[ignore]`d, so an ordinary run never
//! reaches the provider even when OPEN_ROUTER_KEY is in `.env` -- the key
//! being present is not a decision to spend money; asking for the tests
//! by name is. Without the key they still do nothing.
//!
//! It exists because the end-to-end test in `grupper/agent.rs` uses a
//! fake provider whose payloads were written from the providers'
//! documentation — and the Stripe work (CLAUDE.md point 56) taught that
//! payloads written from what the code READS only confirm the code's
//! own assumption. This is the other half: a real provider, reading our
//! real tool schemas, calling our real router.
//!
//!   cargo test -p regnmed-api --test ekte_leverandor -- --ignored --nocapture
//!   cargo nextest run -p regnmed-api --test ekte_leverandor --run-ignored only
//!
//! The model is a FREE one by default (`test_model`); override it with
//! REGNMED_TESTMODELL, and a paid one also needs REGNMED_TESTMODELL_PAID=1.
//!
//! Its own binary rather than a module in a group, for the same reason
//! `seed_browser.rs` is: the grouping exists to stop 33 one-test
//! binaries being linked on every build, and a file that is a no-op in
//! CI and run by hand is the exception that precedent already names.

mod common;

use common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use uuid::Uuid;

/// The model to drive: a FREE one unless a paid one is asked for in so
/// many words. OpenRouter's free models end in `:free`. The default
/// passed both tests on 2026-11-16; `google/gemma-4-31b-it:free` was
/// rate-limited upstream, `inkling-2-small:free` and
/// `nemotron-3.5-super-vl:free` did not finish the job. The free lineup
/// changes -- if this one is gone, try others from
/// https://openrouter.ai/models?q=free with REGNMED_TESTMODELL.
fn test_model() -> String {
    let model = std::env::var("REGNMED_TESTMODELL")
        .unwrap_or_else(|_| "nvidia/nemotron-3.5-lightning:free".to_string());
    assert!(
        model.ends_with(":free") || std::env::var("REGNMED_TESTMODELL_PAID").as_deref() == Ok("1"),
        "«{model}» is not a free model; set REGNMED_TESTMODELL_PAID=1 to spend money on it"
    );
    model
}

#[tokio::test]
#[ignore = "calls a real model provider; run with --ignored"]
async fn a_real_model_can_use_our_tools_against_our_router() {
    let Ok(key) = std::env::var("OPEN_ROUTER_KEY") else {
        eprintln!("hopper over: OPEN_ROUTER_KEY er ikke satt");
        return;
    };
    let model_name = test_model();

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eier Eiersen"), None)
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
#[ignore = "calls a real model provider; run with --ignored"]
async fn a_real_model_can_post_a_voucher_through_the_guards() {
    let Ok(key) = std::env::var("OPEN_ROUTER_KEY") else {
        eprintln!("hopper over: OPEN_ROUTER_KEY er ikke satt");
        return;
    };
    let model_name = test_model();

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eier Eiersen"), None)
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

/// The project from a real session (2026-11-17): RM2027, not billable,
/// with the owner's five days 9-13 November already on it. Returns the
/// state, the company, the server an agent with `role` talks through,
/// and the model -- or None when there is no key or no database.
async fn hours_scenario(
    idp: &TestIdp,
    role: &str,
) -> Option<(
    regnmed_api::AppState,
    Uuid,
    regnmed_mcp::Server,
    regnmed_agent::llm::Llm,
)> {
    let Ok(key) = std::env::var("OPEN_ROUTER_KEY") else {
        eprintln!("hopper over: OPEN_ROUTER_KEY er ikke satt");
        return None;
    };
    let state = test_state(idp).await?;
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Timeprosjekt AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
        .await
        .unwrap();
    let client_id = format!("ekte-{role}-{}", Uuid::new_v4());
    let integration_id = regnmed_db::grant_integration(
        &state.pool,
        company,
        &client_id,
        "Agent Test",
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_state = state.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, regnmed_api::router(serve_state)).await;
    });
    let base = format!("http://{addr}");

    // The owner sets the scene through the same tools, as a person.
    let owner_tools = regnmed_mcp::Server::new(regnmed_mcp::client::Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&owner_sub, "Eier Eiersen"),
    ));
    let mut setup = vec![(
        "prosjekt_opprett",
        serde_json::json!({"kind": "prosjekt", "code": "RM2027", "name": "RegnMed 2027"}),
    )];
    for day in 9..=13 {
        setup.push((
            "timer_registrer",
            serde_json::json!({"dato": format!("2026-11-{day:02}"), "minutter": 480,
                               "prosjekt": "RM2027", "beskrivelse": "Utvikling",
                               "fakturerbar": false}),
        ));
    }
    for (name, args) in setup {
        let out = owner_tools
            .execute_tool(
                regnmed_mcp::tool::find(name).unwrap(),
                args.as_object().unwrap().clone(),
            )
            .await;
        assert!(!out.failed, "{name}: {}", out.text);
    }

    let server = regnmed_mcp::Server::new(regnmed_mcp::client::Client::new(
        &base,
        Some(company.to_string()),
        &idp.token(&client_id, ""),
    ));
    let model =
        regnmed_agent::llm::Llm::new("https://openrouter.ai/api/v1", &test_model(), &key).unwrap();
    Some((state, company, server, model))
}

/// Ground rule 5: a rate with no date given is not set from TODAY. In
/// the real session it was, and the five days already on the project
/// never got it. Asking is right -- and must be RECORDED as a question,
/// or the task closes with it; setting the rate from a date that covers
/// those days is also acceptable. Today is the failure.
#[tokio::test]
#[ignore = "calls a real model provider; run with --ignored"]
async fn a_real_model_does_not_date_a_rate_today_by_default() {
    let idp = TestIdp::new();
    let Some((state, company, server, model)) = hours_scenario(&idp, "bokforing").await else {
        return;
    };
    let outcome = regnmed_agent::run_loop(
        &model,
        &server,
        "bokforing",
        None,
        "La oss sette en timesats til 1500 kr på prosjekt RM2027.",
        &[],
    )
    .await
    .expect("sløyfen skal ikke feile mot en ekte leverandør");
    println!("status:  {}", outcome.status());
    println!("notat:   {}", outcome.note());

    let dated: Vec<chrono::NaiveDate> = sqlx::query_scalar(
        "select s.valid_from from prosjekt_sats s join dimension d on d.id = s.dimension_id
         where s.company_id = $1 and d.code = 'RM2027'",
    )
    .bind(company)
    .fetch_all(&state.pool)
    .await
    .unwrap();
    println!("satser fra: {dated:?}");
    let today = chrono::Local::now().date_naive();
    assert!(
        !dated.contains(&today),
        "satsen ble datert i dag uten at noen sa det: {}",
        outcome.note()
    );
    let covers = dated
        .iter()
        .any(|d| *d <= chrono::NaiveDate::from_ymd_opt(2026, 11, 9).unwrap());
    assert!(
        outcome.status() == "sporsmal" || covers,
        "verken spurt (status {}) eller satt fra en dato som dekker timene: {}",
        outcome.status(),
        outcome.note()
    );
}

/// The other rule: hours post nothing. Asked to log hours "so the
/// development costs are expensed", the agent logs them AND says that
/// logging them books no cost.
#[tokio::test]
#[ignore = "calls a real model provider; run with --ignored"]
async fn a_real_model_says_hours_are_not_bookkeeping() {
    let idp = TestIdp::new();
    let Some((state, company, server, model)) = hours_scenario(&idp, "ansatt").await else {
        return;
    };
    let outcome = regnmed_agent::run_loop(
        &model,
        &server,
        "ansatt",
        None,
        "Før 8 timer utvikling på prosjekt RM2027 mandag 16. november 2026, \
         så vi får utgiftsført utviklingskostnadene.",
        &[],
    )
    .await
    .expect("sløyfen skal ikke feile mot en ekte leverandør");
    println!("status:  {}", outcome.status());
    println!("notat:   {}", outcome.note());

    let logged: i64 = sqlx::query_scalar(
        "select count(*) from time_entry t join person p on p.id = t.person_id
         where t.company_id = $1 and t.dato = '2026-11-16' and t.minutter = 480
           and p.kind = 'integrasjon'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(logged, 1, "timene ble ikke ført: {}", outcome.note());
    // Said, not merely mentioned: «kostnaden er bokført» names the
    // ledger too, and is the very misunderstanding the rule corrects.
    let note = outcome.note().to_lowercase();
    let denies = ["ikke", "ingen", "ingenting"]
        .iter()
        .any(|w| note.contains(w));
    let ledger = ["bokfør", "hovedbok", "kostnad"]
        .iter()
        .any(|w| note.contains(w));
    assert!(
        denies && ledger,
        "agenten sa ikke at timer ikke er bokføring: {}",
        outcome.note()
    );
}
