<script>
  // Kundevilkårene (#185, docs/kundevilkar.md §1.4): innsending til
  // myndighetene på selskapets vegne krever at en administrator har godtatt
  // akkurat denne teksten. Versjonen er tekstens hash — serveren nekter en
  // versjon som ikke er den gjeldende.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId } = $props();

  let status = $state(null);
  let vilkar = $state(null); // { versjon, tekst } når åpnet

  async function hent() {
    try {
      status = await api("/companies/" + companyId + "/terms");
    } catch (error) {
      toast(error.message, false);
    }
  }

  $effect(() => {
    companyId;
    hent();
  });

  async function les() {
    try {
      vilkar = await api("/terms");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function godta() {
    if (
      !(await bekreft(
        "Godta kundevilkårene på vegne av selskapet? Du bekrefter at du har rett til å binde selskapet.",
        { tittel: "Godta kundevilkår", ok: "Godta" },
      ))
    ) {
      return;
    }
    try {
      status = await post("/companies/" + companyId + "/terms/accept", {
        versjon: vilkar.versjon,
      });
      toast("Kundevilkårene er godtatt", true);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Kundevilkår">
  {#if !status}
    <span class="loading loading-spinner"></span>
  {:else}
    {#if status.godtatt}
      <p class="text-sm mb-2">
        Godtatt av <b>{status.godtatt_av}</b>
        {new Date(status.godtatt_tidspunkt).toLocaleString("nb-NO")}.
      </p>
    {:else}
      <p class="text-sm mb-2">
        Før regnmed kan sende a-melding, aksjonærregisteroppgave og andre oppgaver til
        myndighetene på selskapets vegne, må en administrator godta kundevilkårene.
      </p>
    {/if}
    {#if vilkar}
      <pre
        class="whitespace-pre-wrap text-xs border border-base-300 rounded p-3 max-h-96 overflow-y-auto mb-2">{vilkar.tekst}</pre>
      <p class="text-xs opacity-60 mb-2">Versjon {vilkar.versjon.slice(0, 12)}</p>
      {#if !status.godtatt}
        <button class="btn btn-sm btn-primary" onclick={godta}>
          Godta på vegne av selskapet
        </button>
      {/if}
    {:else}
      <button class="btn btn-sm btn-outline" onclick={les}>Les kundevilkårene</button>
    {/if}
  {/if}
</Card>
