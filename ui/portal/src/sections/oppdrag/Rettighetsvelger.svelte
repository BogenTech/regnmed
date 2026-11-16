<script>
  // Rettighetene gruppert som portalen selv er gruppert. Slug-en står i
  // tittelen for den som trenger den; teksten er det man leser.
  // Tilgangsstyrende rettigheter kan ikke delegeres til en egen rolle —
  // de er slått av her, ikke skjult, så det er synlig hvorfor.
  import { antallValgt } from "../brukere/brukere.js";

  let { vokabular, valgte = $bindable([]) } = $props();

  let grupper = $derived.by(() => {
    const map = new Map();
    for (const v of vokabular) {
      if (!map.has(v.gruppe)) map.set(v.gruppe, []);
      map.get(v.gruppe).push(v);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0], "nb"));
  });

  function toggle(rett, checked) {
    if (checked) {
      if (!valgte.includes(rett)) valgte = [...valgte, rett];
    } else {
      valgte = valgte.filter((r) => r !== rett);
    }
  }
</script>

<!-- details/summary i stedet for en bred tabell: det er det som tåler
     375 px uten å måtte rulle sidelengs. The count on each closed group
     shows what is chosen without opening it. -->
{#each grupper as [gruppe, retter] (gruppe)}
  {@const n = antallValgt(retter, valgte)}
  <details class="collapse collapse-arrow border border-base-300 mb-1">
    <summary class="collapse-title min-h-0 py-2 pl-3 pr-8 text-sm font-semibold">
      {gruppe}
      <span class="font-normal {n ? '' : 'opacity-60'}">({n} av {retter.length})</span>
    </summary>
    <div class="collapse-content pl-3">
      {#each retter as v (v.rett)}
        <label
          class="flex items-start gap-2 py-1 {v.kan_delegeres ? 'cursor-pointer' : 'opacity-60'}"
          title={v.rett}
        >
          <input
            type="checkbox"
            class="checkbox checkbox-sm mt-0.5"
            checked={valgte.includes(v.rett)}
            disabled={!v.kan_delegeres}
            onchange={(e) => toggle(v.rett, e.currentTarget.checked)}
          />
          <span class="text-sm">
            {v.beskrivelse}
            {#if !v.kan_delegeres}
              <span class="opacity-70">(kan ikke gis til en egen rolle, siden den styrer hvem som har tilgang)</span>
            {/if}
          </span>
        </label>
      {/each}
    </div>
  </details>
{/each}
