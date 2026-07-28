<script>
  // Katalogen viser bare byråer med verifisert autorisasjon. Å be om
  // oppdrag gir ingen tilgang i seg selv — byrået må godta først.
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, firms, engagements, onDone } = $props();

  let active = $derived(engagements.filter((e) => !e.valid_to));

  async function beOm(f) {
    try {
      await post("/companies/" + companyId + "/engagement-requests", { firm_id: f.firm_id });
      toast("Forespørsel sendt", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Autoriserte byråer (Finanstilsynet-verifisert)">
  <table class="table table-sm">
    <thead>
      <tr><th>Navn</th><th>Orgnr</th><th>Type</th><th>Klienter</th><th></th></tr>
    </thead>
    <tbody>
      {#each firms as f (f.firm_id)}
        <tr>
          <td>{f.name}</td>
          <td>{f.orgnr}</td>
          <td>{f.kind}</td>
          <td>{f.client_count}</td>
          <td>
            {#if active.some((e) => e.firm_id === f.firm_id)}
              <span class="badge badge-ghost">aktivt oppdrag</span>
            {:else}
              <button class="btn btn-xs btn-primary" onclick={() => beOm(f)}>Be om oppdrag</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</Card>
