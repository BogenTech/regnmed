<script>
  // Fakturagrunnlaget: ufakturerte fakturerbare timer gruppert per
  // (prosjekt, sats) — én faktura per kunde, timene merkes fakturert i
  // samme transaksjon. Prosjekter knyttet til en kunde (#80) får et
  // FORSLAG: én knapp som fakturerer akkurat det prosjektet til akkurat
  // den kunden. Forslag, ikke automatikk — knappen er et menneskevalg,
  // og den generelle veien under står som før.
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

  // One suggestion per DISTINCT customer-linked prosjekt (groups are
  // per sats, so the same prosjekt can appear several times).
  let forslag = $derived.by(() => {
    const sett = new Map();
    for (const g of unbilled) {
      if (g.prosjekt && g.kunde && !sett.has(g.prosjekt)) {
        sett.set(g.prosjekt, g);
      }
    }
    return [...sett.values()];
  });

  async function fakturer(body, hva) {
    try {
      const issued = await post("/companies/" + companyId + "/timesheet/invoice", body);
      toast(
        hva + ": faktura " + issued.invoice_no + " opprettet (KID " + issued.kid + ")",
        true,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title={"Ufakturerte timer — " + kr(sum) + " kr"}>
  <table class="table table-sm mb-2">
    <thead>
      <tr>
        <th>Prosjekt</th>
        <th>Kunde</th>
        <th class="text-right">Timer</th>
        <th class="text-right">Sats</th>
      </tr>
    </thead>
    <tbody>
      {#each unbilled as g}
        <tr>
          <td>{g.prosjekt ? g.prosjekt : "(uten prosjekt)"}</td>
          <td class="opacity-70">{g.kunde_navn || ""}</td>
          <td class="text-right">{minutterTilTimer(g.minutter)} t</td>
          <td class="text-right">{kr(g.timesats_ore)}/t</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if forslag.length}
    <div class="flex gap-2 flex-wrap mb-2">
      {#each forslag as f (f.prosjekt)}
        <button
          class="btn btn-sm btn-outline"
          onclick={() =>
            fakturer({ party_no: f.kunde, prosjekt: f.prosjekt }, "Prosjekt " + f.prosjekt)}
        >
          Fakturer {f.prosjekt} → {f.kunde_navn}
        </button>
      {/each}
    </div>
  {/if}
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
      <button class="btn btn-sm btn-primary" onclick={() => fakturer({ party_no: partyNo }, "Alt")}>
        Lag faktura for alt
      </button>
    </div>
  {/if}
</Card>
