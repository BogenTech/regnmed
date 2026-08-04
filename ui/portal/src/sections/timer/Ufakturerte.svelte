<script>
  // Fakturagrunnlaget: ufakturerte fakturerbare timer gruppert per
  // (prosjekt, sats), med hver persons timer som egen linje. Den som
  // holder TIMER_FAKTURER velger hva som blir med: alt, ett prosjekt
  // (forslagsknappene, #80), eller et UTVALG av personlinjer — utvalget
  // faktureres og låses i samme transaksjon, så valgte timer aldri kan
  // endres i mellomtiden (docs/timer.md).
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

  // Utvalget er personlinjer (en persons timer i gruppen) — aldri en
  // halv føring. Nøkkel: gruppeindeks + person.
  let valgte = $state([]);
  function nokkel(gIdx, p) {
    return gIdx + ":" + p.person_id;
  }
  function veksle(gIdx, p) {
    const n = nokkel(gIdx, p);
    valgte = valgte.includes(n) ? valgte.filter((v) => v !== n) : [...valgte, n];
  }
  let utvalg = $derived.by(() => {
    let minutter = 0;
    let belop = 0;
    let entryIds = [];
    unbilled.forEach((g, gIdx) => {
      for (const p of g.personer || []) {
        if (valgte.includes(nokkel(gIdx, p))) {
          minutter += p.minutter;
          belop += Math.round((p.minutter * g.timesats_ore) / 60);
          entryIds = entryIds.concat(p.entry_ids);
        }
      }
    });
    return { minutter, belop, entryIds };
  });

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
      valgte = [];
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
        <th></th>
        <th>Prosjekt</th>
        <th>Kunde</th>
        <th class="text-right">Timer</th>
        <th class="text-right">Sats</th>
      </tr>
    </thead>
    <tbody>
      {#each unbilled as g, gIdx}
        <tr>
          <td></td>
          <td class="font-medium">{g.prosjekt ? g.prosjekt : "(uten prosjekt)"}</td>
          <td class="opacity-70">{g.kunde_navn || ""}</td>
          <td class="text-right">{minutterTilTimer(g.minutter)} t</td>
          <td class="text-right">{kr(g.timesats_ore)}/t</td>
        </tr>
        {#each g.personer || [] as p (nokkel(gIdx, p))}
          <tr class="text-sm">
            <td class="w-8">
              <input
                type="checkbox"
                class="checkbox checkbox-xs"
                checked={valgte.includes(nokkel(gIdx, p))}
                onchange={() => veksle(gIdx, p)}
              />
            </td>
            <td class="pl-6 opacity-70" colspan="2">{p.navn}</td>
            <td class="text-right opacity-70">{minutterTilTimer(p.minutter)} t</td>
            <td></td>
          </tr>
        {/each}
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
    <div class="flex gap-2 flex-wrap items-center">
      <select
        class="select select-sm"
        value={partyNo}
        onchange={(e) => (valgtKunde = e.currentTarget.value)}
      >
        {#each kunder as p (p.party_no)}
          <option value={p.party_no}>{p.party_no} {p.name}</option>
        {/each}
      </select>
      {#if utvalg.entryIds.length}
        <button
          class="btn btn-sm btn-primary"
          onclick={() =>
            fakturer({ party_no: partyNo, entry_ids: utvalg.entryIds }, "Utvalget")}
        >
          Fakturer valgte ({minutterTilTimer(utvalg.minutter)} t — {kr(utvalg.belop)} kr)
        </button>
      {:else}
        <button
          class="btn btn-sm btn-primary"
          onclick={() => fakturer({ party_no: partyNo }, "Alt")}
        >
          Lag faktura for alt
        </button>
      {/if}
      <span class="text-xs opacity-60">
        Huk av personlinjer for å fakturere et utvalg — valgte timer låses idet fakturaen
        utstedes.
      </span>
    </div>
  {/if}
</Card>
