<script>
  // Redigerbart rutenett for utkast: konto × 12 måneder.
  // Linjene lagres i PRESENTASJONSFORTEGN (inntekt positiv, kostnad
  // positiv) — budsjettet skrives slik det leses, og faktiske tall
  // konverteres på serveren før sammenligningen.
  import { api, send } from "../../lib/api.js";
  import { kr, parseKr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, budsjett, onDone } = $props();

  const MANEDER = ["jan", "feb", "mar", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "des"];

  let rader = $state(null);
  let nyKonto = $state("");
  let nyBelop = $state("");
  // Cells are only saved by «Lagre linjer»; say so while there is
  // something unsaved, so nobody leaves believing it is stored.
  let endret = $state(false);

  $effect(() => {
    rader = null;
    api("/companies/" + companyId + "/budgets/" + budsjett.budget_id)
      .then((detalj) => {
        const perKonto = {};
        detalj.lines.forEach((l) => {
          if (!perKonto[l.account]) {
            perKonto[l.account] = { name: l.account_name, celler: new Array(12).fill("") };
          }
          perKonto[l.account].celler[l.maned - 1] = l.belop_ore ? kr(l.belop_ore) : "";
        });
        rader = Object.keys(perKonto)
          .sort()
          .map((konto) => ({ konto, name: perKonto[konto].name, celler: perKonto[konto].celler }));
      })
      .catch((error) => toast(error.message, false));
  });

  function leggTil() {
    const konto = nyKonto.trim();
    if (!konto) return;
    try {
      const perManed = nyBelop.trim();
      const visning = perManed ? kr(parseKr(perManed)) : "";
      rader.push({ konto, name: "", celler: new Array(12).fill(visning) });
      endret = true;
      nyKonto = "";
      nyBelop = "";
    } catch (error) {
      toast(error.message, false);
    }
  }

  function fjern(i) {
    rader.splice(i, 1);
    endret = true;
  }

  async function lagre() {
    const lines = [];
    let feil = null;
    rader.forEach((rad) => {
      rad.celler.forEach((raw, index) => {
        const verdi = raw.trim();
        if (!verdi) return;
        try {
          lines.push({ account: rad.konto, maned: index + 1, belop_ore: parseKr(verdi) });
        } catch (error) {
          feil = error.message;
        }
      });
    });
    if (feil) {
      toast(feil, false);
      return;
    }
    try {
      await send("PUT", "/companies/" + companyId + "/budgets/" + budsjett.budget_id + "/lines", {
        lines,
      });
      toast(lines.length + " budsjettlinjer lagret", true);
      endret = false;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<h3 class="font-semibold mt-6 mb-1">Rediger {budsjett.navn} (v{budsjett.versjon})</h3>
<p class="text-sm opacity-70 mb-2">
  Beløpene skrives slik de leses: inntekt positiv, kostnad positiv. Tomme celler er null.
</p>

{#if !rader}
  <span class="loading loading-spinner loading-sm"></span>
{:else}
  <div class="overflow-x-auto">
    <table class="table table-xs" oninput={() => (endret = true)}>
      <caption class="sr-only">Budsjett per konto og måned, i kroner.</caption>
      <thead>
        <tr>
          <th>Konto</th>
          {#each MANEDER as m}<th class="text-right">{m}</th>{/each}
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each rader as rad, i}
          <tr>
            <td class="whitespace-nowrap">
              {rad.konto}
              {#if rad.name}<span class="opacity-60 text-xs">{rad.name}</span>{/if}
            </td>
            {#each rad.celler as _, m}
              <td>
                <input
                  class="input input-xs w-20 text-right tabular-nums"
                  inputmode="decimal"
                  aria-label={"Konto " + rad.konto + ", " + MANEDER[m]}
                  bind:value={rad.celler[m]}
                />
              </td>
            {/each}
            <td>
              <button class="btn btn-xs btn-ghost" aria-label={"Fjern konto " + rad.konto} onclick={() => fjern(i)}>×</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="flex gap-2 items-end flex-wrap mt-3">
    <label class="flex flex-col gap-1 text-sm">
      Konto
      <input class="input input-sm w-24" inputmode="numeric" bind:value={nyKonto} />
    </label>
    <label class="flex flex-col gap-1 text-sm">
      Beløp per måned
      <input class="input input-sm w-32" inputmode="decimal" bind:value={nyBelop} />
    </label>
    <button class="btn btn-sm btn-ghost" onclick={leggTil}>Legg til konto</button>
    <button class="btn btn-sm btn-primary" onclick={lagre}>Lagre linjer</button>
    {#if endret}<span class="text-sm text-warning" role="status">Ulagrede endringer</span>{/if}
  </div>
{/if}
