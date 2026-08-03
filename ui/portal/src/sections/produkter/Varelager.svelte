<script>
  // Varelager (#39): beholdning og verdi er ALLTID beregnet fra de
  // innsettings-bare bevegelsene — ingenting lagres som saldo.
  import { api, post } from "../../lib/api.js";
  import { kr, parseKr, today, antallStr, parseAntall } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Bevegelser from "./Bevegelser.svelte";

  let { companyId, products, inventory, verdiOre, onDone } = $props();

  let lagerforte = $derived(products.filter((p) => p.lagerfort));

  // Produktvalget står på det første lagerførte til noe annet velges.
  let valgtProdukt = $state("");
  let produkt = $derived(valgtProdukt || lagerforte[0]?.nummer || "");
  let kind = $state("kjop");
  let dato = $state(today());
  let antall = $state("");
  let kost = $state("");
  let note = $state("");

  // Talt antall per produktnummer — tømmes når tellingen er bokført.
  let talt = $state({});
  let tellingDato = $state(today());

  let bevegelser = $state(null);

  async function registrer() {
    try {
      const body = {
        produkt,
        dato,
        kind,
        antall_milli: parseAntall(antall),
        note: note.trim() || null,
      };
      if (kost.trim()) body.kostpris_ore = parseKr(kost);
      await post("/companies/" + companyId + "/inventory/movements", body);
      toast("Bevegelse registrert", true);
      kind = "kjop";
      dato = today();
      antall = "";
      kost = "";
      note = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function bokforTelling() {
    const linjer = [];
    for (const r of inventory) {
      const verdi = talt[r.nummer];
      if (verdi != null && String(verdi).trim() !== "") {
        linjer.push({ produkt: r.nummer, talt_milli: parseAntall(verdi) });
      }
    }
    if (linjer.length === 0) {
      toast("Fyll inn talt antall for minst ett produkt", false);
      return;
    }
    try {
      const result = await post("/companies/" + companyId + "/inventory/count", {
        dato: tellingDato,
        linjer,
      });
      toast(
        "Telling registrert" +
          (result.voucher ? " — bilag " + result.voucher : " (ingen verdiendring)"),
        true,
      );
      talt = {};
      tellingDato = today();
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function visBevegelser(nummer) {
    try {
      const data = await api(
        "/companies/" + companyId + "/inventory/movements?produkt=" + encodeURIComponent(nummer),
      );
      bevegelser = { nummer, movements: data.movements };
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Varelager">
  <p class="text-sm opacity-70 mb-2">
    Beholdning og verdi beregnes alltid fra bevegelsene (gjennomsnittskost) — salg trekkes automatisk
    når faktura utstedes, kreditnota legger tilbake. Varetelling justerer beholdningen og bokfører
    verdiendringen mot 1460/4390.
  </p>
  <div class="flex flex-wrap gap-2 items-end mb-3">
    <select
      class="select select-sm"
      value={produkt}
      onchange={(e) => (valgtProdukt = e.currentTarget.value)}
    >
      {#each lagerforte as p (p.nummer)}
        <option value={p.nummer}>{p.nummer} {p.navn}</option>
      {/each}
    </select>
    <select class="select select-sm" bind:value={kind}>
      <option value="kjop">Varekjøp</option>
      <option value="justering">Justering</option>
    </select>
    <input type="date" class="input input-sm" bind:value={dato} />
    <input class="input input-sm w-20" placeholder="Antall" bind:value={antall} />
    <input class="input input-sm w-28" placeholder="Kost/stk (kr)" bind:value={kost} />
    <input class="input input-sm" placeholder="Notat (justering)" bind:value={note} />
    <button class="btn btn-sm" onclick={registrer}>Registrer</button>
  </div>
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Nr</th><th>Navn</th><th class="text-right">Beholdning</th>
        <th class="text-right">Snittkost</th><th class="text-right">Verdi</th>
        <th>Varetelling</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each inventory as r (r.nummer)}
        <tr>
          <td class="font-mono">{r.nummer}</td>
          <td>{r.navn}</td>
          <td class="text-right">{antallStr(r.antall_milli)}</td>
          <td class="text-right">{r.gjennomsnitt_ore == null ? "–" : kr(r.gjennomsnitt_ore)}</td>
          <td class="text-right">{kr(r.verdi_ore)}</td>
          <td>
            <input
              class="input input-xs w-20"
              placeholder="Talt"
              bind:value={talt[r.nummer]}
            />
          </td>
          <td>
            <button class="btn btn-xs btn-ghost" onclick={() => visBevegelser(r.nummer)}>
              Bevegelser
            </button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  <div class="flex gap-2 items-end mt-2">
    <input type="date" class="input input-sm" bind:value={tellingDato} />
    <button class="btn btn-sm btn-outline" onclick={bokforTelling}>
      Registrer telling og bokfør
    </button>
    <span class="text-sm opacity-70">Sum verdi: {kr(verdiOre)}</span>
  </div>
  {#if bevegelser}
    <div class="mt-3">
      <Bevegelser nummer={bevegelser.nummer} movements={bevegelser.movements} />
    </div>
  {/if}
</Card>
