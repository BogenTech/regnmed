<script>
  // Skattemessig saldo per gruppe — beregnet fra registeret hver gang,
  // aldri lagret. Negativ saldo avskrives ikke; den er
  // inntektsføringskandidat og håndteres manuelt (sktl. §14-45/§14-47).
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { saldo } = $props();
</script>

<Card title={"Saldoavskrivning " + saldo.year + " (skattemessig)"}>
  <p class="text-sm opacity-70 mb-2">
    Grunnlaget for næringsspesifikasjonen: saldo per gruppe etter skatteloven §14-43, beregnet fra
    registeret med satsene i satsregisteret. Negativ saldo avskrives ikke — den er
    inntektsføringskandidat og håndteres manuelt.
  </p>
  <table class="table table-sm">
    <thead>
      <tr>
        <th>Gruppe</th><th class="text-right">Inngående</th><th class="text-right">Tilgang</th>
        <th class="text-right">Vederlag</th><th class="text-right">Grunnlag</th>
        <th class="text-right">Sats</th><th class="text-right">Avskrivning</th>
        <th class="text-right">Utgående</th>
      </tr>
    </thead>
    <tbody>
      {#each saldo.grupper as g (g.gruppe)}
        <tr>
          <td>{g.gruppe} <span class="opacity-60 text-xs">{g.beskrivelse}</span></td>
          <td class="text-right">{kr(g.inngaende_ore)}</td>
          <td class="text-right">{kr(g.tilgang_ore)}</td>
          <td class="text-right">{kr(g.vederlag_ore)}</td>
          <td class="text-right">{kr(g.grunnlag_ore)}</td>
          <td class="text-right">{g.sats_bp / 100} %</td>
          <td class="text-right">{kr(g.avskrivning_ore)}</td>
          <td class="text-right">{kr(g.utgaende_ore)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  <div class="text-sm mt-2">
    Bokført verdi {saldo.year}: <b>{kr(saldo.bokfort_ore)}</b> · Skattemessig saldo:
    <b>{kr(saldo.skattemessig_ore)}</b> · Midlertidig forskjell:
    <b>{kr(saldo.forskjell_ore)}</b>
  </div>
</Card>
