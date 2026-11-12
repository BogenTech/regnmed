<script>
  // Utlegg og kjøregodtgjørelse (#42): kravene først — det er dem folk
  // kommer for («når får jeg pengene?», «hva venter på meg?»). Hvert krav,
  // og skjemaet for et nytt, har sin egen adresse (#/c/{id}/utlegg/{…}).
  // Statusen går bare én vei: innsendt → godkjent/avvist → utbetalt.
  import { harRett } from "../../lib/me.svelte.js";
  import ExpenseList from "./ExpenseList.svelte";
  import NewExpense from "./NewExpense.svelte";
  import ExpenseDetail from "./ExpenseDetail.svelte";

  let { companyId, extra } = $props();

  const TABS = [
    ["", "Åpne"],
    ["utbetalt", "Utbetalt"],
    ["avvist", "Avvist"],
  ];
  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);
  let canSubmit = $derived(harRett(companyId, "UTLEGG_SKRIV_EGNE"));
  let seesAll = $derived(harRett(companyId, "UTLEGG_LES_ALLE"));
</script>

{#if isId(extra)}
  {#key companyId + extra}
    <ExpenseDetail {companyId} expenseId={extra} />
  {/key}
{:else if extra === "ny"}
  {#key companyId}<NewExpense {companyId} />{/key}
{:else if tab === null}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  <div class="flex flex-wrap items-center gap-3 mb-4">
    <h1 class="text-2xl font-bold mr-auto">{seesAll ? "Utlegg" : "Mine utlegg"}</h1>
    {#if canSubmit}
      <a class="btn btn-primary" href={"#/c/" + companyId + "/utlegg/ny"}>Nytt utlegg eller kjøring</a>
    {/if}
  </div>
  {#key companyId + tab}
    <ExpenseList {companyId} {tab} tabs={TABS} {seesAll} />
  {/key}
{/if}
