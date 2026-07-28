<script>
  // Fakturagrunnlaget: ufakturerte fakturerbare timer gruppert per
  // (prosjekt, sats) — én faktura per kunde, timene merkes fakturert i
  // samme transaksjon.
  import { post } from "../../lib/api.js";
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, unbilled, kunder, onDone } = $props();

  // Kunden står på den første i listen til noen velger en annen.
  let valgtKunde = $state("");
  let partyNo = $derived(valgtKunde || kunder[0]?.party_no || "");

  let sum = $derived(
    unbilled.reduce((acc, g) => acc + Math.round((g.minutter * g.timesats_ore) / 60), 0),
  );

  async function lagFaktura() {
    try {
      const issued = await post("/companies/" + companyId + "/timesheet/invoice", {
        party_no: partyNo,
      });
      toast("Faktura " + issued.invoice_no + " opprettet (KID " + issued.kid + ")", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title={"Ufakturerte timer — " + kr(sum) + " kr"}>
  <table class="table table-sm mb-2">
    <thead>
      <tr><th>Prosjekt</th><th class="text-right">Timer</th><th class="text-right">Sats</th></tr>
    </thead>
    <tbody>
      {#each unbilled as g}
        <tr>
          <td>{g.prosjekt ? g.prosjekt : "(uten prosjekt)"}</td>
          <td class="text-right">{minutterTilTimer(g.minutter)} t</td>
          <td class="text-right">{kr(g.timesats_ore)}/t</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if kunder.length}
    <div class="flex gap-2">
      <select
        class="select select-sm select-bordered"
        value={partyNo}
        onchange={(e) => (valgtKunde = e.currentTarget.value)}
      >
        {#each kunder as p (p.party_no)}
          <option value={p.party_no}>{p.party_no} {p.name}</option>
        {/each}
      </select>
      <button class="btn btn-sm btn-primary" onclick={lagFaktura}>Lag faktura</button>
    </div>
  {/if}
</Card>
