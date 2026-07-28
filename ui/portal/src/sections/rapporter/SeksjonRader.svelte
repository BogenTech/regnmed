<script>
  // Én NS 4102-seksjon i resultat/balanse: linjene og sumlinjen.
  // Beløpene kommer i presentasjonsfortegn fra serveren.
  import { kr } from "../../lib/format.js";

  let { seksjon } = $props();
</script>

{#each seksjon.lines as l (l.number)}
  <tr>
    <td>{l.number}</td>
    <td>{l.name}</td>
    <td class="text-right">{kr(l.saldo_ore)}</td>
  </tr>
{/each}
<tr class="font-semibold">
  <td></td>
  <td>Sum {seksjon.heading.toLowerCase()}</td>
  <td class="text-right">{kr(seksjon.sum_ore)}</td>
</tr>
