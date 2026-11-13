<script>
  // New claims start here. Receipts first — as many as you have: each one
  // becomes a draft at once, and the details can wait (finish them now,
  // later, or on another device). A trip goes to its own form.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/expenses");
  const list = $derived("#/c/" + companyId + "/utlegg");

  let people = $state([]);
  let claimant = $state("");
  let progress = $state(null); // { done, total }

  $effect(() => {
    if (harRett(companyId, "UTLEGG_GODKJENN")) {
      api(base + "/claimants").then((r) => (people = r.people)).catch(() => {});
    }
  });

  async function picked(event) {
    const files = [...event.target.files];
    event.target.value = "";
    if (!files.length || progress) return;
    progress = { done: 0, total: files.length };
    const made = [];
    // One at a time: a phone on a weak connection should not start ten
    // uploads that all time out together.
    for (const file of files) {
      try {
        const r = await api(
          base + "/receipts?filename=" + encodeURIComponent(file.name) + (claimant ? "&person_id=" + claimant : ""),
          { method: "POST", headers: { "content-type": file.type || "application/octet-stream" }, body: file },
        );
        made.push(r.expense_id);
      } catch (err) {
        toast(file.name + ": " + err.message, false);
      }
      progress.done += 1;
    }
    progress = null;
    if (!made.length) return;
    toast(made.length === 1 ? "Kvitteringen er lagret som utkast" : made.length + " kvitteringer er lagret som utkast", true);
    location.hash = made.length === 1 ? list + "/" + made[0] : list + "/utkast";
  }
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={list}>← Alle utlegg</a>
</nav>
<h1 class="text-2xl font-bold mb-4">Nytt krav</h1>

<div class="grid gap-4 md:grid-cols-2 max-w-4xl">
  <section class="card border border-base-300 bg-base-100" aria-labelledby="kvitteringer-tittel">
    <div class="card-body grid gap-3">
      <h2 id="kvitteringer-tittel" class="card-title">Utlegg med kvittering</h2>
      <p class="text-sm opacity-70">
        Ta bilde eller velg filer — gjerne mange på en gang. Hver kvittering blir et utkast du fyller ut nå eller
        senere, og du kan rotere og beskjære bildet før du sender inn.
      </p>
      {#if people.length > 1}
        <label class="fieldset">
          <span class="fieldset-legend">Hvem skal ha pengene?</span>
          <select class="select w-full" bind:value={claimant}>
            <option value="">Meg selv</option>
            {#each people as p (p.person_id)}<option value={p.person_id}>{p.name}</option>{/each}
          </select>
        </label>
      {/if}
      {#if progress}
        <div role="status" aria-live="polite" class="grid gap-1">
          <span class="text-sm">Laster opp {progress.done + 1} av {progress.total} …</span>
          <progress class="progress progress-primary" value={progress.done} max={progress.total}></progress>
        </div>
      {:else}
        <div class="flex flex-wrap gap-2">
          <label class="btn btn-primary focus-within:outline-2 focus-within:outline-offset-2">
            Ta bilde
            <input type="file" class="sr-only" accept="image/*" capture="environment" onchange={picked} />
          </label>
          <!-- No `capture`: a phone then offers saved photos and files too. -->
          <label class="btn focus-within:outline-2 focus-within:outline-offset-2">
            Velg filer
            <input type="file" class="sr-only" accept="image/*,application/pdf" multiple onchange={picked} />
          </label>
        </div>
      {/if}
    </div>
  </section>

  <section class="card border border-base-300 bg-base-100" aria-labelledby="kjoring-tittel">
    <div class="card-body grid gap-3">
      <h2 id="kjoring-tittel" class="card-title">Kjøring</h2>
      <p class="text-sm opacity-70">
        Kilometergodtgjørelse etter statens satser, med strekning, passasjerer og bompenger, ferje og parkering
        på samme tur.
      </p>
      <a class="btn w-fit" href={list + "/kjoring"}>Registrer kjøring</a>
    </div>
  </section>
</div>
