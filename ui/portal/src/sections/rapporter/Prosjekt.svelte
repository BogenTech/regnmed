<script>
  // Prosjektlønnsomhet (#71): tjener vi penger på dette prosjektet?
  // Ren sammenstilling — dimensjonsfiltrerte SUM-spørringer, timenes
  // fakturert/ufakturert-splitt og kundekoblingen fra #80. Klikk på et
  // prosjekt åpner NS 4102-seksjonene for akkurat det.
  import { api } from "../../lib/api.js";
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import SeksjonRader from "./SeksjonRader.svelte";

  let { companyId, year } = $props();

  let data = $state(null);
  let detalj = $state(null);
  let valgt = $state("");

  $effect(() => {
    data = null;
    detalj = null;
    valgt = "";
    api("/companies/" + companyId + "/reports/prosjekt?year=" + year)
      .then((svar) => (data = svar))
      .catch((error) => toast(error.message, false));
  });

  async function velg(code) {
    if (valgt === code) {
      valgt = "";
      detalj = null;
      return;
    }
    valgt = code;
    detalj = null;
    try {
      detalj = await api(
        "/companies/" +
          companyId +
          "/reports/prosjekt?year=" +
          year +
          "&prosjekt=" +
          encodeURIComponent(code),
      );
    } catch (error) {
      toast(error.message, false);
      valgt = "";
    }
  }
</script>

{#if !data}
  <span class="loading loading-spinner"></span>
{:else if !data.prosjekter.length}
  <p class="opacity-70">
    Ingen prosjekter registrert — opprett dem i dimensjonsregisteret under Bilag, og knytt gjerne
    hvert prosjekt til kunden det er for.
  </p>
{:else}
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Prosjekt</th>
        <th>Kunde</th>
        <th class="text-right">Inntekter</th>
        <th class="text-right">Kostnader</th>
        <th class="text-right">Resultat</th>
        <th class="text-right">Timer fakturert</th>
        <th class="text-right">På bordet</th>
      </tr>
    </thead>
    <tbody>
      {#each data.prosjekter as p (p.prosjekt)}
        <tr
          class={"cursor-pointer hover:bg-base-200 " + (p.active ? "" : "opacity-50")}
          onclick={() => velg(p.prosjekt)}
        >
          <td>{p.prosjekt} {p.name}{p.active ? "" : " (avsluttet)"}</td>
          <td class="opacity-70">{p.kunde_navn || ""}</td>
          <td class="text-right">{kr(p.inntekter_ore)}</td>
          <td class="text-right">{kr(p.kostnader_ore)}</td>
          <td class={"text-right " + (p.resultat_ore < 0 ? "text-error" : "")}>
            {kr(p.resultat_ore)}
          </td>
          <td class="text-right">
            {minutterTilTimer(p.timer.fakturerte_minutter)} / {minutterTilTimer(p.timer.minutter)} t
          </td>
          <td class="text-right">{kr(p.timer.ufakturert_ore)}</td>
        </tr>
        {#if valgt === p.prosjekt && detalj}
          <tr>
            <td colspan="7" class="bg-base-200">
              <table class="table table-xs">
                <tbody>
                  {#each detalj.seksjoner as s (s.heading)}
                    {#if s.lines.length}
                      <SeksjonRader seksjon={s} />
                    {/if}
                  {/each}
                  <tr class="font-semibold">
                    <td></td>
                    <td>Dekningsbidrag</td>
                    <td class="text-right">{kr(detalj.resultat_ore)}</td>
                  </tr>
                </tbody>
              </table>
              <p class="text-xs opacity-60 mt-1">
                Timer i {year}: {minutterTilTimer(detalj.timer.minutter)} t totalt, fakturert
                {kr(detalj.timer.fakturert_ore)}, ufakturert {kr(detalj.timer.ufakturert_ore)}
                («på bordet»).
              </p>
            </td>
          </tr>
        {/if}
      {/each}
    </tbody>
  </table>
  <p class="text-xs opacity-60 mt-2">
    Beløp i presentasjonsfortegn; «på bordet» er fakturerbare timer som ennå ikke er fakturert,
    verdsatt til timesatsen. Bare posteringer med prosjektdimensjon telles med.
  </p>
{/if}
