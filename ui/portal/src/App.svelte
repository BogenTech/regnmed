<script>
  import { session, tokens, refresh, handleCallback } from "./lib/auth.svelte.js";
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
  import { lazy, preloadWhenIdle } from "./lib/lazy.svelte.js";

  // Alle seksjonene i menyen (lib/meny.js) er portert (#76 steg 2).
  // Brukere/Oppdrag/Periode bor nå som faner i Administrasjon; gamle
  // adresser rutes dit under (FLYTTET_TIL_ADMIN).
  // The overview is in the main bundle (it is where you land); the rest
  // are separate chunks loaded on first visit (lib/lazy.svelte.js).
  const SECTIONS = {
    oversikt: { component: Oversikt, preload() {} },
    faktura: lazy(() => import("./sections/faktura/Faktura.svelte")),
    produkter: lazy(() => import("./sections/produkter/Produkter.svelte")),
    prosjekter: lazy(() => import("./sections/prosjekter/Prosjekter.svelte")),
    timer: lazy(() => import("./sections/timer/Timer.svelte")),
    lonn: lazy(() => import("./sections/lonn/Lonn.svelte")),
    utlegg: lazy(() => import("./sections/utlegg/Utlegg.svelte")),
    anlegg: lazy(() => import("./sections/anlegg/Anlegg.svelte")),
    aksjonarer: lazy(() => import("./sections/aksjonaerer/Aksjonaerer.svelte")),
    reskontro: lazy(() => import("./sections/reskontro/Reskontro.svelte")),
    mva: lazy(() => import("./sections/mva/Mva.svelte")),
    rapporter: lazy(() => import("./sections/rapporter/Rapporter.svelte")),
    bank: lazy(() => import("./sections/bank/Bank.svelte")),
    bilag: lazy(() => import("./sections/bilag/Bilag.svelte")),
    hovedbok: lazy(() => import("./sections/hovedbok/Hovedbok.svelte")),
    kunder: lazy(() => import("./sections/kunder/Kunder.svelte")),
    leverandorer: lazy(() => import("./sections/leverandorer/Leverandorer.svelte")),
    admin: lazy(() => import("./sections/admin/Admin.svelte")),
  };
  const Byra = lazy(() => import("./sections/byra/Byra.svelte"));
  const Plattform = lazy(() => import("./components/Plattform.svelte"));

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
    // Ingenting ligger lagret: etter en omlasting avgjør refresh-cookien
    // om økten lever, så oppstarten veksler den én gang før
    // innloggingen i det hele tatt vises.
    session.authed = !!tokens() || (await refresh());
    boot.done = true;
  }
  start();

  // Offline-køen for kvitteringsfoto (#48): tømmes ved oppstart og når
  // nettet kommer tilbake.
  window.addEventListener("online", koSend);
  koSend();

  $effect(() => {
    if (me.loaded) preloadWhenIdle([...Object.values(SECTIONS), Byra, Plattform]);
  });

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

{#snippet laster(bit)}
  {#if bit.error}
    <!-- Typically a new release went out while the tab was open: the old
         chunks no longer exist on the server. -->
    <div class="alert alert-warning">
      <span>Kunne ikke laste siden. Last inn på nytt for å hente siste versjon.</span>
      <button class="btn btn-sm" onclick={() => location.reload()}>Last inn på nytt</button>
    </div>
  {:else}
    <div class="flex justify-center p-12">
      <span class="loading loading-spinner loading-md"></span>
    </div>
  {/if}
{/snippet}

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
      {@const Admin = SECTIONS.admin.component}
      {#if Admin}
        <Admin companyId={route.companyId} extra={FLYTTET_TIL_ADMIN[route.section]} />
      {:else}
        {@render laster(SECTIONS.admin)}
      {/if}
    {:else if SECTIONS[route.section]}
      {@const Section = SECTIONS[route.section].component}
      {#if Section}
        <Section companyId={route.companyId} extra={route.extra} query={route.query} />
      {:else}
        {@render laster(SECTIONS[route.section])}
      {/if}
    {:else}
      <!-- Ukjent seksjonsnavn i adressen: si fra, ikke vis en tom side. -->
      <div class="alert alert-warning">
        <span>Ukjent seksjon «{route.section}».</span>
      </div>
    {/if}
  </Shell>
{:else if route.view === "byra"}
  {#if Byra.component}
    <Byra.component firmId={route.firmId} />
  {:else}
    {@render laster(Byra)}
  {/if}
{:else if route.view === "plattform"}
  {#if Plattform.component}
    <Plattform.component />
  {:else}
    {@render laster(Plattform)}
  {/if}
{:else}
  <Companies />
{/if}
