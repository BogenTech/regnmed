//! Portalen (Svelte 5 + Vite, #76) serveres fra binæren på roten — den
//! innsjekkede ui/portal/dist, samme binær, samme origin. Requires
//! DATABASE_URL (skips politely otherwise, like the other integration
//! tests).

use crate::common::{TestIdp, test_state};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use regnmed_api::{AppState, router};

struct Svar {
    status: StatusCode,
    content_type: String,
    cache_control: String,
    location: String,
    body: String,
}

async fn get(state: &AppState, uri: &str) -> Svar {
    let response = router(state.clone())
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let header = |navn: &str| {
        response
            .headers()
            .get(navn)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let content_type = header("content-type");
    let cache_control = header("cache-control");
    let location = header("location");
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    Svar {
        status,
        content_type,
        cache_control,
        location,
        body: String::from_utf8_lossy(&bytes).to_string(),
    }
}

#[tokio::test]
async fn portalen_serveres_fra_binaren() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // / og /callback lander begge i appen: rutingen skjer i hashen, og
    // callback-adressen må fullføre PKCE-flyten i appen.
    let mut index = String::new();
    for uri in ["/", "/callback"] {
        let svar = get(&state, uri).await;
        assert_eq!(svar.status, StatusCode::OK, "{uri}");
        assert!(svar.content_type.starts_with("text/html"), "{uri}");
        assert!(svar.body.contains("<div id=\"app\">"), "{uri}");
        index = svar.body;
    }

    // Vite skriver innholdshashede filnavn — finn dem i index.html og
    // sjekk at de faktisk serveres, med lang cache (innholdet bak en
    // hashet adresse endrer seg aldri).
    let mut assets = 0;
    for del in index.split('"') {
        if !del.starts_with("/assets/") {
            continue;
        }
        let svar = get(&state, del).await;
        assert_eq!(svar.status, StatusCode::OK, "{del}");
        assert!(svar.cache_control.contains("immutable"), "{del}");
        if del.ends_with(".js") {
            assert!(svar.content_type.starts_with("text/javascript"), "{del}");
        }
        if del.ends_with(".css") {
            assert!(svar.content_type.starts_with("text/css"), "{del}");
        }
        assets += 1;
    }
    assert!(assets >= 2, "index.html må peke på minst JS + CSS: {index}");

    // En ukjent asset er en FEIL, ikke appen på nytt: en 200 med HTML
    // der JS-en skulle vært, feiler stille i nettleseren.
    let svar = get(&state, "/assets/finnes-ikke.js").await;
    assert_eq!(svar.status, StatusCode::NOT_FOUND);

    // /ny var portalens adresse under migreringen — gamle bokmerker og
    // åpne faner skal lande på portalen, ikke i en 404.
    let svar = get(&state, "/ny").await;
    assert_eq!(svar.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(svar.location, "/");

    // Den rammeverksfrie portalen er borte for godt.
    for gammel in ["/app.js", "/app.css", "/theme.js"] {
        assert_eq!(
            get(&state, gammel).await.status,
            StatusCode::NOT_FOUND,
            "{gammel}"
        );
    }
}
