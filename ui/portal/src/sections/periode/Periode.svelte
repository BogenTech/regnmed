<script>
  // Periodelåsing (#4/M1): låsen er innsettings-bar historikk, aldri en
  // redigering — hver lås er en ny rad. Gjenåpning er en admin-handling
  // og logges; portalen viser bare sporet, serveren håndhever.
  import { api, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Arsavslutning from "./Arsavslutning.svelte";

  let { companyId } = $props();

  let lock = $state(null);
  let lockedThrough = $state("");

  async function load(id) {
    lock = await api("/companies/" + id + "/period-lock");
  }

  $effect(() => {
    lock = null;
    load(companyId).catch((error) => toast(error.message, false));
  });

  async function laas(event) {
    event.preventDefault();
    try {
      await send("PUT", "/companies/" + companyId + "/period-lock", {
        locked_through: lockedThrough,
      });
      toast("Periode låst", true);
      lockedThrough = "";
      await load(companyId);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if !lock}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Card title="Periodelåsing">
    <p class="mb-2">Låst til og med: <strong>{lock.locked_through || "ingen lås"}</strong></p>
    <form class="flex gap-2 items-end" onsubmit={laas}>
      <input type="date" class="input" required bind:value={lockedThrough} />
      <button class="btn btn-primary">Lås periode</button>
    </form>
    <p class="text-sm opacity-70 mt-2">
      Bilag datert i låst periode avvises; rettelser føres i åpen periode. Gjenåpning krever admin
      og logges.
    </p>
  </Card>

  <Card title="Historikk">
    <table class="table table-sm">
      <thead><tr><th>Låst t.o.m.</th><th>Av</th><th>Når</th></tr></thead>
      <tbody>
        {#each lock.history as h}
          <tr>
            <td>{h.locked_through}</td>
            <td>{h.set_by}</td>
            <td>{h.at.slice(0, 16).replace("T", " ")}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </Card>
{/if}

<!--
  Årsavslutningen SETTER periodelåsen (docs/arsavslutning.md), så kortet
  over må lastes på nytt — ellers står det «ingen lås» rett over kortet
  som nettopp låste året, og operatøren får to motstridende svar på
  samme spørsmål.
-->
<Arsavslutning
  {companyId}
  onavsluttet={() => load(companyId).catch((error) => toast(error.message, false))}
/>
