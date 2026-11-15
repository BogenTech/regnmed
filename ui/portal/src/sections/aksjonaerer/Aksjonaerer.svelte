<script>
  // Aksjeeierbok og aksjonærregisteroppgaven (#43): to ting som ofte
  // blandes, holdt fra hverandre. Boken er lovpålagt i seg selv
  // (aksjeloven §4-5); oppgaven er innrapportering (docs/aksjonaer.md).
  //
  // Each card loads on its own with its own retry, so one failed call
  // never leaves the whole page spinning; after a change only what it
  // touched is fetched again.
  import { api } from "../../lib/api.js";
  import { harRett } from "../../lib/me.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import Aksjeeierbok from "./Aksjeeierbok.svelte";
  import Hendelser from "./Hendelser.svelte";
  import Utbytte from "./Utbytte.svelte";
  import Oppgave from "./Oppgave.svelte";

  let { companyId } = $props();

  let kanSkrive = $derived(harRett(companyId, "AKSJEBOK_SKRIV"));

  // null = loading, { error } = failed, otherwise the data.
  let bok = $state(null);
  let hendelser = $state(null);
  let utbytte = $state(null);
  let typer = $state(null);
  // Bumped after any change, so the filing preview reflects it.
  let versjon = $state(0);

  function laster(sett, sti, velg) {
    const id = companyId;
    return api("/companies/" + id + sti)
      .then((svar) => id === companyId && sett(velg(svar)))
      .catch((error) => id === companyId && sett({ error: error.message }));
  }
  const lastBok = () => laster((v) => (bok = v), "/shareholders", (s) => s);
  const lastHendelser = () => laster((v) => (hendelser = v), "/share-events", (s) => s.hendelser);
  const lastUtbytte = () => laster((v) => (utbytte = v), "/dividends", (s) => s.utbytte);
  const lastTyper = () =>
    laster((v) => (typer = v), "/shareholders/transaction-types", (s) => s.typer);

  $effect(() => {
    companyId;
    bok = hendelser = utbytte = typer = null;
    lastBok();
    lastHendelser();
    lastUtbytte();
    lastTyper();
  });

  async function etterHendelse() {
    await Promise.all([lastBok(), lastHendelser()]);
    versjon++;
  }
</script>

{#snippet venter(data, retry, hva)}
  {#if !data}
    <span class="loading loading-spinner" aria-label={"Laster " + hva}></span>
  {:else}
    <LoadError error={data.error} {retry} />
  {/if}
{/snippet}

{#if bok && !bok.error}
  <Aksjeeierbok
    {companyId}
    {bok}
    {kanSkrive}
    onChanged={async () => (await Promise.all([lastBok(), lastHendelser()]), versjon++)}
  />
{:else}
  {@render venter(bok, () => ((bok = null), lastBok()), "aksjeeierboken")}
{/if}

{#if Array.isArray(hendelser) && Array.isArray(typer) && bok && !bok.error}
  <Hendelser
    {companyId}
    {hendelser}
    aksjonarer={bok.aksjonarer}
    {typer}
    {kanSkrive}
    onChanged={etterHendelse}
  />
{:else if hendelser?.error || typer?.error}
  <LoadError
    error={hendelser?.error || typer?.error}
    retry={() => {
      hendelser = typer = null;
      lastHendelser();
      lastTyper();
    }}
  />
{/if}

{#if Array.isArray(utbytte)}
  <Utbytte
    {companyId}
    {utbytte}
    {kanSkrive}
    onChanged={async () => (await lastUtbytte(), versjon++)}
  />
{:else}
  {@render venter(utbytte, () => ((utbytte = null), lastUtbytte()), "utbyttene")}
{/if}

<Oppgave {companyId} {kanSkrive} {versjon} />
