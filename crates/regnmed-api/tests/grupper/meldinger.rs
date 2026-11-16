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
        Some(json!({"text": "Kan du bokføre denne for meg?", "documents": [document]})),
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
    assert!(order.contains(document.as_str().unwrap()), "{order}");

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
    assert_eq!(thread["attach_to"], json!([company]));
    let (_, unread) = call(&state, "GET", "/chat/unread", &eva.token, None).await;
    assert_eq!(unread["unread"], 1, "{unread}");

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
