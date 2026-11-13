<script>
  // Prosjektlønnsomhet (#71): tjener vi penger på dette prosjektet?
  // Ren sammenstilling — dimensjonsfiltrerte SUM-spørringer, timenes
  // fakturert/ufakturert-splitt og kundekoblingen fra #80. Klikk på et
  // prosjekt åpner NS 4102-seksjonene for akkurat det.
  import { api } from "../../lib/api.js";
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import SeksjonRader from "./SeksjonRader.svelte";

  let { companyId, year } = $props();

  let data = $state(null);
  let detalj = $state(null);
  let valgt = $state("");
  let error = $state(null);
  let seq = 0;

  function load() {
    const mine = ++seq;
    data = null;
    detalj = null;
    valgt = "";
    error = null;
    api("/companies/" + companyId + "/reports/prosjekt?year=" + year)
      .then((r) => mine === seq && (data = r))
      .catch((e) => mine === seq && (error = e.message));
  }
  $effect(load);

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

{#if error}
  <LoadError {error} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-sm" role="status" aria-label="Henter prosjektene"></span>
{:else if !data.prosjekter.length}
  <p class="opacity-70">
    Ingen prosjekter registrert — opprett dem i
    <a class="link" href={"#/c/" + companyId + "/prosjekter"}>Prosjekter</a>-seksjonen, og knytt
    gjerne hvert prosjekt til kunden det er for.
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
        <tr class={p.active ? "" : "opacity-60"}>
          <td>
            <button
              class="link link-hover text-left"
              aria-expanded={valgt === p.prosjekt}
              onclick={() => velg(p.prosjekt)}
            >
              {p.prosjekt} {p.name}{p.active ? "" : " (avsluttet)"}
            </button>
          </td>
          <td class="opacity-70">{p.kunde_navn || ""}</td>
          <td class="text-right tabular-nums whitespace-nowrap">{kr(p.inntekter_ore)}</td>
          <td class="text-right tabular-nums whitespace-nowrap">{kr(p.kostnader_ore)}</td>
          <td class={"text-right tabular-nums whitespace-nowrap " + (p.resultat_ore < 0 ? "text-error" : "")}>
            {kr(p.resultat_ore)}
          </td>
          <td class="text-right tabular-nums whitespace-nowrap">
            {minutterTilTimer(p.timer.fakturerte_minutter)} / {minutterTilTimer(p.timer.minutter)} t
          </td>
          <td class="text-right tabular-nums whitespace-nowrap">{kr(p.timer.ufakturert_ore)}</td>
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
                    <td class="text-right tabular-nums whitespace-nowrap">{kr(detalj.resultat_ore)}</td>
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
