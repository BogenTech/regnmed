<script>
  // Abonnementskortet (#65/#74): status, plan, kort og oppsigelse.
  // Portalen VISER, serveren håndhever — også oppsigelsen.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, abo, onDone } = $props();

  let plan = $state(untrack(() => abo.planer?.[0]?.plan || ""));
  let interval = $state("month");
  let sierOpp = $state(false);

  let statusTekst = $derived(
    {
      aktiv: "Aktivt",
      prove: "Prøvetid til " + (abo.dato || ""),
      frist: "Ubetalt — sperres " + (abo.dato || ""),
      sperret: "Sperret siden " + (abo.dato || ""),
    }[abo.status] || abo.status,
  );

  // Samme fargespråk som AbonnementBanner: statusen skal se lik ut der den
  // varsles og der den vises.
  let statusFarge = $derived(
    {
      aktiv: "badge-success",
      prove: "badge-info",
      frist: "badge-warning",
      sperret: "badge-error",
    }[abo.status] || "badge-ghost",
  );

  // Løpende Stripe-abonnement: fornyes til det sies opp.
  let loper = $derived(abo.abonnement && !abo.abonnement.sies_opp);
  let oppsagt = $derived(abo.abonnement?.sies_opp);

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
      const svar = await post("/companies/" + companyId + "/subscription", { plan, interval });
      if (svar.url) {
        location.href = svar.url; // Stripe Checkout i abonnementsmodus
        return;
      }
      toast("Abonnementet er aktivt", true);
      await loadMe(true); // /me-status er endret — hent på nytt
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function siOpp() {
    // Bevisst friksjon: en oppsigelse skal ikke skje ved et uhell.
    if (
      !(await bekreft("Si opp abonnementet? Det løper ut perioden du har betalt for.", {
        tittel: "Si opp abonnementet",
        ok: "Si opp",
        farlig: true,
      }))
    ) {
      return;
    }
    sierOpp = true;
    try {
      const svar = await post("/companies/" + companyId + "/subscription/cancel", {});
      toast(
        svar.gjelder_til
          ? "Sagt opp — abonnementet gjelder ut " + svar.gjelder_til
          : "Abonnementet er sagt opp",
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      sierOpp = false;
    }
  }
</script>

<Card title="Abonnement">
  <p class="text-sm mb-2">
    Status: <span class="badge {statusFarge} badge-sm">{statusTekst}</span>{abo.kort
      ? " · kort " + abo.kort.brand + " •••• " + abo.kort.last4
      : ""}
  </p>

  {#if oppsagt}
    <p class="text-sm mb-2">
      Abonnementet er <b>sagt opp</b> og gjelder ut <b>{abo.abonnement.sies_opp}</b>. Du beholder
      alt du har betalt for fram til da.
    </p>
  {:else if loper}
    <p class="text-sm mb-2">
      Løpende {abo.abonnement.interval === "year" ? "årsabonnement" : "månedsabonnement"} på planen
      <b>{abo.abonnement.plan}</b> — fornyes automatisk til du sier det opp.
    </p>
  {/if}

  <p class="text-sm opacity-70 mb-2">
    Alt er inkludert i begge planer — forskjellen er supportkanalen. Selv om abonnementet sperres,
    kan du alltid lese, eksportere og styre selskapet: hovedboken tas aldri som gissel.
  </p>

  {#if abo.kort_mulig}
    <div class="flex gap-2 flex-wrap items-center">
      <button class="btn btn-sm btn-outline" onclick={leggTilKort}>
        {abo.kort ? "Bytt kort" : "Legg til kort"}
      </button>

      {#if loper}
        <button class="btn btn-sm btn-outline btn-error" onclick={siOpp} disabled={sierOpp}>
          {sierOpp ? "Sier opp …" : "Si opp abonnement"}
        </button>
      {:else if !abo.abonnement && abo.status !== "aktiv"}
        <select class="select select-sm" bind:value={plan}>
          {#each abo.planer || [] as p}
            <option value={p.plan}>{p.plan} — {kr(p.pris_ore_per_mnd)} kr/mnd eks. mva</option>
          {/each}
        </select>
        <select class="select select-sm" bind:value={interval}>
          <option value="month">Månedlig</option>
          <option value="year">Årlig</option>
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
