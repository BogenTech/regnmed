<script>
  // Bilag: innboksen er forsiden; hvert dokument, de behandlede, de
  // bokførte bilagene og innstillingene har hver sin adresse
  // (#/c/{id}/bilag/{…}). En ansatt får bare sin egen kvitteringsside.
  import { company } from "../../lib/me.svelte.js";
  import AnsattKvittering from "./AnsattKvittering.svelte";
  import InboxList from "./InboxList.svelte";
  import BilagDokument from "./BilagDokument.svelte";
  import DecidedList from "./DecidedList.svelte";
  import BilagListe from "./BilagListe.svelte";
  import BilagSettings from "./BilagSettings.svelte";

  let { companyId, extra } = $props();

  // En ansatt kan LASTE OPP en kvittering, men ikke lese innboksen.
  let onlyOwn = $derived(company(companyId)?.access === "ansatt");

  const TABS = [
    ["", "Innboks"],
    ["behandlet", "Behandlede"],
    ["bokforte", "Bokførte bilag"],
    ["innstillinger", "Innstillinger"],
  ];
  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);
</script>

{#if onlyOwn}
  <AnsattKvittering {companyId} />
{:else if isId(extra)}
  {#key companyId + extra}
    <BilagDokument {companyId} documentId={extra} />
  {/key}
{:else if tab === null}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  <nav class="tabs tabs-box mb-4 w-fit max-w-full" aria-label="Bilag">
    {#each TABS as [key, label] (key)}
      <a
        class="tab whitespace-nowrap {tab === key ? 'tab-active' : ''}"
        href={"#/c/" + companyId + "/bilag" + (key ? "/" + key : "")}
        aria-current={tab === key ? "page" : undefined}
      >
        {label}
      </a>
    {/each}
  </nav>
  {#key companyId + tab}
    {#if tab === ""}
      <InboxList {companyId} />
    {:else if tab === "behandlet"}
      <DecidedList {companyId} />
    {:else if tab === "bokforte"}
      <BilagListe {companyId} />
    {:else}
      <BilagSettings {companyId} />
    {/if}
  {/key}
{/if}
