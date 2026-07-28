<script>
  // Oppdragene selskapet har gitt. Å avslutte setter valid_to, og siden
  // den er EKSKLUSIV i tilgangsoppslaget virker tilbakekallingen straks
  // — derfor må /me hentes på nytt etterpå.
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, engagements, onDone } = $props();

  async function avslutt(e) {
    if (!confirm("Avslutte oppdraget?")) return;
    try {
      await post("/companies/" + companyId + "/engagements/" + e.engagement_id + "/end", {});
      toast("Oppdrag avsluttet", true);
      await loadMe(true); // tilgangen er endret nå, ikke ved neste innlogging
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Oppdrag">
  {#if engagements.length}
    <table class="table table-sm">
      <thead>
        <tr><th>Byrå</th><th>Type</th><th>Fra</th><th></th></tr>
      </thead>
      <tbody>
        {#each engagements as e (e.engagement_id)}
          <tr>
            <td>{e.firm}</td>
            <td>{e.kind}</td>
            <td>{e.valid_from}</td>
            <td>
              {#if !e.valid_to}
                <button class="btn btn-xs btn-outline" onclick={() => avslutt(e)}>Avslutt</button>
              {:else}
                <span class="opacity-60 text-sm">avsluttet {e.valid_to}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="opacity-70">Ingen oppdrag ennå — finn et autorisert byrå under.</p>
  {/if}
</Card>
