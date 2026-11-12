<script>
  // Skattekort fra Skatteetaten (docs/lonn.md): hentes for alle som er
  // ansatt i året, og settes på den ansatte bare der lønnskjøringen regner
  // riktig. Resten står med begrunnelsen — trekket må da føres for hånd.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, onDone } = $props();

  const iAar = new Date().getFullYear();
  // Kortene for neste år kommer i desember.
  const aar = new Date().getMonth() === 11 ? [iAar, iAar + 1] : [iAar];

  let data = $state(null);
  let valgtAar = $state(iAar);
  let arbeider = $state(false);

  const RESULTAT = {
    skattekortopplysningerOK: "Skattekort",
    ikkeSkattekort: "Ikke skattekort",
    ikkeTrekkplikt: "Ikke trekkplikt",
  };

  async function hent() {
    try {
      data = await api("/companies/" + companyId + "/tax-cards");
    } catch (error) {
      data = { feil: error.message };
    }
  }

  $effect(() => {
    companyId;
    hent();
  });

  async function bestill() {
    arbeider = true;
    try {
      const b = await post("/companies/" + companyId + "/tax-cards?year=" + valgtAar, {});
      toast(
        b.besvart ? "Skattekortene er hentet" : "Bestilt — svaret er ikke klart ennå",
        true,
      );
      await hent();
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      arbeider = false;
    }
  }

  async function hentSvar(id) {
    arbeider = true;
    try {
      await post("/companies/" + companyId + "/tax-cards/" + id + "/answer", {});
      await hent();
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      arbeider = false;
    }
  }

  function trekk(k) {
    if (k.satt === "prosent") return (k.trekk_prosent_bp / 100).toFixed(2).replace(".", ",") + " %";
    if (k.satt === "tabell") return "tabell " + k.trekk_tabell;
    if (k.satt === "ingen") return "ingen trekk";
    return "–";
  }

  let siste = $derived(data?.bestillinger?.[0]);
</script>

<Card title="Skattekort">
  {#if !data}
    <span class="loading loading-spinner"></span>
  {:else if data.feil}
    <p class="text-sm opacity-60">{data.feil}</p>
  {:else}
    {#if data.henting_mulig}
      <div class="flex flex-wrap items-center gap-2 mb-3">
        <select class="select select-sm" bind:value={valgtAar} disabled={arbeider}>
          {#each aar as a}<option value={a}>{a}</option>{/each}
        </select>
        <button class="btn btn-sm btn-primary" onclick={bestill} disabled={arbeider}>
          {#if arbeider}<span class="loading loading-spinner loading-xs"></span>{/if}
          Hent skattekort fra Skatteetaten
        </button>
      </div>
    {:else}
      <p class="text-sm opacity-60 mb-2">
        Henting av skattekort er ikke satt opp på denne serveren — registrer trekket for hånd.
      </p>
    {/if}
    {#if siste}
      <p class="text-xs opacity-60 mb-1">
        {siste.inntektsaar} · {siste.referanse} · bestilt av {siste.bestilt_av}
        {new Date(siste.bestilt_tidspunkt).toLocaleString("nb-NO")}
      </p>
      {#if !siste.besvart}
        <button class="btn btn-xs btn-outline" onclick={() => hentSvar(siste.id)} disabled={arbeider}>
          Hent svaret
        </button>
      {:else}
        <div class="overflow-x-auto">
          <table class="table table-sm">
            <thead><tr><th>Ansatt</th><th>Skatteetaten</th><th>Satt</th><th>Merknad</th></tr></thead>
            <tbody>
              {#each siste.skattekort as k}
                <tr>
                  <td>{k.navn}</td>
                  <td>{RESULTAT[k.resultat] ?? k.resultat}</td>
                  <td>{trekk(k)}</td>
                  <td class="text-xs">{k.merknad ?? ""}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  {/if}
</Card>
