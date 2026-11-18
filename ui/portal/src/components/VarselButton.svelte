<script>
  // Varslingssenteret ved siden av profilen: det som venter på deg.
  import { varsler } from "../lib/varsler.svelte.js";
  import { prefs } from "../lib/prefs.svelte.js";
  import { formatTime } from "../lib/format.js";
  import Ikon from "./Ikon.svelte";

  const antall = $derived(varsler.liste.length);
</script>

<div class="dropdown dropdown-end">
  <div
    tabindex="0"
    role="button"
    class="btn btn-ghost btn-circle indicator"
    aria-label={antall > 0 ? "Varsler, " + antall + " venter på deg" : "Varsler"}
    title="Varsler"
  >
    {#if antall > 0}
      <span class="indicator-item badge badge-xs badge-warning">{antall}</span>
    {/if}
    <Ikon navn="varsler" stil={prefs.ikonstil === "ingen" ? "linje" : null} />
  </div>
  <div
    tabindex="-1"
    class="dropdown-content bg-base-200 rounded-box z-20 mt-2 w-80 max-w-[90vw] p-2 shadow-lg"
  >
    <h2 class="px-2 py-1 text-sm font-semibold">Varsler</h2>
    {#if antall === 0}
      <p class="px-2 pb-2 text-sm opacity-70">Ingenting venter på deg.</p>
    {:else}
      <ul class="menu menu-sm w-full p-0">
        {#each varsler.liste as v (v.oppgave_id)}
          <li>
            <a href={"#/c/" + v.company_id + "/admin/oppdrag"} class="flex flex-col items-start gap-0.5">
              <span class="font-semibold">
                {v.agent || "En agent"} spør · {v.selskap}
              </span>
              <span class="line-clamp-2 text-xs opacity-80">{v.sporsmal || v.melding}</span>
              {#if v.tidspunkt}
                <span class="text-xs opacity-60">{formatTime(v.tidspunkt)}</span>
              {/if}
            </a>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>
