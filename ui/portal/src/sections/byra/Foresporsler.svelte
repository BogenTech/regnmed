<script>
  // Å godta en forespørsel oppretter oppdraget i én transaksjon —
  // tilgangen er levende med det samme, uten ny innlogging, så /me må
  // hentes på nytt.
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { firmId, pending, onDone } = $props();

  async function avgjor(r, accept) {
    try {
      await post("/firms/" + firmId + "/requests/" + r.request_id + "/decision", { accept });
      toast(accept ? "Oppdrag godtatt" : "Avslått", true);
      await loadMe(true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Innkommende forespørsler">
  {#if pending.length}
    <table class="table table-sm">
      <thead>
        <tr><th>Selskap</th><th>Type</th><th>Melding</th><th></th></tr>
      </thead>
      <tbody>
        {#each pending as r (r.request_id)}
          <tr>
            <td>{r.company} ({r.orgnr})</td>
            <td>{r.kind}</td>
            <td>{r.message || ""}</td>
            <td class="flex gap-1">
              <button class="btn btn-xs btn-primary" onclick={() => avgjor(r, true)}>Godta</button>
              <button class="btn btn-xs" onclick={() => avgjor(r, false)}>Avslå</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="opacity-70">Ingen ventende forespørsler.</p>
  {/if}
</Card>
