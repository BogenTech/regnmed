<script>
  import { post } from "../../lib/api.js";
  import { today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, rates, onDone } = $props();

  let valuta = $state("");
  let dato = $state(today());
  let kurs = $state("");

  async function fetchRates() {
    const valutaer = prompt("Valutaer (kommaseparert):", "EUR,USD,SEK");
    if (!valutaer) return;
    try {
      const result = await post("/companies/" + companyId + "/currency/rates/fetch", {
        valutaer: valutaer.split(",").map((v) => v.trim()),
      });
      toast(result.fetched + " noteringer hentet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function addRate() {
    try {
      await post("/companies/" + companyId + "/currency/rates", {
        valuta: valuta.trim(),
        dato,
        kurs: kurs.trim(),
      });
      toast("Kurs registrert", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Valutakurser">
  <p class="text-sm opacity-70 mb-2">
    Dagskurser fra Norges Bank (eller manuelt, med kilde). Fakturaer i valuta bokføres i NOK til
    kursen på fakturadatoen; kursinformasjonen er del av bilagsbeviset.
  </p>
  <div class="flex flex-wrap gap-2 items-end mb-2">
    <button class="btn btn-sm btn-outline" onclick={fetchRates}>Hent fra Norges Bank</button>
    <input class="input input-sm w-20" placeholder="EUR" bind:value={valuta} />
    <input type="date" class="input input-sm" bind:value={dato} />
    <input class="input input-sm w-28" placeholder="Kurs (11,6543)" bind:value={kurs} />
    <button class="btn btn-sm" onclick={addRate}>Registrer manuelt</button>
  </div>
  {#if rates.length}
    <table class="table table-sm">
      <thead>
        <tr><th>Valuta</th><th class="text-right">Kurs</th><th>Dato</th><th>Kilde</th></tr>
      </thead>
      <tbody>
        {#each rates as r}
          <tr>
            <td class="font-mono">{r.valuta}</td>
            <td class="text-right font-mono">{r.kurs}</td>
            <td>{r.dato}</td>
            <td class="text-xs opacity-70">{r.kilde}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="opacity-70 text-sm">Ingen kurser ennå.</p>
  {/if}
</Card>
