<script>
  // The hub after login: pick a company or byrå — or, for a brand-new
  // user, the guided registration flow (#77) front and center. Existing
  // users reach the same flow through a collapsed card at the bottom.
  import { api } from "../lib/api.js";
  import { me } from "../lib/me.svelte.js";
  import UserMenu from "./UserMenu.svelte";
  import Registrering from "./Registrering.svelte";

  let firms = $state([]);
  let firmsLastet = $state(false);

  async function lastByraer() {
    try {
      const mine = await api("/firms/mine");
      firms = mine.firms || [];
    } catch (error) {
      /* seksjonen er valgfri */
    }
    firmsLastet = true;
  }

  $effect(() => {
    lastByraer();
  });

  // Only call the user "new" once both sources have answered — a
  // byrå-only user must not see the welcome flow flash by.
  // En ren plattformbruker er ikke «ny» — den skal til plattformkortet,
  // ikke inn i registreringsflyten.
  const nyBruker = $derived(
    firmsLastet && me.companies.length === 0 && firms.length === 0 && !me.plattform,
  );
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1"><span class="btn btn-ghost text-xl">regnmed</span></div>
  <div class="flex-none gap-2">
    <UserMenu />
  </div>
</div>
<main class="p-6 max-w-3xl mx-auto">
  {#if me.nyeTilganger > 0}
    <div class="alert alert-success mb-4 text-sm">
      Invitasjonen din er innløst — du har fått {me.nyeTilganger} ny(e) tilgang(er).
    </div>
  {/if}

  {#if me.plattform}
    <a href="#/plattform" class="card card-border bg-base-100 hover:border-primary mb-4 block">
      <div class="card-body">
        <h2 class="card-title">
          Plattform <span class="badge badge-warning">{me.plattform.rolle}</span>
        </h2>
        <p class="text-sm opacity-70">
          Stamdata på tvers av selskaper og byråer — alle kall logges.
        </p>
      </div>
    </a>
  {/if}

  {#if nyBruker}
    <h1 class="text-lg mb-1">Velkommen, {me.name || me.email || ""}!</h1>
    <p class="opacity-70 mb-4">Hva beskriver deg best?</p>
    <Registrering onFirmsChanged={lastByraer} />
  {:else}
    <h1 class="text-lg mb-4">Hei, {me.name || me.email || ""} — velg selskap:</h1>
    <div class="grid gap-4 sm:grid-cols-2">
      {#each me.companies as c (c.company_id)}
        <a
          href={"#/c/" + c.company_id + "/oversikt"}
          class="card card-border bg-base-100 transition-colors hover:border-primary"
        >
          <div class="card-body">
            <h2 class="card-title">{c.name}</h2>
            <p class="text-sm opacity-70">{c.orgnr} · {c.access} via {c.via}</p>
          </div>
        </a>
      {:else}
        <p class="opacity-70">Ingen selskaper ennå.</p>
      {/each}
    </div>

    {#if firms.length}
      <h2 class="text-lg mt-8 mb-4">Mine byråer</h2>
      <div class="grid gap-4 sm:grid-cols-2">
        {#each firms as f (f.firm_id)}
          <a href={"#/byra/" + f.firm_id} class="card card-border bg-base-100 hover:border-primary">
            <div class="card-body">
              <h2 class="card-title">
                {f.name}
                {#if f.pending_requests > 0}
                  <span class="badge badge-primary">{f.pending_requests} nye</span>
                {/if}
              </h2>
              <p class="text-sm opacity-70">{f.kind}{f.verified ? " · autorisert" : ""}</p>
            </div>
          </a>
        {/each}
      </div>
    {/if}

    <div class="collapse collapse-arrow bg-base-100 border border-base-200 mt-8">
      <input type="checkbox" />
      <div class="collapse-title font-semibold">Registrer nytt selskap eller byrå</div>
      <div class="collapse-content">
        <Registrering onFirmsChanged={lastByraer} />
      </div>
    </div>
  {/if}
</main>
