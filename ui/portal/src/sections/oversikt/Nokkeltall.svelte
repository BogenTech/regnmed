<script>
  // Resultat per måned (#36): rene SUM-tall fra serveren, CSS-søyler
  // uten diagrambibliotek — frugality. Underskudd tegnes NEDOVER fra
  // nullinjen, aldri som en rød søyle oppover: retningen er tallet.
  import { kr } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";

  let { tall } = $props();

  const MND_NAVN = ["jan", "feb", "mar", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "des"];

  let maxAbs = $derived(Math.max(...tall.maaneder.map(Math.abs), 1));
  let harUnderskudd = $derived(tall.maaneder.some((v) => v < 0));
  const hoyde = (v) => Math.max(2, Math.round((Math.abs(v) / maxAbs) * 44));
  // Thousands of kroner, so twelve labels fit side by side on a phone.
  const tusen = (v) => String(Math.round(v / 100000)).replace("-", "−");
</script>

<Card title={"Resultat per måned " + tall.year}>
  <div class="flex gap-1 sm:gap-2">
    {#each tall.maaneder as v, i}
      <div class="flex-1 min-w-0 flex flex-col items-center" title={MND_NAVN[i] + ": " + kr(v)}>
        <div class="h-12 w-full flex items-end justify-center">
          {#if v > 0}
            <div class="w-full max-w-6 rounded-t bg-primary" style:height={hoyde(v) + "px"}></div>
          {/if}
        </div>
        <div class="h-px w-full bg-base-300"></div>
        {#if harUnderskudd}
          <div class="h-12 w-full flex items-start justify-center">
            {#if v < 0}
              <div class="w-full max-w-6 rounded-b bg-error" style:height={hoyde(v) + "px"}></div>
            {/if}
          </div>
        {/if}
        <span class="text-[10px] opacity-60 mt-1">{MND_NAVN[i]}</span>
        <span class="hidden sm:inline text-[10px] tabular-nums whitespace-nowrap {v < 0 ? 'text-error' : ''}">
          {v ? tusen(v) : ""}
        </span>
      </div>
    {/each}
  </div>
  <!-- Twelve value labels collide on a phone; the shape still reads. -->
  <p class="hidden sm:block text-xs opacity-60">Tall i tusen kroner.</p>
</Card>
