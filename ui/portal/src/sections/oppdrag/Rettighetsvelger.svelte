<script>
  // Rettighetene gruppert som portalen selv er gruppert. Slug-en står i
  // tittelen for den som trenger den; teksten er det man leser.
  // Tilgangsstyrende rettigheter kan ikke delegeres til en egen rolle —
  // de er slått av her, ikke skjult, så det er synlig hvorfor.
  let { vokabular, valgte = $bindable([]) } = $props();

  let grupper = $derived.by(() => {
    const map = new Map();
    for (const v of vokabular) {
      if (!map.has(v.gruppe)) map.set(v.gruppe, []);
      map.get(v.gruppe).push(v);
    }
    return [...map.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
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
     375 px uten å måtte rulle sidelengs. -->
{#each grupper as [gruppe, retter] (gruppe)}
  <details class="mb-1">
    <summary class="cursor-pointer text-sm font-semibold py-1">{gruppe}</summary>
    <div class="pl-2">
      {#each retter as v (v.rett)}
        <label
          class="flex items-start gap-2 py-0.5 {v.kan_delegeres ? '' : 'opacity-50'}"
          title={v.rett}
        >
          <input
            type="checkbox"
            class="checkbox checkbox-xs mt-0.5"
            checked={valgte.includes(v.rett)}
            disabled={!v.kan_delegeres}
            onchange={(e) => toggle(v.rett, e.currentTarget.checked)}
          />
          <span class="text-sm">
            {v.beskrivelse}
            {#if !v.kan_delegeres}
              <span class="opacity-70">
                — kan bare gis av admin, siden den styrer hvem som har tilgang
              </span>
            {/if}
          </span>
        </label>
      {/each}
    </div>
  </details>
{/each}
