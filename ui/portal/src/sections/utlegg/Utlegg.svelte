<script>
  // Expenses and mileage (#42): the claims first — that is what people
  // come for ("when do I get paid?", "what is waiting for me?"). Each
  // claim, and the form for a new one, has its own address
  // (#/c/{id}/utlegg/{…}). Status only moves one way:
  // innsendt → godkjent/avvist → utbetalt.
  import { harRett } from "../../lib/me.svelte.js";
  import ExpenseList from "./ExpenseList.svelte";
  import NewExpense from "./NewExpense.svelte";
  import DraftForm from "./DraftForm.svelte";
  import TravelForm from "./TravelForm.svelte";
  import CardLines from "./CardLines.svelte";
  import { api } from "../../lib/api.js";
  import ExpenseDetail from "./ExpenseDetail.svelte";

  let { companyId, extra } = $props();

  const TABS = [
    ["utkast", "Utkast"],
    ["", "Åpne"],
    ["utbetalt", "Utbetalt"],
    ["avvist", "Avvist"],
  ];
  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
  let tab = $derived(TABS.some(([k]) => k === (extra || "")) ? extra || "" : null);
  let canSubmit = $derived(harRett(companyId, "UTLEGG_SKRIV_EGNE"));
  let seesAll = $derived(harRett(companyId, "UTLEGG_LES_ALLE"));

  // Company-card purchases still waiting for a receipt: the first thing a
  // cardholder should see, because the books cannot close without them.
  let missing = $state(0);
  // Fetched when the list comes into view, not on every tab switch.
  let inList = $derived(tab !== null);
  $effect(() => {
    if (!canSubmit || !inList) return;
    api("/companies/" + companyId + "/expenses/card-lines")
      .then((r) => (missing = r.lines.length))
      .catch(() => {});
  });
</script>

{#if isId(extra)}
  {#key companyId + extra}
    <ExpenseDetail {companyId} expenseId={extra} />
  {/key}
{:else if extra === "ny"}
  {#key companyId}<NewExpense {companyId} />{/key}
{:else if extra === "kjoring"}
  <nav class="mb-3"><a class="link link-hover text-sm" href={"#/c/" + companyId + "/utlegg"}>← Alle utlegg</a></nav>
  <h1 class="text-2xl font-bold mb-4">Ny kjøring</h1>
  {#key companyId}<DraftForm {companyId} kind="kjoring" />{/key}
{:else if extra === "kort"}
  {#key companyId}<CardLines {companyId} />{/key}
{:else if extra === "reise"}
  <nav class="mb-3"><a class="link link-hover text-sm" href={"#/c/" + companyId + "/utlegg"}>← Alle utlegg</a></nav>
  <h1 class="text-2xl font-bold mb-4">Ny reise</h1>
  {#key companyId}<TravelForm {companyId} />{/key}
{:else if tab === null}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  <div class="flex flex-wrap items-center gap-3 mb-4">
    <h1 class="text-2xl font-bold mr-auto">{seesAll ? "Utlegg" : "Mine utlegg"}</h1>
    {#if canSubmit}
      <a class="btn btn-primary" href={"#/c/" + companyId + "/utlegg/ny"}>Nytt utlegg</a>
      <a class="btn" href={"#/c/" + companyId + "/utlegg/kjoring"}>Ny kjøring</a>
      <a class="btn" href={"#/c/" + companyId + "/utlegg/reise"}>Ny reise</a>
    {/if}
  </div>
  {#if missing}
    <div class="alert alert-warning alert-soft mb-4" role="status">
      <span>{missing === 1 ? "Ett kortkjøp mangler" : missing + " kortkjøp mangler"} kvittering.</span>
      <a class="btn btn-sm" href={"#/c/" + companyId + "/utlegg/kort"}>Legg ved kvitteringer</a>
    </div>
  {/if}
  {#key companyId + tab}
    <ExpenseList {companyId} {tab} tabs={TABS} {seesAll} />
  {/key}
{/if}
