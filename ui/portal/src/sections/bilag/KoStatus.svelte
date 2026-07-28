<script>
  import { koLes } from "../../lib/ko.js";

  let antall = $state(0);

  $effect(() => {
    koLes()
      .then((rader) => (antall = rader.length))
      .catch(() => (antall = 0));
  });
</script>

{#if antall}
  <div class="text-xs opacity-70 mt-2">
    <span class="badge badge-warning badge-sm">{antall} i kø</span>
    venter på dekning — sendes automatisk når nettet er tilbake
  </div>
{/if}
