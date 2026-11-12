<script>
  import { api } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { companyId } = $props();

  let latest = $state(null);
  $effect(() => {
    api("/companies/" + companyId + "/anchors")
      .then((r) => (latest = r.anchors[0]))
      .catch(() => (latest = null));
  });
  let verifying = $state(false);
  let result = $state(null); // {ok, vouchers_checked, ...} | {error}

  async function verify() {
    verifying = true;
    result = null;
    try {
      result = await api("/companies/" + companyId + "/anchors/verify");
    } catch (error) {
      result = { error: error.message };
    }
    verifying = false;
  }
</script>

<Card title="Forankring">
  {#if latest}
    <p class="text-sm opacity-70 mb-2">
      Sist forankret {formatDate(latest.created_at)} {latest.created_at.slice(11, 16)}
      (bilag t.o.m. sekvens {latest.last_seq}{latest.witnesses.length
        ? ", bevitnet eksternt"
        : ""}).
    </p>
    <p class="text-xs font-mono opacity-50 mb-2 break-all">rot {latest.root_hash}</p>
  {:else}
    <p class="text-sm opacity-70 mb-2">Ikke forankret ennå — kjøres periodisk av systemet.</p>
  {/if}
  <p class="text-sm opacity-70 mb-2">
    Hovedbokens hash-kjede forankres under en offentlig rot utenfor databasen — omskrevet
    historikk kan derfor bevises, ikke bare mistenkes.
  </p>
  <button class="btn btn-sm btn-outline" onclick={verify}>
    Verifiser kjeden mot forankringen
  </button>
  <div class="mt-2">
    {#if verifying}
      <span class="loading loading-spinner loading-sm"></span>
    {:else if result?.error}
      <div class="alert alert-error text-sm py-2">{result.error}</div>
    {:else if result?.ok}
      <div class="alert alert-success text-sm py-2">
        Kjeden verifisert fra genesis: {result.vouchers_checked} bilag,
        {result.attachments_checked} vedlegg, {result.anchors_checked} forankringer stemmer.
      </div>
    {:else if result}
      <div class="alert alert-error text-sm py-2">
        {#each result.problems as p, i}{#if i}<br />{/if}{p}{/each}
      </div>
    {/if}
  </div>
</Card>
