<script>
  // Rapporter (#4, #24, #41): ett kort, én fane av gangen. Bare den
  // valgte rapporten hentes — hver fanekomponent laster sitt eget kall.
  import { untrack } from "svelte";
  import Card from "../../components/Card.svelte";
  import Saldobalanse from "./Saldobalanse.svelte";
  import Resultat from "./Resultat.svelte";
  import Balanse from "./Balanse.svelte";
  import Budsjett from "./Budsjett.svelte";
  import Kontospesifikasjon from "./Kontospesifikasjon.svelte";
  import Reskontrospesifikasjon from "./Reskontrospesifikasjon.svelte";
  import Prosjekt from "./Prosjekt.svelte";
  import Bokforingsspesifikasjon from "./Bokforingsspesifikasjon.svelte";
  import Revisjon from "./Revisjon.svelte";
  import Balansedokumentasjon from "./Balansedokumentasjon.svelte";

  let { companyId, extra } = $props();

  const FANER = [
    ["saldobalanse", "Saldobalanse"],
    ["resultat", "Resultat"],
    ["balanse", "Balanse"],
    ["budsjett", "Budsjett"],
    ["prosjekt", "Prosjekt"],
    ["kontospesifikasjon", "Kontospesifikasjon"],
    ["reskontrospesifikasjon", "Reskontro"],
    ["bokforingsspesifikasjon", "Bokføringsspesifikasjon"],
    ["balansedokumentasjon", "Balansedokumentasjon"],
    ["revisjon", "Revisjon"],
  ];

  // Fanevalg og år er lokal tilstand. `extra` (#/c/{id}/rapporter/{fane})
  // kan peke rett på en fane, men bare på et navn som finnes.
  let rapport = $state(untrack(() => FANER.some((f) => f[0] === extra) ? extra : "saldobalanse"));
  let year = $state(new Date().getFullYear());

  let from = $derived(year + "-01-01");
  let to = $derived(year + "-12-31");

  let tittel = $derived(
    {
      saldobalanse: "Saldobalanse " + year,
      resultat: "Resultatregnskap " + year,
      balanse: "Balanse per " + to,
      budsjett: "Budsjett og avvik " + year,
      prosjekt: "Prosjektlønnsomhet " + year,
      kontospesifikasjon: "Kontospesifikasjon " + year,
      reskontrospesifikasjon: "Kunde- og leverandørspesifikasjon " + year,
      bokforingsspesifikasjon: "Bokføringsspesifikasjon " + year,
      balansedokumentasjon: "Balansedokumentasjon (bokføringsloven §11)",
      revisjon: "Verifikasjonsrapport",
    }[rapport],
  );
</script>

<Card title={tittel}>
  <div class="flex gap-2 flex-wrap mb-4">
    <!-- Faner, ikke en knapperad: role=tablist/tab sier til skjermlesere
         at dette bytter panel under, ikke utfører en handling. -->
    <div role="tablist" class="tabs tabs-box tabs-sm">
      {#each FANER as [key, label] (key)}
        <button
          role="tab"
          class="tab {key === rapport ? 'tab-active' : ''}"
          aria-selected={key === rapport}
          onclick={() => (rapport = key)}
        >
          {label}
        </button>
      {/each}
    </div>
    <div class="join">
      <button class="join-item btn btn-sm" onclick={() => year--}>«</button>
      <span class="join-item btn btn-sm btn-disabled">{year}</span>
      <button class="join-item btn btn-sm" onclick={() => year++}>»</button>
    </div>
  </div>

  {#if rapport === "saldobalanse"}
    <Saldobalanse {companyId} {from} {to} />
  {:else if rapport === "resultat"}
    <Resultat {companyId} {from} {to} />
  {:else if rapport === "balanse"}
    <Balanse {companyId} {to} />
  {:else if rapport === "budsjett"}
    <Budsjett {companyId} {year} />
  {:else if rapport === "prosjekt"}
    <Prosjekt {companyId} {year} />
  {:else if rapport === "kontospesifikasjon"}
    <Kontospesifikasjon {companyId} {from} {to} />
  {:else if rapport === "reskontrospesifikasjon"}
    <Reskontrospesifikasjon {companyId} {from} {to} />
  {:else if rapport === "bokforingsspesifikasjon"}
    <Bokforingsspesifikasjon {companyId} {from} {to} />
  {:else if rapport === "balansedokumentasjon"}
    <Balansedokumentasjon {companyId} {to} />
  {:else if rapport === "revisjon"}
    <Revisjon {companyId} />
  {/if}
</Card>
