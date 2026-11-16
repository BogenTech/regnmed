<script>
  // «Varslet»-kravet fra #57 (docs/auth.md §8): hva leverandørens
  // plattformroller har gjort som angår dette selskapet, lest fra den
  // innsettings-bare loggen. Tomt kort er også et svar — det vises,
  // slik at fraværet av innsyn er synlig, ikke bare innsynet.
  import { formatTime } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  // `hvor`: "selskapet" or "byrået" — the same log exists for both.
  let { innsyn, hvor = "selskapet" } = $props();
  let hvorsin = $derived(hvor === "byrået" ? "byråets" : "selskapets");
</script>

<Card title="Leverandørtilgang">
  <p class="text-sm opacity-70 mb-2">
    Alt regnmeds støttepersonale gjør som angår {hvor}, blir logget og vist her. Ingen hos
    regnmed kan lese regnskapet til {hvor === "byrået" ? "byråets klienter" : "selskapet"}.
  </p>
  {#if innsyn.length === 0}
    <p class="text-sm opacity-70">Ingen fra regnmed har vært inne i {hvorsin} oppsett.</p>
  {:else}
    <ul class="divide-y divide-base-200 text-sm" aria-label="Leverandørtilgang">
      {#each innsyn as rad, i (i)}
        <li class="py-1.5">
          <span class="tabular-nums">{formatTime(rad.tidspunkt)}</span>, {rad.navn} ({rad.rolle})
          <div class="text-xs opacity-60 font-mono break-all">{rad.method} {rad.path}</div>
        </li>
      {/each}
    </ul>
  {/if}
</Card>
