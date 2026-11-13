<script>
  // Reskontro: the accountant's workbench across customers and suppliers.
  // Each party has ONE page, under Kunder or Leverandører; here is what
  // spans them — whether the reskontro agrees with the hovedbok, who has
  // items to match, the register for both kinds, importing from another
  // system, and which accounts are reskontro accounts. Old links to
  // #/c/{id}/reskontro/{party} land on the party's own page.
  import { api } from "../../lib/api.js";
  import PartyList from "../parties/PartyList.svelte";
  import Overview from "./Overview.svelte";
  import Import from "./Import.svelte";
  import Settings from "./Settings.svelte";

  let { companyId, extra, query = "" } = $props();

  const TABS = [
    ["", "Oversikt"],
    ["parter", "Parter"],
    ["import", "Import"],
    ["innstillinger", "Reskontrokontoer"],
  ];
  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);
  let error = $state(null);

  $effect(() => {
    if (!isId(extra)) return;
    api("/companies/" + companyId + "/parties/" + extra)
      .then((p) =>
        location.replace(
          "#/c/" + companyId + "/" + (p.kind === "leverandor" ? "leverandorer" : "kunder") + "/" + p.party_id,
        ),
      )
      .catch((e) => (error = e.message));
  });

  let kind = $state(new URLSearchParams(query).get("kind") === "leverandor" ? "leverandor" : "kunde");
</script>

{#if isId(extra)}
  {#if error}
    <div class="alert alert-error" role="alert"><span>{error}</span></div>
  {:else}
    <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
  {/if}
{:else}
  <h1 class="text-2xl font-bold mb-4">Reskontro</h1>
  {#if tab === null}
    <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
  {:else}
    <nav class="tabs tabs-box mb-4 w-fit max-w-full overflow-x-auto" aria-label="Reskontro">
      {#each TABS as [key, label] (key)}
        <a
          class="tab whitespace-nowrap {tab === key ? 'tab-active' : ''}"
          href={"#/c/" + companyId + "/reskontro" + (key ? "/" + key : "")}
          aria-current={tab === key ? "page" : undefined}
        >
          {label}
        </a>
      {/each}
    </nav>
    {#key companyId + tab}
      {#if tab === ""}
        <Overview {companyId} />
      {:else if tab === "parter"}
        <div class="join mb-4" role="group" aria-label="Vis">
          <button class="btn btn-sm join-item {kind === 'kunde' ? 'btn-active' : ''}" aria-pressed={kind === "kunde"} onclick={() => (kind = "kunde")}>Kunder</button>
          <button class="btn btn-sm join-item {kind === 'leverandor' ? 'btn-active' : ''}" aria-pressed={kind === "leverandor"} onclick={() => (kind = "leverandor")}>Leverandører</button>
        </div>
        {#key kind}
          <PartyList {companyId} {kind} query={kind === new URLSearchParams(query).get("kind") ? query : ""} listSection="reskontro/parter" heading={false} />
        {/key}
      {:else if tab === "import"}
        <Import {companyId} />
      {:else}
        <Settings {companyId} />
      {/if}
    {/key}
  {/if}
{/if}
