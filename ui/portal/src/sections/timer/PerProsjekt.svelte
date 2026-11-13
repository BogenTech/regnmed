<script>
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import DimRegisterLenke from "../../components/DimRegisterLenke.svelte";

  let { companyId, summary, dims = [] } = $props();

  const navn = (kode) => dims.find((d) => d.kind === "prosjekt" && d.code === kode)?.name || "";
</script>

<Card title="Per prosjekt denne uken">
  <div class="overflow-x-auto">
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Prosjekt</th><th class="text-right">Timer</th>
          <th class="text-right hidden sm:table-cell">Fakturerbart</th><th class="text-right">Ufakturert</th>
        </tr>
      </thead>
      <tbody>
        {#each summary as s}
          <tr>
            <td>
              {#if s.prosjekt}
                <span class="font-medium">{s.prosjekt}</span>
                <span class="text-xs opacity-60 block sm:inline">{navn(s.prosjekt)}</span>
              {:else}
                Uten prosjekt
              {/if}
            </td>
            <td class="text-right whitespace-nowrap">{minutterTilTimer(s.minutter)} t</td>
            <td class="text-right whitespace-nowrap hidden sm:table-cell">
              {minutterTilTimer(s.fakturerbare_minutter)} t
            </td>
            <td class="text-right whitespace-nowrap">{kr(s.ufakturert_ore)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <DimRegisterLenke {companyId} tekst="Prosjekter opprettes og avsluttes i" />
</Card>
