<script>
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { companyId, yearly, year, termin, report } = $props();

  // Årstermin har bare én periode — da velger man år, ikke termin.
  let tittel = $derived(
    "Mva-spesifikasjon " +
      (report ? report.label : yearly ? "Årstermin " + year : termin + ". termin " + year),
  );
</script>

<Card title={tittel}>
  <div class="join mb-2">
    {#if yearly}
      {#each [year - 1, year, year + 1] as y}
        <a
          class="join-item btn btn-sm {y === year ? 'btn-primary' : ''}"
          href={"#/c/" + companyId + "/mva?year=" + y}>{y}</a
        >
      {/each}
    {:else}
      {#each [1, 2, 3, 4, 5, 6] as t}
        <a
          class="join-item btn btn-sm {t === termin ? 'btn-primary' : ''}"
          href={"#/c/" + companyId + "/mva?year=" + year + "&termin=" + t}>{t}</a
        >
      {/each}
    {/if}
  </div>
  {#if report}
    <p class="text-sm opacity-70 mb-2">Leveringsfrist {report.frist}</p>
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Kode</th><th>Beskrivelse</th>
          <th class="text-right">Grunnlag</th><th class="text-right">Avgift</th>
        </tr>
      </thead>
      <tbody>
        {#each report.lines as l}
          <tr>
            <td>{l.code}</td>
            <td>{l.description}</td>
            <td class="text-right">{kr(l.grunnlag_ore)}</td>
            <td class="text-right">{kr(l.avgift_ore)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    <div class="stats bg-base-200 mt-4">
      <div class="stat">
        <div class="stat-title">Utgående</div>
        <div class="stat-value text-lg">{kr(report.utgaende_ore)}</div>
      </div>
      <div class="stat">
        <div class="stat-title">Inngående</div>
        <div class="stat-value text-lg">{kr(report.inngaende_ore)}</div>
      </div>
      <div class="stat">
        <div class="stat-title">{report.netto_ore >= 0 ? "Å betale" : "Til gode"}</div>
        <div class="stat-value text-lg">{kr(Math.abs(report.netto_ore))}</div>
      </div>
    </div>
  {:else}
    <p class="opacity-70">Ingen mva-posteringer i terminen.</p>
  {/if}
</Card>
