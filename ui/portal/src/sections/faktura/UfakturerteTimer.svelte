<script>
  // Fakturagrunnlaget: ufakturerte fakturerbare timer gruppert per
  // (prosjekt, sats), med hver persons timer som egen linje. Den som
  // holder TIMER_FAKTURER velger hva som blir med: alt, ett prosjekt
  // (forslagsknappene, #80), eller et UTVALG av personlinjer — utvalget
  // faktureres og låses i samme transaksjon, så valgte timer aldri kan
  // endres i mellomtiden (docs/timer.md).
  //
  // A faktura cannot be undone, so nothing here issues one without the
  // user having named the customer and confirmed the amount.
  import { post } from "../../lib/api.js";
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, unbilled, kunder, onDone } = $props();

  let valgte = $state([]);
  function nokkel(gIdx, p) {
    return gIdx + ":" + p.person_id;
  }
  function veksle(gIdx, p) {
    const n = nokkel(gIdx, p);
    valgte = valgte.includes(n) ? valgte.filter((v) => v !== n) : [...valgte, n];
  }

  let sum = $derived(
    unbilled.reduce((acc, g) => acc + Math.round((g.minutter * g.timesats_ore) / 60), 0),
  );

  let utvalg = $derived.by(() => {
    let minutter = 0;
    let belop = 0;
    let entryIds = [];
    const groups = new Set();
    unbilled.forEach((g, gIdx) => {
      for (const p of g.personer || []) {
        if (valgte.includes(nokkel(gIdx, p))) {
          minutter += p.minutter;
          belop += Math.round((p.minutter * g.timesats_ore) / 60);
          entryIds = entryIds.concat(p.entry_ids);
          groups.add(g);
        }
      }
    });
    return { minutter, belop, entryIds, groups: [...groups] };
  });

  // The customer is suggested only when the hours themselves say whose
  // they are: every chosen (or, with nothing chosen, every) group belongs
  // to one customer-linked project. Never "the first customer in the list".
  let suggested = $derived.by(() => {
    const groups = utvalg.groups.length ? utvalg.groups : unbilled;
    const owners = new Set(groups.map((g) => g.kunde || ""));
    return owners.size === 1 ? [...owners][0] : "";
  });
  let valgtKunde = $state("");
  let partyNo = $derived(valgtKunde || suggested);
  let kunde = $derived(kunder.find((k) => k.party_no === partyNo));

  // One suggestion per DISTINCT customer-linked prosjekt (groups are
  // per sats, so the same prosjekt can appear several times).
  let forslag = $derived.by(() => {
    const sett = new Map();
    for (const g of unbilled) {
      if (g.prosjekt && g.kunde && !sett.has(g.prosjekt)) sett.set(g.prosjekt, g);
    }
    return [...sett.values()].map((g) => {
      const groups = unbilled.filter((x) => x.prosjekt === g.prosjekt);
      return {
        ...g,
        minutter: groups.reduce((s, x) => s + x.minutter, 0),
        belop: groups.reduce((s, x) => s + Math.round((x.minutter * x.timesats_ore) / 60), 0),
      };
    });
  });

  async function fakturer(body, customerName, minutter, belop) {
    const ok = await bekreft(
      "Faktura til " + customerName + " for " + minutterTilTimer(minutter) + " timer, " + kr(belop) +
        " kr pluss mva. Timene låses, og en utstedt faktura kan ikke endres — feil rettes med kreditnota.",
      { tittel: "Utstede fakturaen?", ok: "Utsted faktura" },
    );
    if (!ok) return;
    try {
      const issued = await post("/companies/" + companyId + "/timesheet/invoice", body);
      toast("Faktura " + issued.invoice_no + " er utstedt", true);
      valgte = [];
      onDone();
      location.hash = "#/c/" + companyId + "/faktura/" + issued.invoice_id;
    } catch (error) {
      toast(error.message, false);
    }
  }

  function fakturerValgt() {
    if (!kunde) return toast("Velg hvilken kunde fakturaen skal til", false);
    if (utvalg.entryIds.length) {
      fakturer({ party_no: partyNo, entry_ids: utvalg.entryIds }, kunde.name, utvalg.minutter, utvalg.belop);
    } else {
      const minutter = unbilled.reduce((s, g) => s + g.minutter, 0);
      fakturer({ party_no: partyNo }, kunde.name, minutter, sum);
    }
  }
</script>

<p class="mb-4">
  <b class="tabular-nums">{kr(sum)} kr</b> i fakturerbare timer som ikke er fakturert. Huk av
  personlinjer for å fakturere et utvalg, eller fakturer alt.
</p>

{#if forslag.length}
  <div class="flex gap-2 flex-wrap mb-4">
    {#each forslag as f (f.prosjekt)}
      <button
        class="btn btn-sm btn-outline"
        onclick={() =>
          fakturer({ party_no: f.kunde, prosjekt: f.prosjekt }, f.kunde_navn, f.minutter, f.belop)}
      >
        Fakturer prosjekt {f.prosjekt} til {f.kunde_navn}
      </button>
    {/each}
  </div>
{/if}

<Card title="Timer per prosjekt">
  <table class="table table-sm mb-2">
    <caption class="sr-only">Ufakturerte timer per prosjekt og timesats, med hver persons timer.</caption>
    <thead>
      <tr>
        <th scope="col"><span class="sr-only">Ta med</span></th>
        <th scope="col">Prosjekt</th>
        <th scope="col" class="hidden sm:table-cell">Kunde</th>
        <th scope="col" class="text-right">Timer</th>
        <th scope="col" class="text-right">Sats</th>
      </tr>
    </thead>
    <tbody>
      {#each unbilled as g, gIdx (gIdx)}
        <tr>
          <td></td>
          <td class="font-medium">{g.prosjekt ? g.prosjekt : "Uten prosjekt"}</td>
          <td class="opacity-70 hidden sm:table-cell">{g.kunde_navn || ""}</td>
          <td class="text-right tabular-nums whitespace-nowrap">{minutterTilTimer(g.minutter)} t</td>
          <td class="text-right tabular-nums whitespace-nowrap">{kr(g.timesats_ore)}/t</td>
        </tr>
        {#each g.personer || [] as p (nokkel(gIdx, p))}
          <tr class="text-sm">
            <td class="w-8">
              <input
                id={"timer-" + nokkel(gIdx, p)}
                type="checkbox"
                class="checkbox checkbox-xs"
                checked={valgte.includes(nokkel(gIdx, p))}
                onchange={() => veksle(gIdx, p)}
              />
            </td>
            <td class="pl-6" colspan="2">
              <label for={"timer-" + nokkel(gIdx, p)} class="cursor-pointer">
                {p.navn}<span class="sr-only">, {g.prosjekt || "uten prosjekt"}</span>
              </label>
            </td>
            <td class="text-right tabular-nums opacity-70 whitespace-nowrap">{minutterTilTimer(p.minutter)} t</td>
            <td></td>
          </tr>
        {/each}
      {/each}
    </tbody>
  </table>

  {#if kunder.length}
    <div class="flex gap-2 flex-wrap items-end">
      <label class="flex flex-col gap-1 text-sm">
        Fakturaen går til
        <select
          class="select select-sm"
          value={partyNo}
          onchange={(e) => (valgtKunde = e.currentTarget.value)}
        >
          <option value="" disabled>Velg kunde</option>
          {#each kunder as p (p.party_no)}
            <option value={p.party_no}>{p.name} ({p.party_no})</option>
          {/each}
        </select>
      </label>
      <button class="btn btn-sm btn-primary" onclick={fakturerValgt}>
        {#if utvalg.entryIds.length}
          Fakturer valgte ({minutterTilTimer(utvalg.minutter)} t, {kr(utvalg.belop)} kr)
        {:else}
          Fakturer alle timene ({kr(sum)} kr)
        {/if}
      </button>
    </div>
  {:else}
    <p class="text-sm">
      Du trenger en kunde før timene kan faktureres.
      <a class="link" href={"#/c/" + companyId + "/kunder/ny"}>Opprett kunde</a>
    </p>
  {/if}
</Card>
