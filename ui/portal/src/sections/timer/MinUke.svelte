<script>
  // Egne timer: redigerbare til måneden låses eller timene faktureres —
  // en fakturert linje viser fakturaen i stedet for slettknappen.
  import { api, post } from "../../lib/api.js";
  import { kr, parseKr, today, minutterTilTimer } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import DimRegisterLenke from "../../components/DimRegisterLenke.svelte";
  import DimSelect from "../../components/DimSelect.svelte";

  let { companyId, uke, from, to, forrige, neste, dims, onDone } = $props();

  let dato = $state(today());
  let timer = $state("");
  let beskrivelse = $state("");
  let prosjekt = $state("");
  let fakturerbar = $state(false);
  let sats = $state("");

  let ukeTotal = $derived(uke.entries.reduce((sum, e) => sum + e.minutter, 0));

  async function forTimer() {
    try {
      await post("/companies/" + companyId + "/timesheet", {
        dato,
        minutter: Math.round(Number(String(timer).replace(",", ".")) * 60),
        beskrivelse,
        prosjekt: prosjekt || null,
        fakturerbar,
        timesats_ore: fakturerbar ? parseKr(sats) : null,
      });
      dato = today();
      timer = "";
      beskrivelse = "";
      prosjekt = "";
      fakturerbar = false;
      sats = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function slett(e) {
    try {
      await api("/companies/" + companyId + "/timesheet/" + e.entry_id, { method: "DELETE" });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title={"Min uke " + from + " – " + to}>
  <div class="flex gap-1 items-center">
    <a class="btn btn-ghost btn-xs" href={"#/c/" + companyId + "/timer?uke=" + forrige}>«</a>
    <a class="btn btn-ghost btn-xs" href={"#/c/" + companyId + "/timer?uke=" + neste}>»</a>
  </div>
  {#if uke.locked_through}
    <p class="text-xs opacity-70 mb-2">Låst t.o.m. {uke.locked_through}</p>
  {/if}
  <div class="flex flex-wrap gap-2 mb-3 items-end">
    <input type="date" class="input input-sm" bind:value={dato} />
    <input class="input input-sm w-20" placeholder="Timer" bind:value={timer} />
    <input
      class="input input-sm flex-1"
      placeholder="Hva jobbet du med?"
      bind:value={beskrivelse}
    />
    <DimSelect {dims} kind="prosjekt" cls="select select-sm" bind:value={prosjekt}>
      {#snippet tomHint()}
        <DimRegisterLenke
          {companyId}
          tekst="Ingen prosjekter ennå — opprett dem i"
          ansattTekst="Ingen prosjekter er opprettet ennå."
        />
      {/snippet}
    </DimSelect>
    <label class="label cursor-pointer gap-1">
      <input type="checkbox" class="checkbox checkbox-xs" bind:checked={fakturerbar} />
      <span class="text-xs">Fakturerbar</span>
    </label>
    <input class="input input-sm w-24" placeholder="Sats (kr/t)" bind:value={sats} />
    <button class="btn btn-sm btn-primary" onclick={forTimer}>Før timer</button>
  </div>
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Dato</th><th>Hvem</th><th>Beskrivelse</th><th>Prosjekt</th>
        <th class="text-right">Timer</th><th>Sats</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each uke.entries as e (e.entry_id)}
        <tr>
          <td>{e.dato}</td>
          <td>{e.person}</td>
          <td>{e.beskrivelse}</td>
          <td>{e.prosjekt ? e.prosjekt : "–"}</td>
          <td class="text-right">{minutterTilTimer(e.minutter)} t</td>
          <td>{e.fakturerbar ? kr(e.timesats_ore) + "/t" : "–"}</td>
          <td>
            {#if e.own && !e.invoice_no}
              <button class="btn btn-xs btn-ghost" onclick={() => slett(e)}>Slett</button>
            {:else if e.invoice_no}
              <span class="badge badge-ghost badge-xs">faktura {e.invoice_no}</span>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  <p class="text-sm mt-2">Sum uke: <b>{minutterTilTimer(ukeTotal)} t</b></p>
</Card>
