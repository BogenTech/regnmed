//! Meldinger (docs/meldinger.md): who reaches whom across a company and
//! its byrå, the byrå's chosen contact, a receipt sent to the company's
//! agent becoming its order and its question coming back, and reach
//! ending with the engagement. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(
            builder
                .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

struct Person {
    id: Uuid,
    token: String,
}

async fn person(state: &AppState, idp: &TestIdp, name: &str) -> Person {
    let sub = format!("test|{}", Uuid::new_v4());
    let id = regnmed_db::ensure_person(&state.pool, &sub, Some(name), None)
        .await
        .unwrap();
    Person {
        id,
        token: idp.token(&sub, name),
    }
}

/// Names in my contact list, with how each is reached.
async fn contacts(state: &AppState, who: &Person) -> Vec<(String, String)> {
    let (status, body) = call(state, "GET", "/chat/contacts", &who.token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut out: Vec<(String, String)> = body["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_string(),
                c["links"][0]["via"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn people_reach_their_colleagues_their_byra_and_their_own_agents() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let tag = Uuid::new_v4().simple().to_string()[..6].to_string();
    let n = |s: &str| format!("{s} {tag}");

    // ---- A company, its byrå, and a stranger company with its own agent ----
    let company = regnmed_db::create_company(pool, &unique_orgnumber(), "Praten AS")
        .await
        .unwrap();
    let (anne, eva) = (
        person(&state, &idp, &n("Anne")).await,
        person(&state, &idp, &n("Eva")).await,
    );
    regnmed_db::ensure_company_member(pool, company, anne.id, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(pool, company, eva.id, "ansatt")
        .await
        .unwrap();
    let firm_orgnr = unique_orgnumber();
    let firm = regnmed_db::ensure_firm(pool, &firm_orgnr, "Byrået AS", "regnskap")
        .await
        .unwrap();
    let (bjorn, kari) = (
        person(&state, &idp, &n("Bjørn")).await,
        person(&state, &idp, &n("Kari")).await,
    );
    regnmed_db::ensure_firm_member(pool, firm, bjorn.id, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(pool, firm, kari.id, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(pool, firm, company, "regnskap")
        .await
        .unwrap();
    let engagement: Uuid =
        sqlx::query_scalar("select id from engagement where firm_id = $1 and company_id = $2")
            .bind(firm)
            .bind(company)
            .fetch_one(pool)
            .await
            .unwrap();

    let agent_client = format!("bokforer-{}", Uuid::new_v4());
    let agent_token = idp.token(&agent_client, "");
    let (status, granted) = call(
        &state,
        "POST",
        &format!("/companies/{company}/integrations"),
        &anne.token,
        Some(json!({"client_id": agent_client, "navn": n("Bokføreren"), "access": "bokforing"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{granted}");

    let other = regnmed_db::create_company(pool, &unique_orgnumber(), "Fremmed AS")
        .await
        .unwrap();
    let ola = person(&state, &idp, &n("Ola")).await;
    regnmed_db::ensure_company_member(pool, other, ola.id, "admin")
        .await
        .unwrap();
    let (status, _) = call(
        &state,
        "POST",
        &format!("/companies/{other}/integrations"),
        &ola.token,
        Some(json!({"client_id": format!("annen-{}", Uuid::new_v4()), "navn": n("Fremmedagent"), "access": "les"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ---- Who reaches whom ----
    // The client: colleagues, its OWN agent, and the byrå's admin until
    // the byrå picks a contact. Not the byrå's other people, not the
    // stranger's agent.
    assert_eq!(
        contacts(&state, &eva).await,
        vec![
            (n("Anne"), "colleague".into()),
            (n("Bjørn"), "accountant".into()),
            (n("Bokføreren"), "agent".into()),
        ]
    );
    // The byrå: its colleagues, and everyone in the client, agent included.
    assert_eq!(
        contacts(&state, &kari).await,
        vec![
            (n("Anne"), "client".into()),
            (n("Bjørn"), "firm_colleague".into()),
            (n("Bokføreren"), "agent".into()),
            (n("Eva"), "client".into()),
        ]
    );

    // ---- The byrå picks Kari as the client's contact ----
    let (status, refused) = call(
        &state,
        "PUT",
        &format!("/firms/{firm}/engagements/{engagement}/contacts"),
        &kari.token,
        Some(json!({"person_ids": [kari.id]})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "only the byrå's admin chooses: {refused}"
    );
    let (status, _) = call(
        &state,
        "PUT",
        &format!("/firms/{firm}/engagements/{engagement}/contacts"),
        &bjorn.token,
        Some(json!({"person_ids": [ola.id]})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a contact must be in the byrå"
    );
    let (status, _) = call(
        &state,
        "PUT",
        &format!("/firms/{firm}/engagements/{engagement}/contacts"),
        &bjorn.token,
        Some(json!({"person_ids": [kari.id]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let reached = contacts(&state, &eva).await;
    assert!(
        reached.contains(&(n("Kari"), "accountant".into())),
        "{reached:?}"
    );
    assert!(
        !reached.iter().any(|(name, _)| *name == n("Bjørn")),
        "{reached:?}"
    );
    // A chosen contact who leaves the byrå counts as none chosen: the
    // admins step in rather than leaving the client with nobody.
    let set_active = |active: bool| {
        sqlx::query("update firm_member set active = $3 where firm_id = $1 and person_id = $2")
            .bind(firm)
            .bind(kari.id)
            .bind(active)
            .execute(pool)
    };
    set_active(false).await.unwrap();
    let reached = contacts(&state, &eva).await;
    assert!(
        reached.contains(&(n("Bjørn"), "accountant".into())),
        "{reached:?}"
    );
    set_active(true).await.unwrap();

    // ---- The stranger cannot open a conversation with the client ----
    let (status, _) = call(
        &state,
        "POST",
        "/chat/conversations",
        &ola.token,
        Some(json!({"person_id": eva.id})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ---- The client sends a receipt to her company's agent ----
    let (_, eva_contacts) = call(&state, "GET", "/chat/contacts", &eva.token, None).await;
    let agent = eva_contacts["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["agent"] == true)
        .unwrap()
        .clone();
    let (status, opened) = call(
        &state,
        "POST",
        "/chat/conversations",
        &eva.token,
        Some(json!({"person_id": agent["person_id"], "company_id": company})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    let with_agent = opened["conversation_id"].as_str().unwrap().to_string();
    // The task card's «Skriv til …» opens this same conversation, from the
    // agent's person id on its grant.
    let (_, grants) = call(
        &state,
        "GET",
        &format!("/companies/{company}/integrations"),
        &anne.token,
        None,
    )
    .await;
    assert_eq!(
        grants["integrasjoner"][0]["person_id"], agent["person_id"],
        "{grants}"
    );

    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/companies/{company}/inbox?filename=kvittering.jpg"
                ))
                .header("authorization", format!("Bearer {}", eva.token))
                .header("content-type", "image/jpeg")
                .body(Body::from(format!("jpeg {}", Uuid::new_v4())))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let document = serde_json::from_slice::<Value>(&bytes).unwrap()["document_id"].clone();

    let (status, posted) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_agent}/messages"),
        &eva.token,
        // The same file twice (the inbox links a copy already there) is
        // one attachment and one mention in the order.
        Some(json!({"text": "Kan du bokføre denne for meg?", "documents": [document, document]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let task_id = posted["task_id"]
        .as_str()
        .expect("a message to an agent is an order")
        .to_string();
    let (_, task) = call(
        &state,
        "GET",
        &format!("/companies/{company}/agent/oppgaver/{task_id}"),
        &anne.token,
        None,
    )
    .await;
    assert_eq!(task["oppgave"]["opprettet_av"], n("Eva"), "{task}");
    let order = task["oppgave"]["melding"].as_str().unwrap();
    assert!(order.contains("Kan du bokføre denne for meg?"), "{order}");
    assert_eq!(
        order.matches(document.as_str().unwrap()).count(),
        1,
        "{order}"
    );

    // The agent's question comes back into the same conversation.
    let (status, answered) = call(
        &state,
        "POST",
        &format!("/companies/{company}/agent/oppgaver/{task_id}/svar"),
        &agent_token,
        Some(json!({"status": "sporsmal", "notat": "Bevertning eller kontorrekvisita?"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answered}");
    let (_, thread) = call(
        &state,
        "GET",
        &format!("/chat/conversations/{with_agent}"),
        &eva.token,
        None,
    )
    .await;
    let last = thread["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["author"], n("Bokføreren"), "{thread}");
    assert_eq!(
        thread["messages"][0]["documents"][0]["filename"],
        "kvittering.jpg"
    );
    assert_eq!(
        thread["messages"][0]["documents"].as_array().unwrap().len(),
        1,
        "{thread}"
    );
    assert_eq!(thread["attach_to"], json!([company]));
    let (_, unread) = call(&state, "GET", "/chat/unread", &eva.token, None).await;
    assert_eq!(unread["unread"], 1, "{unread}");
    // The bell: the agent's question is a message from the agent, pointing
    // at the conversation -- for Eva, and for nobody else.
    let (_, bell) = call(&state, "GET", "/notifications", &eva.token, None).await;
    assert_eq!(bell["varsler"].as_array().unwrap().len(), 1, "{bell}");
    assert_eq!(bell["varsler"][0]["type"], "melding");
    assert_eq!(bell["varsler"][0]["conversation_id"], with_agent.as_str());
    assert_eq!(bell["varsler"][0]["fra"], n("Bokføreren"));
    assert_eq!(
        bell["varsler"][0]["tekst"],
        "Bevertning eller kontorrekvisita?"
    );
    let (_, bell) = call(&state, "GET", "/notifications", &anne.token, None).await;
    assert_eq!(bell["varsler"], json!([]), "someone else's conversation");
    // The task list shows what Eva wrote, not the wrapped order.
    let (_, tasks) = call(
        &state,
        "GET",
        &format!("/companies/{company}/agent/oppgaver"),
        &anne.token,
        None,
    )
    .await;
    assert_eq!(
        tasks["oppgaver"][0]["fra_melding"], "Kan du bokføre denne for meg?",
        "{tasks}"
    );

    // Her reply continues the SAME task.
    let (_, posted) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_agent}/messages"),
        &eva.token,
        Some(json!({"text": "Kontorrekvisita"})),
    )
    .await;
    assert_eq!(posted["task_id"], task_id.as_str(), "{posted}");
    let (_, unread) = call(&state, "GET", "/chat/unread", &eva.token, None).await;
    assert_eq!(unread["unread"], 0, "writing marks the conversation read");
    let (_, bell) = call(&state, "GET", "/notifications", &eva.token, None).await;
    assert_eq!(
        bell["varsler"],
        json!([]),
        "a read conversation rings no bell"
    );

    // ---- «Ny samtale»: the agent starts fresh ----
    // The agent asks again, and Eva starts over instead of answering. Every
    // order carries the recent conversation, and an agent trusts its own
    // earlier answers -- so a wrong one kept coming back (prod, 2026-11-19).
    let (status, _) = call(
        &state,
        "POST",
        &format!("/companies/{company}/agent/oppgaver/{task_id}/svar"),
        &agent_token,
        Some(json!({"status": "sporsmal", "notat": "Hvilken konto?"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, restarted) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_agent}/restart"),
        &eva.token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restarted}");
    let (_, thread) = call(
        &state,
        "GET",
        &format!("/chat/conversations/{with_agent}"),
        &eva.token,
        None,
    )
    .await;
    assert_eq!(thread["ny_start"].as_array().unwrap().len(), 1, "{thread}");
    assert!(
        thread["messages"].as_array().unwrap().len() >= 4,
        "nothing is deleted: {thread}"
    );
    let order_text = |task: &str| {
        let state = state.clone();
        let token = anne.token.clone();
        let url = format!("/companies/{company}/agent/oppgaver/{task}");
        async move {
            let (_, task) = call(&state, "GET", &url, &token, None).await;
            task["oppgave"]["melding"].as_str().unwrap().to_string()
        }
    };
    let (_, posted) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_agent}/messages"),
        &eva.token,
        Some(json!({"text": "Hva er saldoen på 1920?"})),
    )
    .await;
    let fresh = posted["task_id"].as_str().unwrap().to_string();
    assert_ne!(fresh, task_id, "the open question is not continued");
    let order = order_text(&fresh).await;
    assert!(!order.contains("Kontorrekvisita"), "{order}");
    assert!(!order.contains("Tidligere i samtalen"), "{order}");
    // What is said after the restart is the new conversation.
    call(
        &state,
        "POST",
        &format!("/companies/{company}/agent/oppgaver/{fresh}/svar"),
        &agent_token,
        Some(json!({"status": "utfort", "notat": "Saldoen er 1 117,75 kr."})),
    )
    .await;
    let (_, posted) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_agent}/messages"),
        &eva.token,
        Some(json!({"text": "Og 1950?"})),
    )
    .await;
    let order = order_text(posted["task_id"].as_str().unwrap()).await;
    assert!(order.contains("Saldoen er 1 117,75 kr."), "{order}");
    assert!(!order.contains("Kontorrekvisita"), "{order}");

    // ---- Client and byrå contact talk; the newest conversation is on top ----
    let (_, opened) = call(
        &state,
        "POST",
        "/chat/conversations",
        &eva.token,
        Some(json!({"person_id": kari.id})),
    )
    .await;
    let with_kari = opened["conversation_id"].as_str().unwrap().to_string();
    // Opening again is the same conversation, from either side.
    let (_, again) = call(
        &state,
        "POST",
        "/chat/conversations",
        &kari.token,
        Some(json!({"person_id": eva.id})),
    )
    .await;
    assert_eq!(again["conversation_id"], with_kari.as_str());
    // Starting over only means something to an agent.
    let (status, _) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_kari}/restart"),
        &eva.token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &state,
        "POST",
        &format!("/chat/conversations/{with_kari}/messages"),
        &eva.token,
        Some(json!({"text": "Hei Kari, når er mva-fristen?"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(&state, "GET", "/chat/conversations", &eva.token, None).await;
    assert_eq!(
        list["conversations"][0]["conversation_id"],
        with_kari.as_str(),
        "{list}"
    );
    assert_eq!(
        list["conversations"][1]["conversation_id"],
        with_agent.as_str(),
        "{list}"
    );
    let (_, list) = call(&state, "GET", "/chat/conversations", &kari.token, None).await;
    assert_eq!(list["conversations"][0]["unread"], 1, "{list}");
    // «Marker alle som lest» empties the bell without opening anything.
    let (_, bell) = call(&state, "GET", "/notifications", &kari.token, None).await;
    assert_eq!(bell["varsler"][0]["uleste"], 1, "{bell}");
    let (status, _) = call(&state, "POST", "/chat/read-all", &kari.token, None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, bell) = call(&state, "GET", "/notifications", &kari.token, None).await;
    assert_eq!(bell["varsler"], json!([]), "{bell}");

    // An outsider cannot read it, even knowing the id.
    let (status, _) = call(
        &state,
        "GET",
        &format!("/chat/conversations/{with_kari}"),
        &ola.token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ---- The engagement ends: so does the conversation, both ways ----
    sqlx::query("update engagement set valid_to = current_date where id = $1")
        .bind(engagement)
        .execute(pool)
        .await
        .unwrap();
    let (status, _) = call(
        &state,
        "GET",
        &format!("/chat/conversations/{with_kari}"),
        &kari.token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = call(&state, "GET", "/chat/conversations", &eva.token, None).await;
    assert_eq!(list["conversations"].as_array().unwrap().len(), 1, "{list}");
}

/// A message to an agent that cannot run is ANSWERED, not left waiting:
/// an agent regnmed hosts but cannot start writes why into its task, and
/// that note reaches the conversation. One that regnmed does not host at
/// all (no model, no secret -- the customer's own MCP agent looks the
/// same) is never told it is broken; the thread says up front that it
/// answers only where it runs.
#[tokio::test]
async fn an_agent_that_cannot_run_says_so_in_the_conversation() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let key = state.sealing_key.clone().expect("the test state seals");
    let owner = person(&state, &idp, "Ola Eier").await;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Stille AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner.id, "admin")
        .await
        .unwrap();
    let integration = regnmed_db::grant_integration(
        &state.pool,
        company,
        &format!("agent-{}", Uuid::new_v4()),
        "Agent Test",
        None,
        "bokforing",
        "Ola Eier",
    )
    .await
    .unwrap();

    let (_, found) = call(&state, "GET", "/chat/contacts", &owner.token, None).await;
    let agent = found["contacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["agent"] == true)
        .unwrap()
        .clone();
    let (_, opened) = call(
        &state,
        "POST",
        "/chat/conversations",
        &owner.token,
        Some(json!({"person_id": agent["person_id"], "company_id": company})),
    )
    .await;
    let conversation = opened["conversation_id"].as_str().unwrap().to_string();
    let thread_uri = format!("/chat/conversations/{conversation}");

    // Not hosted here: the thread says so, and the worker stays out.
    let (_, thread) = call(&state, "GET", &thread_uri, &owner.token, None).await;
    assert_eq!(thread["runs_here"], false, "{thread}");
    let (_, posted) = call(
        &state,
        "POST",
        &format!("{thread_uri}/messages"),
        &owner.token,
        Some(json!({"text": "Kan du lage et utlegg?"})),
    )
    .await;
    let task: Uuid = posted["task_id"].as_str().unwrap().parse().unwrap();
    let run = |task| {
        let state = state.clone();
        let key = key.clone();
        async move {
            regnmed_agent::run_task(&state.pool, &key, company, task, "unused", "unused", None)
                .await
        }
    };
    assert!(
        run(task)
            .await
            .unwrap_err()
            .is::<regnmed_agent::NotRunHere>(),
        "not ours to answer"
    );
    assert!(
        regnmed_db::agent_task::history(&state.pool, company, task)
            .await
            .unwrap()
            .is_empty(),
        "an agent run elsewhere must not be told it is broken"
    );

    // Hosted (its secret is stored) but no model: it answers in the task,
    // and the answer is a message in the same conversation.
    regnmed_db::agentllm::sett_klienthemmelighet(&state.pool, &key, company, integration, "s3cret")
        .await
        .unwrap();
    let (_, thread) = call(&state, "GET", &thread_uri, &owner.token, None).await;
    assert_eq!(thread["runs_here"], true, "{thread}");
    let outcome = run(task).await.unwrap();
    assert_eq!(outcome.status(), "sporsmal", "a question: a reply retries");
    assert!(outcome.note().contains("språkmodell"), "{}", outcome.note());

    let (_, thread) = call(&state, "GET", &thread_uri, &owner.token, None).await;
    let last = thread["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["author"], "Agent Test", "{thread}");
    assert!(
        last["text"].as_str().unwrap().contains("Brukere og roller")
            && last["text"].as_str().unwrap().contains("Gi modell"),
        "{thread}"
    );
}

/// The conversation with an agent is the whole history with it: a task
/// given from the Oppdrag card shows up in Meldinger with its question and
/// answer, without anything being copied (docs/meldinger.md). Found in
/// prod on 2026-11-18, where three tasks with RegnmedAgent left Meldinger
/// saying «Ingen samtaler ennå». And no thread is sent whole: it pages.
#[tokio::test]
async fn an_agents_task_history_is_its_conversation_and_long_threads_page() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let owner = person(&state, &idp, "Kari Eier").await;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Historikk AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner.id, "admin")
        .await
        .unwrap();
    let agent = regnmed_db::grant_integration(
        &state.pool,
        company,
        &format!("agent-{}", Uuid::new_v4()),
        "Historieagent",
        None,
        "bokforing",
        "Kari Eier",
    )
    .await
    .unwrap();
    // Given from the card: no chat message, only the task trail.
    let task = regnmed_db::agent_task::create_task(
        &state.pool,
        company,
        Some(agent),
        "Sett timesats 1700 kr på prosjekt 10001.",
        None,
        "Kari Eier",
        Some(owner.id),
    )
    .await
    .unwrap();
    for (status, note, by) in [
        ("pagar", None, "Historieagent"),
        ("sporsmal", Some("Fra hvilken dato?"), "Historieagent"),
        ("pagar", Some("2027-01-01"), "Kari Eier"),
        ("utfort", Some("Satt fra 2027-01-01."), "Historieagent"),
    ] {
        regnmed_db::agent_task::answer(&state.pool, company, task, status, note, by, None)
            .await
            .unwrap();
    }

    // The conversation exists where the person looks for it.
    let (status, list) = call(&state, "GET", "/chat/conversations", &owner.token, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let row = list["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["with"]["name"] == "Historieagent")
        .unwrap_or_else(|| panic!("no conversation with the agent: {list}"))
        .clone();
    assert_eq!(row["last_text"], "Satt fra 2027-01-01.");
    assert_eq!(row["last_from_me"], false);
    let conv = row["conversation_id"].as_str().unwrap().to_string();

    let (status, thread) = call(
        &state,
        "GET",
        &format!("/chat/conversations/{conv}"),
        &owner.token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let me = thread["me"].as_str().unwrap().to_string();
    let lines: Vec<(String, bool)> = thread["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["text"].as_str().unwrap().to_string(),
                m["author_id"] == me.as_str(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        vec![
            ("Sett timesats 1700 kr på prosjekt 10001.".into(), true),
            ("Fra hvilken dato?".into(), false),
            ("2027-01-01".into(), true),
            ("Satt fra 2027-01-01.".into(), false),
        ],
        "the order, the question, the answer and the result, in order"
    );
    assert_eq!(thread["messages"][0]["task_status"], "utfort");
    assert_eq!(thread["older"], false);

    // Paging: the newest page, then back from its first message.
    let (_, page) = call(
        &state,
        "GET",
        &format!("/chat/conversations/{conv}?limit=3"),
        &owner.token,
        None,
    )
    .await;
    assert_eq!(page["messages"].as_array().unwrap().len(), 3);
    assert_eq!(page["older"], true, "{page}");
    assert_eq!(page["messages"][0]["text"], "Fra hvilken dato?");
    let first = &page["messages"][0];
    let (_, back) = call(
        &state,
        "GET",
        &format!(
            "/chat/conversations/{conv}?limit=3&before={}&before_id={}",
            urlencoding(first["created_at"].as_str().unwrap()),
            first["message_id"].as_str().unwrap()
        ),
        &owner.token,
        None,
    )
    .await;
    let back_texts: Vec<&str> = back["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(back_texts, vec!["Sett timesats 1700 kr på prosjekt 10001."]);
    assert_eq!(back["older"], false);
}

fn urlencoding(s: &str) -> String {
    s.replace('+', "%2B").replace(':', "%3A")
}
