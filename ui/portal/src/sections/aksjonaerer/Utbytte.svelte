<script>
  // Utbytte: ett vedtak, ikke ett beløp per eier — den enkeltes del er
  // aksjene på beslutningsdatoen ganger utbytte per aksje. Registering it
  // posts a voucher (2050 → 2800), so it is two steps: the server works
  // out each owner's part with ?preview=true (and tries the posting
  // without keeping it), the person sees the total, and only then posts.
  // The confirm carries the previewed total and is refused if the book
  // changed in between.
  import { tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { formatDate, kr, oreOrNull, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, utbytte, kanSkrive, onChanged } = $props();

  let dato = $state(today());
  let perAksje = $state("");
  let forslag = $state(null);
  let lagrer = $state(false);

  const fokus = async (id) => (await tick(), document.getElementById(id)?.focus());

  async function seOver(event) {
    event.preventDefault();
    const ore = oreOrNull(perAksje);
    if (ore == null || ore <= 0)
      return toast("Skriv inn utbytte per aksje i kroner, for eksempel 500", false);
    const body = { besluttet_dato: dato, per_aksje_ore: ore };
    try {
      const svar = await post("/companies/" + companyId + "/dividends?preview=true", body);
      forslag = { body, svar };
      fokus("utbytte-oversikt");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function bokfor() {
    if (lagrer || !forslag) return;
    lagrer = true;
    try {
      const svar = await post("/companies/" + companyId + "/dividends", {
        ...forslag.body,
        forventet_totalt_ore: forslag.svar.totalt_ore,
      });
      toast("Utbyttet er registrert og bokført: " + kr(svar.totalt_ore) + " kr", true);
      forslag = null;
      perAksje = "";
      dato = today();
      await onChanged();
      fokus("utbytte-per-aksje");
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }

  function endre() {
    forslag = null;
    fokus("utbytte-per-aksje");
  }
</script>

<Card title="Utbytte">
  <p class="text-sm opacity-70 mb-2">
    Hver eier får antall aksjer på vedtaksdatoen ganger utbytte per aksje. Vedtaket bokføres fra
    annen egenkapital (2050) til skyldig utbytte (2800).
  </p>
  {#if utbytte.length}
    <ul class="divide-y divide-base-200 text-sm">
      {#each utbytte as u (u.id)}
        <li class="py-2 flex gap-3">
          <span class="whitespace-nowrap w-20 shrink-0">{formatDate(u.besluttet_dato)}</span>
          <div class="min-w-0 flex-1">
            <div>{kr(u.per_aksje_ore)} kr per aksje</div>
            <div class="text-xs opacity-60">Registrert av {u.created_by}</div>
          </div>
          <span class="whitespace-nowrap text-right">{kr(u.totalt_ore)} kr</span>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70">Ingen utbyttevedtak registrert.</p>
  {/if}

  {#if kanSkrive}
    <h3 class="font-semibold mt-4">Nytt utbyttevedtak</h3>
    <form class="flex flex-wrap gap-x-3 gap-y-1 items-end text-sm" onsubmit={seOver}>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Vedtatt</span>
        <input
          type="date"
          class="input input-sm"
          required
          bind:value={dato}
          oninput={() => (forslag = null)}
        />
      </label>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Utbytte per aksje (kr)</span>
        <input
          id="utbytte-per-aksje"
          class="input input-sm w-32"
          inputmode="decimal"
          required
          bind:value={perAksje}
          oninput={() => (forslag = null)}
        />
      </label>
      <button class="btn btn-sm mb-1">Se over</button>
    </form>
    {#if forslag}
      {@const s = forslag.svar}
      <div
        id="utbytte-oversikt"
        tabindex="-1"
        role="region"
        aria-label="Oversikt over utbyttet"
        class="mt-3 p-3 rounded-box bg-base-200 text-sm"
      >
        <h3 class="font-semibold mb-1">
          Utbytte vedtatt {formatDate(forslag.body.besluttet_dato)}, {kr(
            forslag.body.per_aksje_ore,
          )} kr per aksje
        </h3>
        <ul class="mb-2">
          {#each s.fordeling as f, i (i)}
            <li class="flex gap-2">
              <span class="min-w-0 flex-1 truncate">{f.navn}</span>
              <span class="whitespace-nowrap opacity-60"
                >{f.antall_aksjer.toLocaleString("nb-NO")} aksjer</span
              >
              <span class="whitespace-nowrap w-28 text-right">{kr(f.belop_ore)} kr</span>
            </li>
          {/each}
        </ul>
        <p>
          Totalt <span class="font-medium">{kr(s.totalt_ore)} kr</span> bokføres fra konto 2050 til
          2800.
        </p>
        <div class="flex gap-2 mt-2">
          <button class="btn btn-sm btn-primary" disabled={lagrer} onclick={bokfor}>
            Registrer og bokfør
          </button>
          <button class="btn btn-sm btn-ghost" onclick={endre}>Endre</button>
        </div>
      </div>
    {/if}
  {/if}
</Card>
