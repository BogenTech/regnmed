<script>
  // Abonnementskortet (#65/#74): status, plan, kort og oppsigelse.
  // Portalen VISER, serveren håndhever — også oppsigelsen.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { formatDate, kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import { bruktekst, minsteSomPasser } from "../../lib/pakke.js";
  import { harRett, loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, abo, onDone } = $props();

  let kanStyre = $derived(harRett(companyId, "SELSKAP_ADMIN"));
  let plan = $state(untrack(() => abo.planer?.[0]?.plan || ""));
  let interval = $state("month");
  // One action at a time: a double click must not open two checkouts.
  let opptatt = $state(null); // "kort" | "start" | "opp" | "bytt"

  let bruk = $derived(abo.bruk);
  let forslag = $derived(minsteSomPasser(abo.planer, bruk));
  // A plan's limits in one line, for the plan list and the change dialog.
  const grenser = (p) =>
    [
      p.bilag_grense != null ? "opptil " + p.bilag_grense + " bilag/år" : "ubegrenset bilag",
      p.ansatte_grense != null ? p.ansatte_grense + " i lønn" : null,
      p.integrasjoner_grense != null ? p.integrasjoner_grense + " integrasjoner" : null,
    ]
      .filter(Boolean)
      .join(", ");

  let statusTekst = $derived(
    {
      aktiv: "Aktivt",
      prove: "Prøvetid til " + formatDate(abo.dato),
      frist: "Ubetalt, sperres " + formatDate(abo.dato),
      sperret: "Sperret siden " + formatDate(abo.dato),
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

  // The price for the chosen interval — a yearly plan is shown per year.
  const pris = (p) =>
    interval === "year"
      ? kr(p.pris_ore_per_ar ?? p.pris_ore_per_mnd * 12) + " kr/år"
      : kr(p.pris_ore_per_mnd) + " kr/mnd";

  async function leggTilKort() {
    if (opptatt) return;
    opptatt = "kort";
    try {
      const svar = await post("/companies/" + companyId + "/subscription/card-setup", {});
      location.href = svar.url; // Stripe hosted checkout — kortdata berører aldri oss
    } catch (error) {
      toast(error.message, false);
      opptatt = null;
    }
  }

  async function start() {
    if (opptatt) return;
    opptatt = "start";
    try {
      const svar = await post("/companies/" + companyId + "/subscription", { plan, interval });
      if (svar.url) {
        location.href = svar.url; // Stripe Checkout i abonnementsmodus
        return;
      }
      toast("Abonnementet er aktivt", true);
      await loadMe(true); // /me-status er endret — hent på nytt
      opptatt = null;
      onDone();
    } catch (error) {
      toast(error.message, false);
      opptatt = null;
    }
  }

  // Bytt pakke: kortabonnement (Stripe) byttes via drift, serveren sier fra.
  async function byttPakke() {
    if (opptatt) return;
    const andre = (abo.planer || []).filter((p) => p.plan !== abo.pakke?.slug);
    if (!andre.length) {
      toast("Det finnes ingen annen pakke å bytte til nå", false);
      return;
    }
    const svar = await skjema(
      "Bytt pakke",
      [
        {
          navn: "plan",
          etikett: "Ny pakke",
          type: "select",
          standard: (andre.find((p) => p.plan === forslag?.plan) ?? andre[0]).plan,
          valg: andre.map((p) => [p.plan, p.navn + ", " + kr(p.pris_ore_per_mnd) + " kr/mnd eks. mva (" + grenser(p) + ")"]),
        },
      ],
      {
        ok: "Bytt pakke",
        melding:
          (forslag ? "Den minste pakken som passer bruken i dag er " + forslag.navn + ". " : "") +
          "Byttet gjelder fra i dag, og prisen følger den nye pakken.",
      },
    );
    if (!svar) return;
    opptatt = "bytt";
    try {
      const res = await post("/companies/" + companyId + "/subscription/change", { plan: svar.plan });
      toast("Pakken er byttet fra " + formatDate(res.fra), true);
      await loadMe(true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      opptatt = null;
    }
  }

  async function siOpp() {
    if (opptatt) return;
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
    opptatt = "opp";
    try {
      const svar = await post("/companies/" + companyId + "/subscription/cancel", {});
      toast(
        svar.gjelder_til
          ? "Sagt opp. Abonnementet gjelder ut " + formatDate(svar.gjelder_til)
          : "Abonnementet er sagt opp",
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      opptatt = null;
    }
  }
</script>

<Card title="Abonnement">
  <p class="text-sm mb-2 flex flex-wrap items-center gap-x-2 gap-y-1">
    Status: <span class="badge {statusFarge} badge-sm">{statusTekst}</span>
    {#if abo.kort}<span>Kort {abo.kort.brand} som slutter på {abo.kort.last4}</span>{/if}
  </p>

  {#if oppsagt}
    <p class="text-sm mb-2">
      Abonnementet er <b>sagt opp</b> og gjelder ut <b>{formatDate(abo.abonnement.sies_opp)}</b>.
      Du beholder alt du har betalt for fram til da.
    </p>
  {:else if loper}
    <p class="text-sm mb-2">
      Løpende {abo.abonnement.interval === "year" ? "årsabonnement" : "månedsabonnement"} på planen
      <b>{abo.pakke?.navn ?? abo.abonnement.plan}</b>. Det fornyes automatisk til du sier det opp.
    </p>
  {/if}

  {#if abo.pakke || bruk}
    <div class="text-sm mb-3">
      {#if abo.pakke}<p>Pakke: <b>{abo.pakke.navn}</b></p>{/if}
      {#if bruk}
        <ul class="mt-1 space-y-0.5">
          <li class={bruk.over_bilag ? "text-warning font-medium" : ""}>
            {bruktekst(bruk.bilag, bruk.bilag_grense, "bilag i år")}
          </li>
          <li>{bruktekst(bruk.ansatte, bruk.ansatte_grense, "ansatte i lønn")}</li>
          <li>{bruktekst(bruk.integrasjoner, bruk.integrasjoner_grense, "integrasjoner og agenter")}</li>
        </ul>
        {#if bruk.over_bilag}
          <p class="mt-2">
            Selskapet har flere bilag enn pakken er laget for. Bokføringen fortsetter som før{forslag
              ? ", men " + forslag.navn + " passer bedre"
              : ""}.
          </p>
        {/if}
      {/if}
    </div>
  {/if}

  <p class="text-sm opacity-70 mb-3">
    Alle funksjoner er med i alle pakker; pakkene skiller på størrelse. Selv om abonnementet sperres,
    kan du alltid lese, eksportere og styre selskapet: hovedboken tas aldri som gissel.
  </p>

  {#if kanStyre && abo.pakke && abo.status === "aktiv"}
    <div class="mb-3">
      <button class="btn btn-sm btn-outline" disabled={!!opptatt} onclick={byttPakke}>
        {#if opptatt === "bytt"}<span class="loading loading-spinner loading-xs"></span>{/if}
        Bytt pakke
      </button>
    </div>
  {/if}

  {#if !kanStyre}
    <p class="text-xs opacity-60">Abonnementet styres av selskapets administrator.</p>
  {:else if abo.kort_mulig}
    {#if !loper && !abo.abonnement && abo.status !== "aktiv"}
      <div class="flex flex-wrap items-end gap-2 mb-3">
        <fieldset class="fieldset py-0">
          <legend class="fieldset-legend">Plan</legend>
          <select class="select select-sm" bind:value={plan}>
            {#each abo.planer || [] as p (p.plan)}
              <option value={p.plan}>{p.navn ?? p.plan}, {pris(p)} eks. mva ({grenser(p)})</option>
            {/each}
          </select>
        </fieldset>
        <fieldset class="fieldset py-0">
          <legend class="fieldset-legend">Betales</legend>
          <select class="select select-sm" bind:value={interval}>
            <option value="month">Månedlig</option>
            <option value="year">Årlig</option>
          </select>
        </fieldset>
        <button class="btn btn-sm btn-primary" disabled={!!opptatt} onclick={start}>
          {#if opptatt === "start"}<span class="loading loading-spinner loading-xs"></span>{/if}
          Start abonnement
        </button>
      </div>
    {/if}
    <div class="flex gap-2 flex-wrap items-center">
      <button class="btn btn-sm btn-outline" disabled={!!opptatt} onclick={leggTilKort}>
        {#if opptatt === "kort"}<span class="loading loading-spinner loading-xs"></span>{/if}
        {abo.kort ? "Bytt kort" : "Legg til kort"}
      </button>
      {#if loper}
        <button class="btn btn-sm btn-outline btn-error" disabled={!!opptatt} onclick={siOpp}>
          {#if opptatt === "opp"}<span class="loading loading-spinner loading-xs"></span>{/if}
          Si opp abonnement
        </button>
      {/if}
    </div>
  {:else}
    <p class="text-xs opacity-60">
      Kortbetaling er ikke satt opp på denne installasjonen. Abonnement avtales med drift.
    </p>
  {/if}
</Card>
