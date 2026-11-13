<script>
  // Rapporter (#4, #24, #41): ett kort, én fane av gangen. Bare den
  // valgte rapporten hentes — hver fanekomponent laster sitt eget kall.
  // Fane, år og fanens egen tilstand står i adressen (reportAddress.js).
  import { tick, untrack } from "svelte";
  import Card from "../../components/Card.svelte";
  import YearPicker from "../../components/YearPicker.svelte";
  import Saldobalanse from "./Saldobalanse.svelte";
  import Resultat from "./Resultat.svelte";
  import Balanse from "./Balanse.svelte";
  import Budsjett from "./Budsjett.svelte";
  import Kontospesifikasjon from "./Kontospesifikasjon.svelte";
  import Reskontrospesifikasjon from "./Reskontrospesifikasjon.svelte";
  import Prosjekt from "./Prosjekt.svelte";
  import Bokforingsspesifikasjon from "./Bokforingsspesifikasjon.svelte";
  import Revisjon from "./Revisjon.svelte";
  import Forankring from "./Forankring.svelte";
  import Balansedokumentasjon from "./Balansedokumentasjon.svelte";
  import Saft from "./Saft.svelte";
  import { TABS, buildAddress, parseAddress } from "./reportAddress.js";

  let { companyId, extra, query = "" } = $props();

  const thisYear = new Date().getFullYear();
  const start = untrack(() => parseAddress(extra, query, thisYear));
  let rapport = $state(start.tab);
  let year = $state(start.year);
  // The open tab's own state; the tabs read and write it, and it rides
  // in the address with the year.
  let params = $state(start.params);

  let from = $derived(year + "-01-01");
  let to = $derived(year + "-12-31");

  // An address changed from outside (the address bar) moves the page;
  // the page's own replaceState below never reaches `extra`/`query`.
  $effect(() => {
    const next = parseAddress(extra, query, thisYear);
    untrack(() => {
      const here = buildAddress(companyId, rapport, year, params);
      if (buildAddress(companyId, next.tab, next.year, next.params) === here) return;
      rapport = next.tab;
      year = next.year;
      params = next.params;
    });
  });

  $effect(() => {
    const hash = buildAddress(companyId, rapport, year, params);
    if (location.hash !== hash) history.replaceState(null, "", hash);
  });

  function velg(key) {
    if (key === rapport) return;
    rapport = key;
    params = {};
  }

  // A new year starts the long lists from their first page again.
  function setYear(y) {
    year = y;
    delete params.side;
  }

  // Arrow keys move between the tabs, Home/End to the ends — one Tab
  // stop for the whole row (WAI-ARIA tabs pattern).
  async function tabKey(event) {
    const i = TABS.findIndex(([key]) => key === rapport);
    const next = {
      ArrowRight: (i + 1) % TABS.length,
      ArrowLeft: (i - 1 + TABS.length) % TABS.length,
      Home: 0,
      End: TABS.length - 1,
    }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    velg(TABS[next][0]);
    await tick();
    document.getElementById("rapport-tab-" + TABS[next][0])?.focus();
  }

  let tittel = $derived(
    {
      saldobalanse: "Saldobalanse " + year,
      resultat: "Resultatregnskap " + year,
      balanse: "Balanse per 31.12." + year,
      budsjett: "Budsjett og avvik " + year,
      prosjekt: "Prosjektlønnsomhet " + year,
      kontospesifikasjon: "Kontospesifikasjon " + year,
      reskontrospesifikasjon: "Kunde- og leverandørspesifikasjon " + year,
      bokforingsspesifikasjon: "Bokføringsspesifikasjon " + year,
      balansedokumentasjon: "Balansedokumentasjon (bokføringsloven §11)",
      revisjon: "Verifikasjonsrapport",
      saft: "SAF-T " + year,
    }[rapport],
  );
</script>

<Card title={tittel}>
  <div class="flex gap-2 flex-wrap items-end mb-4">
    <!-- Wide screens: the tab row. Phones: one native select, so the
         report starts on the first screen instead of under five rows of
         tabs. -->
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div role="tablist" aria-label="Rapporter" class="tabs tabs-box tabs-sm hidden sm:flex" onkeydown={tabKey}>
      {#each TABS as [key, label] (key)}
        <button
          role="tab"
          id={"rapport-tab-" + key}
          class="tab {key === rapport ? 'tab-active' : ''}"
          aria-selected={key === rapport}
          aria-controls="rapport-panel"
          tabindex={key === rapport ? 0 : -1}
          onclick={() => velg(key)}
        >
          {label}
        </button>
      {/each}
    </div>
    <label class="sm:hidden flex flex-col gap-1 text-sm">
      Rapport
      <select class="select select-sm" value={rapport} onchange={(e) => velg(e.currentTarget.value)}>
        {#each TABS as [key, label] (key)}
          <option value={key}>{label}</option>
        {/each}
      </select>
    </label>
    <YearPicker bind:year={() => year, setYear} />
  </div>

  <div role="tabpanel" id="rapport-panel" aria-labelledby={"rapport-tab-" + rapport}>
    {#if rapport === "saldobalanse"}
      <Saldobalanse {companyId} {from} {to} {year} />
    {:else if rapport === "resultat"}
      <Resultat {companyId} {from} {to} {params} />
    {:else if rapport === "balanse"}
      <Balanse {companyId} {to} />
    {:else if rapport === "budsjett"}
      <Budsjett {companyId} {year} />
    {:else if rapport === "prosjekt"}
      <Prosjekt {companyId} {year} />
    {:else if rapport === "kontospesifikasjon"}
      <Kontospesifikasjon {companyId} {from} {to} {year} {params} />
    {:else if rapport === "reskontrospesifikasjon"}
      <Reskontrospesifikasjon {companyId} {from} {to} {params} />
    {:else if rapport === "bokforingsspesifikasjon"}
      <Bokforingsspesifikasjon {companyId} {from} {to} {params} />
    {:else if rapport === "balansedokumentasjon"}
      <Balansedokumentasjon {companyId} {to} {params} />
    {:else if rapport === "revisjon"}
      <Revisjon {companyId} />
    {:else if rapport === "saft"}
      <Saft {companyId} {year} />
    {/if}
  </div>
</Card>

{#if rapport === "revisjon"}
  <Forankring {companyId} />
{/if}
