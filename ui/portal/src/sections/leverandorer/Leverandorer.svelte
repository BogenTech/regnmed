<script>
  // Leverandører: registeret er forsiden, hver leverandør og «ny
  // leverandør» har sin egen adresse — samme sider som Kunder, med
  // leverandørens ord og fortegn.
  import PartyList from "../parties/PartyList.svelte";
  import NewParty from "../parties/NewParty.svelte";
  import SupplierDetail from "./SupplierDetail.svelte";

  let { companyId, extra, query } = $props();

  const isId = (s) => /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(s || "");
</script>

{#if extra === "ny"}
  <NewParty {companyId} kind="leverandor" />
{:else if isId(extra)}
  {#key companyId + extra}
    <SupplierDetail {companyId} partyId={extra} />
  {/key}
{:else if extra}
  <div class="alert alert-warning"><span>Ukjent side «{extra}».</span></div>
{:else}
  {#key companyId + "?" + query}
    <PartyList {companyId} kind="leverandor" {query} />
  {/key}
{/if}
