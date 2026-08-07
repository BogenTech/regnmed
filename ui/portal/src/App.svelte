<script>
  import { session, tokens, handleCallback } from "./lib/auth.svelte.js";
  import { route } from "./lib/router.svelte.js";
  import { me, loadMe } from "./lib/me.svelte.js";
  import { FLYTTET_TIL_ADMIN } from "./lib/meny.js";
  import { toast } from "./lib/toast.svelte.js";
  import { setIkonstil } from "./lib/prefs.svelte.js";
  import { koSend } from "./lib/ko.js";
  import Login from "./components/Login.svelte";
  import Companies from "./components/Companies.svelte";
  import Shell from "./components/Shell.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Dialog from "./components/Dialog.svelte";
  import Oversikt from "./sections/oversikt/Oversikt.svelte";
  import Faktura from "./sections/faktura/Faktura.svelte";
  import Produkter from "./sections/produkter/Produkter.svelte";
  import Prosjekter from "./sections/prosjekter/Prosjekter.svelte";
  import Timer from "./sections/timer/Timer.svelte";
  import Lonn from "./sections/lonn/Lonn.svelte";
  import Utlegg from "./sections/utlegg/Utlegg.svelte";
  import Anlegg from "./sections/anlegg/Anlegg.svelte";
  import Aksjonaerer from "./sections/aksjonaerer/Aksjonaerer.svelte";
  import Reskontro from "./sections/reskontro/Reskontro.svelte";
  import Mva from "./sections/mva/Mva.svelte";
  import Rapporter from "./sections/rapporter/Rapporter.svelte";
  import Bank from "./sections/bank/Bank.svelte";
  import Bilag from "./sections/bilag/Bilag.svelte";
  import Hovedbok from "./sections/hovedbok/Hovedbok.svelte";
  import Kunder from "./sections/kunder/Kunder.svelte";
  import Leverandorer from "./sections/leverandorer/Leverandorer.svelte";
  import Admin from "./sections/admin/Admin.svelte";
  import Byra from "./sections/byra/Byra.svelte";
  import Plattform from "./components/Plattform.svelte";

  // Alle seksjonene i menyen (lib/meny.js) er portert (#76 steg 2).
  // Brukere/Oppdrag/Periode bor nå som faner i Administrasjon; gamle
  // adresser rutes dit under (FLYTTET_TIL_ADMIN).
  const SECTIONS = {
    oversikt: Oversikt,
    faktura: Faktura,
    produkter: Produkter,
    prosjekter: Prosjekter,
    timer: Timer,
    lonn: Lonn,
    utlegg: Utlegg,
    anlegg: Anlegg,
    aksjonarer: Aksjonaerer,
    reskontro: Reskontro,
    mva: Mva,
    rapporter: Rapporter,
    bank: Bank,
    bilag: Bilag,
    hovedbok: Hovedbok,
    kunder: Kunder,
    leverandorer: Leverandorer,
    admin: Admin,
  };

  let boot = $state({ done: false, error: null });

  async function start() {
    session.config = await (await fetch("/portal-config")).json();
    // Ikonstilen er en plattforminnstilling (låst globalt av
    // systemadmin) og følger med portal-config.
    if (session.config.ikonstil) setIkonstil(session.config.ikonstil);
    if (location.pathname === "/callback") {
      try {
        await handleCallback();
      } catch (error) {
        boot.error = error.message;
        boot.done = true;
        return;
      }
    }
    session.authed = !!tokens();
    boot.done = true;
  }
  start();

  // Offline-køen for kvitteringsfoto (#48): tømmes ved oppstart og når
  // nettet kommer tilbake.
  window.addEventListener("online", koSend);
  koSend();

  $effect(() => {
    if (boot.done && session.authed && !me.loaded) {
      loadMe().catch((error) => {
        if (error.message !== "ikke innlogget" && error.message !== "utløpt økt") {
          toast(error.message, false);
        }
      });
    }
  });
</script>

<Toasts />
<Dialog />

{#if !boot.done}
  <div class="min-h-screen flex items-center justify-center">
    <span class="loading loading-spinner loading-lg"></span>
  </div>
{:else if boot.error}
  <div class="p-8">
    <div class="alert alert-error"><span>{boot.error}</span></div>
    <a class="btn mt-4" href="/">Til forsiden</a>
  </div>
{:else if !session.authed}
  <Login />
{:else if !me.loaded}
  <div class="min-h-screen flex items-center justify-center">
    <span class="loading loading-spinner loading-lg"></span>
  </div>
{:else if route.view === "company"}
  <Shell companyId={route.companyId} section={route.section}>
    {#if FLYTTET_TIL_ADMIN[route.section]}
      <!-- Gamle bokmerker (#/c/{id}/brukere osv.) lander på riktig fane
           i Administrasjon-konsollen i stedet for en død adresse. -->
      <Admin companyId={route.companyId} extra={FLYTTET_TIL_ADMIN[route.section]} />
    {:else if SECTIONS[route.section]}
      {@const Section = SECTIONS[route.section]}
      <Section companyId={route.companyId} extra={route.extra} query={route.query} />
    {:else}
      <!-- Ukjent seksjonsnavn i adressen: si fra, ikke vis en tom side. -->
      <div class="alert alert-warning">
        <span>Ukjent seksjon «{route.section}».</span>
      </div>
    {/if}
  </Shell>
{:else if route.view === "byra"}
  <Byra firmId={route.firmId} />
{:else if route.view === "plattform"}
  <Plattform />
{:else}
  <Companies />
{/if}
