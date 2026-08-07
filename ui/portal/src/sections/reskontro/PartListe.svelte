<script>
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { companyId, parties } = $props();
</script>

<!-- Alle parter, begge typer. Den lovpålagte spesifikasjonen bor under
     Rapporter → Reskontro; dette er arbeidsvisningen på tvers, mens
     Kunder og Leverandører har hver sin egen seksjon. -->
<Card title="Alle parter">
  <table class="table table-sm">
    <thead>
      <tr><th>Nr</th><th>Navn</th><th>Type</th><th class="text-right">Saldo</th></tr>
    </thead>
    <tbody>
      {#each parties as p (p.party_id)}
        <tr>
          <td>{p.party_no}</td>
          <td>
            <a class="link" href={"#/c/" + companyId + "/reskontro/" + p.party_id}>{p.name}</a>
          </td>
          <td>{p.kind === "leverandor" ? "Leverandør" : "Kunde"}</td>
          <td class="text-right">{kr(p.saldo_ore)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</Card>
