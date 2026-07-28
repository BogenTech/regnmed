<script>
  // Avviksrapporten NAVNGIR alltid budsjettversjonen den måler mot —
  // uten et budsjett leses alt budsjettert som null, og det sies rett ut.
  import { kr } from "../../lib/format.js";

  let { av, year } = $props();
</script>

<h3 class="font-semibold mt-6 mb-1">Avvik hittil (t.o.m. måned {av.t_o_m_maned})</h3>
<p class="text-sm mb-2">
  {#if av.budsjett}
    Sammenlignet mot <strong>{av.budsjett.navn} v{av.budsjett.versjon}</strong>
    ({av.budsjett.status}).
  {:else}
    Ingen budsjett for {year} — alt budsjettert leses som null.
  {/if}
</p>

<table class="table table-sm">
  <thead>
    <tr>
      <th>Konto</th><th>Navn</th>
      <th class="text-right">Budsjett</th><th class="text-right">Faktisk</th>
      <th class="text-right">Avvik</th><th class="text-right">Budsjett år</th>
    </tr>
  </thead>
  <tbody>
    {#each av.seksjoner as s (s.heading)}
      {#if s.linjer.length}
        {#each s.linjer as l (l.account)}
          <tr>
            <td>{l.account}</td>
            <td>{l.name}</td>
            <td class="text-right">{kr(l.budsjett_hittil_ore)}</td>
            <td class="text-right">{kr(l.faktisk_hittil_ore)}</td>
            <td class="text-right {l.avvik_hittil_ore < 0 ? 'text-error' : ''}">
              {kr(l.avvik_hittil_ore)}
            </td>
            <td class="text-right opacity-60">{kr(l.budsjett_ar_ore)}</td>
          </tr>
        {/each}
        <tr class="font-semibold">
          <td></td>
          <td>Sum {s.heading.toLowerCase()}</td>
          <td class="text-right">{kr(s.budsjett_hittil_ore)}</td>
          <td class="text-right">{kr(s.faktisk_hittil_ore)}</td>
          <td class="text-right">{kr(s.avvik_hittil_ore)}</td>
          <td class="text-right opacity-60">{kr(s.budsjett_ar_ore)}</td>
        </tr>
      {/if}
    {/each}
    <tr class="font-bold">
      <td></td>
      <td>Resultat</td>
      <td class="text-right">{kr(av.resultat_budsjett_hittil_ore)}</td>
      <td class="text-right">{kr(av.resultat_faktisk_hittil_ore)}</td>
      <td class="text-right">{kr(av.resultat_avvik_hittil_ore)}</td>
      <td class="text-right opacity-60">{kr(av.resultat_budsjett_ar_ore)}</td>
    </tr>
  </tbody>
</table>
