<script>
  // Abonnementskortet (#65/#74): status, plan og betalingskort.
  // Kort er standardveien — portalen viser, SERVEREN håndhever.
  import { post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, abo, onDone } = $props();

  let plan = $state(abo.planer?.[0]?.plan || "");

  let statusTekst = $derived(
    {
      aktiv: "Aktivt",
      prove: "Prøvetid til " + (abo.dato || ""),
      frist: "Ubetalt — sperres " + (abo.dato || ""),
      sperret: "Sperret siden " + (abo.dato || ""),
    }[abo.status] || abo.status,
  );

  async function leggTilKort() {
    try {
      const svar = await post("/companies/" + companyId + "/subscription/card-setup", {});
      location.href = svar.url; // Stripe hosted checkout — kortdata berører aldri oss
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function start() {
    try {
      await post("/companies/" + companyId + "/subscription", { plan });
      toast("Abonnementet er aktivt", true);
      await loadMe(true); // /me-status er endret — hent på nytt
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Abonnement">
  <p class="text-sm mb-2">
    Status: <b>{statusTekst}</b>{abo.kort ? " · kort " + abo.kort.brand + " •••• " + abo.kort.last4 : ""}
  </p>
  <p class="text-sm opacity-70 mb-2">
    Alt er inkludert i begge planer — forskjellen er supportkanalen. Kort er standardveien;
    beløpet trekkes automatisk for hver månedsfaktura.
  </p>
  {#if abo.kort_mulig}
    <div class="flex gap-2 flex-wrap items-center">
      <button class="btn btn-sm btn-outline" onclick={leggTilKort}>
        {abo.kort ? "Bytt kort" : "Legg til kort"}
      </button>
      {#if abo.status !== "aktiv"}
        <select class="select select-sm select-bordered" bind:value={plan}>
          {#each abo.planer || [] as p}
            <option value={p.plan}>{p.plan} — {kr(p.pris_ore_per_mnd)} kr/mnd eks. mva</option>
          {/each}
        </select>
        <button class="btn btn-sm btn-primary" onclick={start}>Start abonnement</button>
      {/if}
    </div>
  {:else}
    <p class="text-xs opacity-60">
      Kortbetaling er ikke satt opp på denne installasjonen — abonnement avtales med drift.
    </p>
  {/if}
</Card>
