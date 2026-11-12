<script>
  // Kunder: registeret er forsiden, hver kunde og «ny kunde» har sin egen
  // adresse (#/c/{id}/kunder/{…}), så tilbakeknappen og lenker virker.
  import PartyList from "../parties/PartyList.svelte";
  import NewParty from "../parties/NewParty.svelte";
  import CustomerDetail from "./CustomerDetail.svelte";

  let { companyId, extra, query } = $props();

  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
</script>

{#if extra === "ny"}
  <NewParty {companyId} kind="kunde" />
{:else if isId(extra)}
  {#key companyId + extra}
    <CustomerDetail {companyId} partyId={extra} />
  {/key}
{:else if extra}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  {#key companyId + "?" + query}
    <PartyList {companyId} kind="kunde" {query} />
  {/key}
{/if}
