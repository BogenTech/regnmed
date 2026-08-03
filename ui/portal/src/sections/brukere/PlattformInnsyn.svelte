<script>
  // «Varslet»-kravet fra #57 (docs/auth.md §8): hva leverandørens
  // plattformroller har gjort som angår dette selskapet, lest fra den
  // innsettings-bare loggen. Tomt kort er også et svar — det vises,
  // slik at fraværet av innsyn er synlig, ikke bare innsynet.
  let { innsyn } = $props();
  import Card from "../../components/Card.svelte";
</script>

<Card title="Leverandørtilgang">
  <p class="text-sm opacity-70 mb-2">
    Alt plattformens støtteroller gjør som angår selskapet, logges og vises her
    (docs/auth.md §8). Ingen leverandørrolle kan lese selskapets regnskap.
  </p>
  {#if innsyn.length === 0}
    <p class="opacity-70">Ingen plattformtilgang er registrert.</p>
  {:else}
    <table class="table table-sm">
      <thead>
        <tr><th>Tidspunkt</th><th>Hvem</th><th>Rolle</th><th>Hva</th></tr>
      </thead>
      <tbody>
        {#each innsyn as rad, i (i)}
          <tr>
            <td>{new Date(rad.tidspunkt).toLocaleString("nb-NO")}</td>
            <td>{rad.navn}</td>
            <td>{rad.rolle}</td>
            <td class="font-mono text-xs">{rad.method} {rad.path}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
