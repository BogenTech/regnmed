<script>
  // Aksjeeierbok og aksjonærregisteroppgaven (#43): to ting som ofte
  // blandes, holdt fra hverandre. Boken er lovpålagt i seg selv
  // (aksjeloven §4-5); oppgaven er innrapportering (docs/aksjonaer.md).
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Aksjeeierbok from "./Aksjeeierbok.svelte";
  import Hendelser from "./Hendelser.svelte";
  import Utbytte from "./Utbytte.svelte";
  import Oppgave from "./Oppgave.svelte";

  let { companyId } = $props();

  // Oppgaven for et år leveres i januar året etter (frist 31. januar), så
  // fram til juni står forrige inntektsår først.
  const now = new Date();
  const thisYear = now.getFullYear();
  let year = $state(now.getMonth() < 5 ? thisYear - 1 : thisYear);

  let data = $state(null);

  async function load(id) {
    const [bok, hendelser, utbytte, typer, oppgave] = await Promise.all([
      api("/companies/" + id + "/shareholders"),
      api("/companies/" + id + "/share-events"),
      api("/companies/" + id + "/dividends"),
      api("/companies/" + id + "/shareholders/transaction-types"),
      api("/companies/" + id + "/reports/aksjonaeroppgave?year=" + year).catch(() => null),
    ]);
    data = {
      bok,
      hendelser: hendelser.hendelser,
      utbytte: utbytte.utbytte,
      typer: typer.typer,
      oppgave,
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Aksjeeierbok {companyId} bok={data.bok} onDone={reload} />
  <Hendelser
    {companyId}
    hendelser={data.hendelser}
    aksjonarer={data.bok.aksjonarer}
    typer={data.typer}
    onDone={reload}
  />
  <Utbytte {companyId} utbytte={data.utbytte} onDone={reload} />
  <div class="flex justify-end mb-2">
    <select
      class="select select-sm w-fit"
      aria-label="Inntektsår for aksjonærregisteroppgaven"
      bind:value={year}
      onchange={reload}
    >
      {#each [thisYear, thisYear - 1, thisYear - 2] as y (y)}
        <option value={y}>Inntektsår {y}</option>
      {/each}
    </select>
  </div>
  {#if data.oppgave}
    <Oppgave {companyId} oppgave={data.oppgave} {year} />
  {:else}
    <p class="text-sm opacity-70">Ingen aksjonærregisteroppgave for {year}.</p>
  {/if}
{/if}
