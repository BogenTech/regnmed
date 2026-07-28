<script>
  import { api, post } from "../lib/api.js";
  import { me, loadMe } from "../lib/me.svelte.js";
  import { logout } from "../lib/auth.svelte.js";
  import { toast } from "../lib/toast.svelte.js";
  import ThemeControls from "./ThemeControls.svelte";

  let firms = $state([]);
  let orgnr = $state("");
  let preview = $state(null);

  $effect(() => {
    api("/firms/mine")
      .then((mine) => (firms = mine.firms || []))
      .catch(() => {
        /* seksjonen er valgfri */
      });
  });

  async function slaOpp() {
    preview = null;
    try {
      preview = await api("/registry/enheter/" + encodeURIComponent(orgnr.trim()));
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function opprett() {
    try {
      const created = await post("/companies", { orgnr: orgnr.trim() });
      toast(created.navn + " opprettet med " + created.seeded_accounts + " kontoer", true);
      preview = null;
      await loadMe(true);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<div class="navbar bg-base-100 shadow-sm">
  <div class="flex-1"><span class="btn btn-ghost text-xl">regnmed</span></div>
  <div class="flex-none gap-2">
    <ThemeControls />
    <button class="btn btn-ghost btn-sm" onclick={logout}>Logg ut</button>
  </div>
</div>
<main class="p-6 max-w-3xl mx-auto">
  <h1 class="text-lg mb-4">Hei, {me.name || me.email || ""} — velg selskap:</h1>
  <div class="grid gap-4 sm:grid-cols-2">
    {#each me.companies as c (c.company_id)}
      <a
        href={"#/c/" + c.company_id + "/oversikt"}
        class="card bg-base-100 shadow-sm hover:shadow-md transition-shadow"
      >
        <div class="card-body">
          <h2 class="card-title">{c.name}</h2>
          <p class="text-sm opacity-70">{c.orgnr} · {c.access} via {c.via}</p>
        </div>
      </a>
    {:else}
      <p class="opacity-70">Ingen selskaper ennå — be om tilgang, et oppdrag, eller start under.</p>
    {/each}
  </div>

  <div class="card bg-base-100 shadow-sm mt-8">
    <div class="card-body">
      <h2 class="card-title">Nytt selskap fra Enhetsregisteret</h2>
      <div class="flex gap-2">
        <input
          class="input input-bordered"
          placeholder="Organisasjonsnummer"
          maxlength="9"
          bind:value={orgnr}
        />
        <button class="btn" onclick={slaOpp}>Slå opp</button>
      </div>
      {#if preview}
        <div class="mt-4 p-4 bg-base-200 rounded-box">
          <p class="font-semibold">{preview.navn} ({preview.organisasjonsform || ""})</p>
          <p class="text-sm opacity-70">{preview.naeringskode || ""}</p>
          <div class="flex gap-2 mt-2">
            {#if preview.mva_registrert}<span class="badge badge-ghost">MVA-registrert</span>{/if}
            {#if preview.autorisasjon.regnskap}
              <span class="badge badge-success">Autorisert regnskapsførerselskap</span>
            {/if}
            {#if preview.autorisasjon.revisjon}
              <span class="badge badge-success">Autorisert revisjonsselskap</span>
            {/if}
            {#if preview.konkurs}<span class="badge badge-error">Konkurs</span>{/if}
          </div>
          <button class="btn btn-primary btn-sm mt-4" onclick={opprett}>Opprett selskap</button>
        </div>
      {/if}
    </div>
  </div>

  {#if firms.length}
    <h2 class="text-lg mt-8 mb-4">Mine byråer</h2>
    <div class="grid gap-4 sm:grid-cols-2">
      {#each firms as f (f.firm_id)}
        <a href={"#/byra/" + f.firm_id} class="card bg-base-100 shadow-sm hover:shadow-md">
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
</main>
