<script>
  // Prosjekter og avdelinger som egen seksjon — registeret bodde nederst
  // i Bilag (#37), som er det siste stedet noen som tenker i prosjekter
  // leter. Reglene er uendret: koden er PERMANENT (den inngår i
  // bilagshashen), så en dimensjon avsluttes — slettes aldri; navnet og
  // kundekoblingen (#80) er redigerbar metadata utenfor kjeden.
  //
  // Synlig for alle med DIMENSJON_LES (også ansatte — de fører timer på
  // prosjektene); alt som skriver vises bare med DIMENSJON_SKRIV, og
  // serveren håndhever uansett (docs/auth.md).
  //
  // A list of stacked rows rather than a nine-column table, so it fits a
  // phone; editing opens in place under one row at a time.
  import { tick } from "svelte";
  import { api } from "../../lib/api.js";
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import Endre from "./Endre.svelte";
  import NyDimensjon from "./NyDimensjon.svelte";
  import { delOpp } from "./dimensjon.js";

  let { companyId } = $props();

  let kanSkrive = $derived(harRett(companyId, "DIMENSJON_SKRIV"));
  // Satsene er TIMER_SATS_SKRIV sitt territorium (docs/timer.md): uten
  // retten finnes verken editoren eller andres satser i svarene.
  let kanSatser = $derived(harRett(companyId, "TIMER_SATS_SKRIV"));
  let kanMedlemmer = $derived(harRett(companyId, "MEDLEM_ADMIN"));
  let serTimer = $derived(harRett(companyId, "TIMER_RAPPORT_LES"));

  // null = loading, { error } = failed, otherwise the register.
  let dims = $state(null);
  // Only fetched for whoever can use them; null = not shown.
  let kunder = $state(null);
  let timer = $state(null);

  // The one row open for editing: "kind/code".
  let apen = $state(null);
  const nokkel = (d) => d.kind + "/" + d.code;

  // After an edit only the register changes — not the customers, not a
  // year of hours.
  async function lastDims(id = companyId) {
    try {
      const svar = await api("/companies/" + id + "/dimensions");
      if (id === companyId) dims = svar.dimensions;
    } catch (error) {
      if (id === companyId) dims = { error: error.message };
    }
  }

  $effect(() => {
    const id = companyId;
    dims = kunder = timer = apen = null;
    lastDims(id);
    if (kanSkrive)
      api("/companies/" + id + "/parties?kind=kunde")
        .then((s) => id === companyId && (kunder = s.parties))
        .catch(() => {});
    if (serTimer) {
      const year = new Date().getFullYear();
      api(`/companies/${id}/timesheet/summary?from=${year}-01-01&to=${year}-12-31`)
        .then(
          (s) => id === companyId && (timer = new Map(s.prosjekter.map((p) => [p.prosjekt, p]))),
        )
        .catch(() => {});
    }
  });

  let liste = $derived(Array.isArray(dims) ? dims : []);
  let prosjekter = $derived(delOpp(liste, "prosjekt"));
  let avdelinger = $derived(delOpp(liste, "avdeling"));
  let visAvsluttede = $state({ prosjekt: false, avdeling: false });

  // Back to the row's button when the panel closes, so the keyboard
  // does not drop to the top of the page.
  async function lukk(d) {
    apen = null;
    await tick();
    // A project just closed has moved behind the toggle; land there.
    (
      document.getElementById("knapp-" + d.kind + "-" + d.code) ??
      document.getElementById("avsluttede-" + d.kind)
    )?.focus();
  }

  function detaljer(d) {
    const deler = [d.kunde_navn || "ingen kunde"];
    if (d.fakturerbar_default) deler.push("fakturerbar");
    if (d.min_timesats_ore != null) deler.push(kr(d.min_timesats_ore) + " kr/t");
    const t = timer?.get(d.code);
    if (t?.minutter) deler.push(minutterTilTimer(t.minutter) + " t i år");
    if (t?.ufakturert_ore) deler.push(kr(t.ufakturert_ore) + " ufakturert");
    return deler.join(", ");
  }
</script>

{#snippet rader(utvalg)}
  <ul class="divide-y divide-base-200">
    {#each utvalg as d (nokkel(d))}
      <li class="py-2">
        <div class="flex items-center gap-2">
          <div class="min-w-0 flex-1">
            <div class="truncate">
              <span class="font-medium">{d.code}</span>
              {d.name}
              {#if !d.active}<span class="badge badge-xs badge-ghost ml-1">avsluttet</span>{/if}
            </div>
            {#if d.kind === "prosjekt"}
              <div class="text-xs opacity-60">{detaljer(d)}</div>
            {/if}
          </div>
          {#if kanSkrive || (kanSatser && d.kind === "prosjekt")}
            <button
              id={"knapp-" + d.kind + "-" + d.code}
              class="btn btn-xs btn-ghost shrink-0"
              aria-expanded={apen === nokkel(d)}
              aria-label={(apen === nokkel(d) ? "Lukk " : "Endre ") + d.code + " " + d.name}
              onclick={() => (apen === nokkel(d) ? lukk(d) : (apen = nokkel(d)))}
            >
              {apen === nokkel(d) ? "Lukk" : "Endre"}
            </button>
          {/if}
        </div>
        {#if apen === nokkel(d)}
          <Endre
            {companyId}
            dim={d}
            {kunder}
            {kanSkrive}
            {kanSatser}
            {kanMedlemmer}
            onSaved={() => lastDims()}
            onClose={() => lukk(d)}
          />
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet register(kind, del, flertall)}
  {#if del.aktive.length}
    {@render rader(del.aktive)}
  {:else}
    <p class="text-sm opacity-70">Ingen {del.avsluttede.length ? "åpne " : ""}{flertall} ennå.</p>
  {/if}
  {#if del.avsluttede.length}
    <button
      id={"avsluttede-" + kind}
      class="btn btn-xs btn-ghost mt-1"
      aria-expanded={visAvsluttede[kind]}
      onclick={() => (visAvsluttede[kind] = !visAvsluttede[kind])}
    >
      {visAvsluttede[kind] ? "Skjul" : "Vis"} avsluttede ({del.avsluttede.length})
    </button>
    {#if visAvsluttede[kind]}
      {@render rader(del.avsluttede)}
    {/if}
  {/if}
  {#if kanSkrive}
    <NyDimensjon {companyId} {kind} {kunder} {kanSatser} onCreated={() => lastDims()} />
  {/if}
{/snippet}

{#if !dims}
  <span class="loading loading-spinner loading-lg" aria-label="Laster"></span>
{:else if dims.error}
  <LoadError error={dims.error} retry={() => ((dims = null), lastDims())} />
{:else}
  <Card title="Prosjekter">
    <p class="text-sm opacity-70 mb-2">
      Timer og bilag føres på et prosjekt. Er prosjektet knyttet til en kunde, foreslår fakturaen
      den kunden. Koden kan ikke endres, men navnet kan. Lønnsomheten står under
      <a class="link" href={"#/c/" + companyId + "/rapporter/prosjekt"}>Rapporter → Prosjekt</a>.
    </p>
    {@render register("prosjekt", prosjekter, "prosjekter")}
  </Card>

  <Card title="Avdelinger">
    <p class="text-sm opacity-70 mb-2">
      Resultatrapporten kan filtreres per avdeling. Koden kan ikke endres, men navnet kan.
    </p>
    {@render register("avdeling", avdelinger, "avdelinger")}
  </Card>
{/if}
