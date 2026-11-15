<script>
  // Katalogen viser bare byråer med verifisert autorisasjon. Å be om
  // oppdrag gir ingen tilgang i seg selv — byrået må godta først.
  //
  // The directory can hold hundreds of firms, so it is a search with a
  // capped list rather than one long table, and each row is a stack that
  // keeps its button on screen at phone width.
  import { tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import { BYRATYPE, VIS_MAKS, finnByraer, formatOrgnr } from "./byra.js";

  let { companyId, firms, engagements, requests, onDone } = $props();

  let sok = $state("");
  let type = $state("");

  let treff = $derived(finnByraer(firms, sok, type));
  let aktive = $derived(new Set(engagements.filter((e) => !e.valid_to).map((e) => e.firm_id)));
  let ventende = $derived(
    new Map(requests.filter((r) => r.status === "pending").map((r) => [r.firm_id, r])),
  );

  // The row's button changes from one action to the other; keep the
  // keyboard on it instead of dropping focus to the page.
  async function fokuser(firmId) {
    await tick();
    document.getElementById("byra-" + firmId)?.querySelector("button")?.focus();
  }

  async function beOm(f) {
    try {
      await post("/companies/" + companyId + "/engagement-requests", { firm_id: f.firm_id });
      toast("Forespørsel sendt til " + f.name, true);
      await onDone();
      fokuser(f.firm_id);
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekk(f, r) {
    try {
      await post(
        "/companies/" + companyId + "/engagement-requests/" + r.request_id + "/withdraw",
        {},
      );
      toast("Forespørselen til " + f.name + " er trukket tilbake", true);
      await onDone();
      fokuser(f.firm_id);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Finn et byrå">
  <p class="text-sm opacity-70 mb-2">
    Bare byråer med autorisasjon verifisert hos Finanstilsynet. Byrået må godta forespørselen før
    det får tilgang.
  </p>
  <div class="flex flex-wrap gap-2 mb-2" role="search">
    <input
      class="input input-sm flex-1 min-w-48"
      type="search"
      aria-label="Søk etter navn eller organisasjonsnummer"
      placeholder="Navn eller organisasjonsnummer"
      bind:value={sok}
    />
    <select class="select select-sm w-auto" aria-label="Type byrå" bind:value={type}>
      <option value="">Alle typer</option>
      <option value="regnskap">Regnskapsførere</option>
      <option value="revisjon">Revisorer</option>
    </select>
  </div>

  <p class="text-xs opacity-60 mb-1" aria-live="polite">
    {#if !treff.length}
      Ingen byråer passer søket.
    {:else if treff.length > VIS_MAKS}
      Viser {VIS_MAKS} av {treff.length} — søk for å snevre inn.
    {:else}
      {treff.length} {treff.length === 1 ? "byrå" : "byråer"}
    {/if}
  </p>

  <ul class="divide-y divide-base-200">
    {#each treff.slice(0, VIS_MAKS) as f (f.firm_id)}
      {@const forespurt = ventende.get(f.firm_id)}
      <li id={"byra-" + f.firm_id} class="flex items-center gap-2 py-2">
        <div class="min-w-0 flex-1">
          <div class="font-medium truncate">{f.name}</div>
          <div class="text-xs opacity-60">
            {BYRATYPE[f.kind] || f.kind}, org.nr. {formatOrgnr(f.orgnr)}, {f.client_count}
            {f.client_count === 1 ? "klient" : "klienter"}
          </div>
        </div>
        {#if aktive.has(f.firm_id)}
          <span class="badge badge-sm badge-success badge-soft shrink-0">aktivt oppdrag</span>
        {:else if forespurt}
          <span class="badge badge-sm badge-warning badge-soft shrink-0">forespurt</span>
          <button
            class="btn btn-xs btn-ghost shrink-0"
            aria-label={"Trekk tilbake forespørselen til " + f.name}
            onclick={() => trekk(f, forespurt)}>Trekk tilbake</button
          >
        {:else}
          <button
            class="btn btn-xs btn-primary shrink-0"
            aria-label={"Be " + f.name + " om oppdrag"}
            onclick={() => beOm(f)}>Be om oppdrag</button
          >
        {/if}
      </li>
    {/each}
  </ul>
</Card>
