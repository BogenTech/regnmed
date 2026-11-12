<script>
  // Skattekort fra Skatteetaten (docs/lonn.md): hentes for alle som er
  // ansatt i året, og settes på den ansatte bare der lønnskjøringen regner
  // riktig. Resten står med begrunnelsen — trekket må da føres for hånd.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, onDone } = $props();

  // Bare inneværende år: kortet settes på den ansatte og gjelder hver
  // kjøring, så neste års kort hentes i januar (serveren nekter ellers).
  const iAar = new Date().getFullYear();

  let data = $state(null);
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
      const b = await post("/companies/" + companyId + "/tax-cards?year=" + iAar, {});
      toast(
        b.besvart ? "Skattekortene er hentet" : "Bestilt — svaret er ikke klart ennå",
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      arbeider = false;
      // Også etter en feil: en bestilling som ble lagret skal synes, så
      // ingen bestiller på nytt fordi den forrige ikke vistes.
      await hent();
    }
  }

  async function hentSvar(id) {
    arbeider = true;
    try {
      const b = await post("/companies/" + companyId + "/tax-cards/" + id + "/answer", {});
      if (!b.besvart) toast("Svaret fra Skatteetaten er ikke klart ennå — prøv igjen om litt", false);
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
        <button class="btn btn-sm btn-primary" onclick={bestill} disabled={arbeider}>
          {#if arbeider}<span class="loading loading-spinner loading-xs"></span>{/if}
          Hent skattekort for {iAar} fra Skatteetaten
        </button>
      </div>
    {:else}
      <p class="text-sm opacity-60 mb-2">
        Henting av skattekort er ikke satt opp på denne serveren — registrer trekket for hånd.
      </p>
    {/if}
    {#if siste}
      <p class="text-xs opacity-60 mb-1">
        {siste.inntektsaar} · {siste.referanse ?? "ikke mottatt"} · bestilt av {siste.bestilt_av}
        {new Date(siste.bestilt_tidspunkt).toLocaleString("nb-NO")}
      </p>
      {#if siste.feil}
        <div role="alert" class="alert alert-error alert-soft text-xs">
          <span>Skatteetaten avviste bestillingen: {siste.feil}</span>
        </div>
      {:else if !siste.besvart}
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
