<script>
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, utbytte, onDone } = $props();

  let dato = $state(today());
  let perAksje = $state("");

  async function opprett() {
    try {
      const made = await post("/companies/" + companyId + "/dividends", {
        besluttet_dato: dato,
        per_aksje_ore: parseKr(perAksje.trim()),
      });
      toast("Utbytte registrert og bokført: " + kr(made.totalt_ore), true);
      dato = today();
      perAksje = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Utbytte">
  <p class="text-sm opacity-70 mb-2">
    Ett vedtak, ikke ett beløp per eier: beløpet den enkelte får er antall aksjer på
    beslutningsdatoen ganger utbytte per aksje, så delene kan aldri avvike fra helheten. Vedtaket
    bokføres (2050 → 2800) i samme transaksjon som det registreres.
  </p>
  {#if utbytte.length}
    <table class="table table-sm mb-4">
      <thead>
        <tr>
          <th>Besluttet</th>
          <th class="text-right">Per aksje</th>
          <th class="text-right">Totalt</th>
          <th>Registrert av</th>
        </tr>
      </thead>
      <tbody>
        {#each utbytte as u}
          <tr>
            <td>{u.besluttet_dato}</td>
            <td class="text-right">{kr(u.per_aksje_ore)}</td>
            <td class="text-right">{kr(u.totalt_ore)}</td>
            <td class="text-xs opacity-70">{u.created_by}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="grid gap-2 max-w-md">
    <div class="grid grid-cols-2 gap-2">
      <input type="date" class="input input-sm input-bordered" bind:value={dato} />
      <input
        class="input input-sm input-bordered"
        placeholder="Utbytte per aksje (kr)"
        bind:value={perAksje}
      />
    </div>
    <button class="btn btn-sm" onclick={opprett}>Registrer utbyttevedtak</button>
  </div>
</Card>
