<script>
  import { kr, minutterTilTimer } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { summary } = $props();
</script>

<Card title="Per prosjekt (denne uken)">
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Prosjekt</th><th class="text-right">Timer</th>
        <th class="text-right">Fakturerbart</th><th class="text-right">Ufakturert</th>
      </tr>
    </thead>
    <tbody>
      {#each summary as s}
        <tr>
          <td>{s.prosjekt ? s.prosjekt : "(uten prosjekt)"}</td>
          <td class="text-right">{minutterTilTimer(s.minutter)} t</td>
          <td class="text-right">{minutterTilTimer(s.fakturerbare_minutter)} t</td>
          <td class="text-right">{kr(s.ufakturert_ore)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</Card>
