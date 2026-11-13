<script>
  // A year stepper with names a screen reader can say.
  let { year = $bindable() } = $props();
</script>

<div class="join" role="group" aria-label="År">
  <button class="btn btn-sm join-item" aria-label={"Forrige år, " + (year - 1)} onclick={() => (year -= 1)}>‹</button>
  <span class="btn btn-sm join-item pointer-events-none tabular-nums" aria-live="polite">{year}</span>
  <button class="btn btn-sm join-item" aria-label={"Neste år, " + (year + 1)} onclick={() => (year += 1)}>›</button>
</div>
