<script>
  // Terminordningen er datert og innsettings-bar historikk (#51): en ny
  // rad registrerer vedtaket Skatteetaten har fattet, med virkning fra
  // en dato. Ingenting utledes — spesifikasjon, melding og frister
  // følger den registrerte ordningen.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, ordningInfo, year, onDone } = $props();

  const ORDNING_NAVN = {
    "to-maneder": "to-månedersterminer",
    arlig: "årstermin",
    primaernaering: "årstermin (primærnæring)",
  };

  let ordning = $state("to-maneder");
  // Startverdi, ikke binding: feltet skal kunne redigeres fritt. Velges
  // et annet år lastes seksjonen på nytt og kortet bygges opp igjen.
  let fra = $state(untrack(() => year) + "-01-01");
  let note = $state("");

  async function registrer() {
    try {
      await post("/companies/" + companyId + "/mva/terminordning", {
        ordning,
        valid_from: fra,
        note: note.trim() || null,
      });
      toast("Terminordning registrert", true);
      ordning = "to-maneder";
      fra = year + "-01-01";
      note = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Terminordning">
  <p class="text-sm opacity-70 mb-2">
    Gjeldende ordning: <b>{ORDNING_NAVN[ordningInfo.ordning] || ordningInfo.ordning}</b>. Ordningen
    Skatteetaten har innvilget registreres med virkning fra en dato — spesifikasjon, melding og
    frister følger den. Systemet vurderer aldri berettigelse.
  </p>
  <div class="flex flex-wrap gap-2 items-end mb-2">
    <select class="select select-sm select-bordered" bind:value={ordning}>
      <option value="to-maneder">to-månedersterminer</option>
      <option value="arlig">årstermin</option>
      <option value="primaernaering">årstermin (primærnæring)</option>
    </select>
    <input type="date" class="input input-sm input-bordered" bind:value={fra} />
    <input class="input input-sm input-bordered" placeholder="Vedtaksreferanse" bind:value={note} />
    <button class="btn btn-sm" onclick={registrer}>Registrer</button>
  </div>
  {#if ordningInfo.history.length}
    <table class="table table-xs max-w-md">
      <thead><tr><th>Fra</th><th>Ordning</th><th>Notat</th></tr></thead>
      <tbody>
        {#each ordningInfo.history as h}
          <tr>
            <td>{h.valid_from}</td>
            <td>{ORDNING_NAVN[h.ordning] || h.ordning}</td>
            <td class="text-xs opacity-70">{h.note || ""}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
