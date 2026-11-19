<script>
  // Varslingssenteret ved siden av profilen (docs/meldinger.md §7.2): det
  // som venter på deg. I dag er det uleste meldinger — et spørsmål fra en
  // agent er en melding fra agenten, som fra en kollega. Å lese samtalen
  // fjerner varselet; `type` gir rom for andre ting som trenger et blikk.
  import { chat, markAllRead } from "../lib/chat.svelte.js";
  import { route } from "../lib/router.svelte.js";
  import { prefs } from "../lib/prefs.svelte.js";
  import { formatTime } from "../lib/format.js";
  import Ikon from "./Ikon.svelte";

  const antall = $derived(chat.unread);
  // Inside a company the messenger keeps the company's menu around it.
  const meldinger = $derived(
    route.view === "company" ? "#/c/" + route.companyId + "/meldinger/" : "#/meldinger/",
  );

  // The dropdown is open while something in it has focus; a choice made
  // closes it.
  const lukk = () => document.activeElement?.blur();
</script>

<div class="dropdown dropdown-end">
  <div
    tabindex="0"
    role="button"
    class="btn btn-ghost btn-circle indicator"
    aria-label={antall > 0 ? "Varsler, " + antall + " uleste" : "Varsler"}
    title="Varsler"
  >
    {#if antall > 0}
      <span class="indicator-item badge badge-xs badge-primary">{antall}</span>
    {/if}
    <Ikon navn="varsler" stil={prefs.ikonstil === "ingen" ? "linje" : null} />
  </div>
  <div
    tabindex="-1"
    class="dropdown-content bg-base-200 rounded-box z-20 mt-2 w-80 max-w-[90vw] p-2 shadow-lg"
  >
    <div class="flex items-center justify-between px-2 py-1">
      <h2 class="text-sm font-semibold">Varsler</h2>
      {#if chat.varsler.length}
        <button
          class="btn btn-ghost btn-xs"
          onclick={() => {
            lukk();
            markAllRead();
          }}
        >
          Marker alle som lest
        </button>
      {/if}
    </div>
    {#if chat.varsler.length === 0}
      <p class="px-2 pb-2 text-sm opacity-70">Ingenting venter på deg.</p>
    {:else}
      <ul class="menu menu-sm w-full p-0">
        {#each chat.varsler as v (v.conversation_id)}
          <li>
            <a
              href={meldinger + v.conversation_id}
              class="flex flex-col items-start gap-0.5"
              onclick={lukk}
            >
              <span class="flex w-full items-center gap-2 font-semibold">
                <span class="truncate">{v.fra}</span>
                {#if v.uleste > 1}
                  <span class="badge badge-xs badge-primary ml-auto">{v.uleste}</span>
                {/if}
              </span>
              <span class="line-clamp-2 text-xs opacity-80">{v.tekst}</span>
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
