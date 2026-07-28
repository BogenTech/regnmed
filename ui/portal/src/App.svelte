<script>
  import { session, tokens, handleCallback } from "./lib/auth.svelte.js";
  import { route } from "./lib/router.svelte.js";
  import { me, loadMe } from "./lib/me.svelte.js";
  import { seksjonNavn } from "./lib/meny.js";
  import { toast } from "./lib/toast.svelte.js";
  import Login from "./components/Login.svelte";
  import Companies from "./components/Companies.svelte";
  import Shell from "./components/Shell.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Placeholder from "./sections/Placeholder.svelte";
  import Oversikt from "./sections/oversikt/Oversikt.svelte";

  // Porterte seksjoner (#76 steg 2) — resten er plassholdere til
  // dagens portal inntil de flyttes hit.
  const SECTIONS = { oversikt: Oversikt };

  let boot = $state({ done: false, error: null });

  async function start() {
    session.config = await (await fetch("/portal-config")).json();
    if (location.pathname === "/ny/callback") {
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

{#if !boot.done}
  <div class="min-h-screen flex items-center justify-center">
    <span class="loading loading-spinner loading-lg"></span>
  </div>
{:else if boot.error}
  <div class="p-8">
    <div class="alert alert-error"><span>{boot.error}</span></div>
    <a class="btn mt-4" href="/ny">Til forsiden</a>
  </div>
{:else if !session.authed}
  <Login />
{:else if !me.loaded}
  <div class="min-h-screen flex items-center justify-center">
    <span class="loading loading-spinner loading-lg"></span>
  </div>
{:else if route.view === "company"}
  <Shell companyId={route.companyId} section={route.section}>
    {#if SECTIONS[route.section]}
      {@const Section = SECTIONS[route.section]}
      <Section companyId={route.companyId} extra={route.extra} />
    {:else}
      <Placeholder
        companyId={route.companyId}
        section={route.section}
        label={seksjonNavn(route.section)}
      />
    {/if}
  </Shell>
{:else if route.view === "byra"}
  <div class="p-8">
    <div class="card bg-base-100 shadow-sm max-w-md">
      <div class="card-body">
        <h2 class="card-title">Byrå</h2>
        <p class="opacity-70 text-sm">Byråvisningen er ikke flyttet til den nye portalen ennå.</p>
        <a class="btn btn-primary btn-sm w-fit" href={"/#/byra/" + route.firmId}>
          Åpne i dagens portal
        </a>
      </div>
    </div>
  </div>
{:else}
  <Companies />
{/if}
