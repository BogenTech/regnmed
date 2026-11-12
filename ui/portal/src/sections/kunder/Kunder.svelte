<script>
  // Kunder: registeret er forsiden, hver kunde og «ny kunde» har sin egen
  // adresse (#/c/{id}/kunder/{…}), så tilbakeknappen og lenker virker.
  import CustomerList from "./CustomerList.svelte";
  import CustomerDetail from "./CustomerDetail.svelte";
  import NewCustomer from "./NewCustomer.svelte";

  let { companyId, extra, query } = $props();

  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
</script>

{#if extra === "ny"}
  <NewCustomer {companyId} />
{:else if isId(extra)}
  {#key companyId + extra}
    <CustomerDetail {companyId} partyId={extra} />
  {/key}
{:else if extra}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  {#key companyId + "?" + query}
    <CustomerList {companyId} {query} />
  {/key}
{/if}
