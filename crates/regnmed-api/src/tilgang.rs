//! Én tilgangsvakt for alle selskapsendepunkter (#56, docs/auth.md).
//!
//! Før dette hadde hver modul sin egen `require_access` — 22 kopier i
//! tre ulike former (`write: bool`, `admin: bool`, et nivå som streng,
//! og noen uten parameter i det hele tatt). Så lenge regelen var «les
//! eller skriv» gikk det bra. Det slutter å gå bra i det øyeblikket
//! rollene blir flere enn en rangstige, og hver kopi er et sted å ta
//! feil.
//!
//! Modellen her snur spørsmålet: et endepunkt sier hva handlingen
//! **krever**, ikke hvem som får gjøre den. Rollen avgjør om kravet er
//! oppfylt, og den avgjørelsen finnes ett sted. Samme begrunnelse som
//! ratebegrensningen i #45: én søm ingen kan glemme.
//!
//! Autorisasjonen selv ligger fortsatt i regnmed-db (`company_access`)
//! — tokenet beviser identitet, databasen avgjør tilgang.

use uuid::Uuid;

use crate::AppState;
use crate::auth::ApiError;

/// Hva en handling krever av den som utfører den.
///
/// Kravet hører til **handlingen**, ikke til personen: «å bokføre
/// krever bokføringstilgang» er en egenskap ved det å bokføre, og den
/// endrer seg ikke når vi legger til en rolle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Krav {
    /// Lese selskapets bøker og rapporter.
    Les,
    /// Endre hovedboken, eller noe som ender i den.
    Bokfor,
    /// Selskapsadministrasjon: innstillinger, låser, integrasjoner.
    Admin,
}

impl Krav {
    fn beskrivelse(self) -> &'static str {
        match self {
            Self::Les => "krever tilgang til selskapet",
            Self::Bokfor => "krever bokføringstilgang",
            Self::Admin => "krever admin-tilgang",
        }
    }
}

/// En persons rolle i ett selskap, slik `company_access` løser den.
///
/// Rekkefølgen i enumet er rangstigen, og den er bevisst uttrykt som en
/// type framfor en streng: et endepunkt kan ikke sammenligne seg fram
/// til feil svar med en skrivefeil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rolle {
    Les,
    Bokforing,
    Admin,
}

impl Rolle {
    /// Ukjente verdier blir den svakeste rollen, ikke en feil.
    ///
    /// Databasen har en check-constraint på lovlige roller, så dette
    /// skal ikke kunne skje — men skulle det skje, er «minst tilgang»
    /// det trygge svaret. Å feile åpent her ville gjort en
    /// datafeil til en tilgangseskalering.
    pub fn fra_db(s: &str) -> Self {
        match s {
            "admin" => Self::Admin,
            "bokforing" => Self::Bokforing,
            _ => Self::Les,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Bokforing => "bokforing",
            Self::Les => "les",
        }
    }

    pub fn er_admin(self) -> bool {
        self == Self::Admin
    }

    /// Om rollen oppfyller kravet.
    pub fn oppfyller(self, krav: Krav) -> bool {
        match krav {
            Krav::Les => true,
            Krav::Bokfor => self >= Self::Bokforing,
            Krav::Admin => self == Self::Admin,
        }
    }
}

/// Slår opp personens rolle i selskapet og krever at den oppfyller
/// `krav`. Returnerer rollen, så en handler som trenger å vite mer
/// (typisk: «er dette en admin?») slipper et nytt oppslag.
///
/// **Ukjent selskap og manglende tilgang gir begge 404.** En som ikke
/// har tilgang skal ikke få vite om selskapet finnes; det er samme
/// regel som ellers i API-et (docs/auth.md).
pub async fn krev(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    krav: Krav,
) -> Result<Rolle, ApiError> {
    let rolle = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .map(|s| Rolle::fra_db(&s))
        .ok_or(ApiError::NotFound)?;
    if !rolle.oppfyller(krav) {
        return Err(ApiError::Forbidden(krav.beskrivelse()));
    }
    Ok(rolle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rangstigen_er_den_samme_som_for() {
        // Les kommer alle til; det er selve medlemskapet som teller.
        for r in [Rolle::Les, Rolle::Bokforing, Rolle::Admin] {
            assert!(r.oppfyller(Krav::Les), "{r:?}");
        }
        // Bokføring: alt unntatt les.
        assert!(!Rolle::Les.oppfyller(Krav::Bokfor));
        assert!(Rolle::Bokforing.oppfyller(Krav::Bokfor));
        assert!(Rolle::Admin.oppfyller(Krav::Bokfor));
        // Admin: bare admin.
        assert!(!Rolle::Les.oppfyller(Krav::Admin));
        assert!(!Rolle::Bokforing.oppfyller(Krav::Admin));
        assert!(Rolle::Admin.oppfyller(Krav::Admin));
    }

    #[test]
    fn rollen_er_rundtur_mot_databasens_verdier() {
        for slug in ["admin", "bokforing", "les"] {
            assert_eq!(Rolle::fra_db(slug).slug(), slug);
        }
    }

    /// En rolle vi ikke kjenner igjen skal gi MINST tilgang. Feiler
    /// dette, blir en datafeil til en tilgangseskalering.
    #[test]
    fn ukjent_rolle_faller_til_svakeste() {
        assert_eq!(Rolle::fra_db("superbruker"), Rolle::Les);
        assert_eq!(Rolle::fra_db(""), Rolle::Les);
        assert!(!Rolle::fra_db("superbruker").oppfyller(Krav::Bokfor));
    }
}
