<script>
  // The firm's clients. An active client links straight to its books —
  // the reason an accountant opens this page. Ended engagements are
  // history: listed apart, without a link (the access ended with them).
  import { formatDate, today } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import { oppdragstype, delKlienter } from "./byra.js";

  let { clients } = $props();

  let delt = $derived(delKlienter(clients, today()));
</script>

<Card title="Klienter">
  {#if delt.aktive.length}
    <ul class="divide-y divide-base-200" aria-label="Aktive klienter">
      {#each delt.aktive as e (e.engagement_id)}
        <li>
          {#if e.valid_from > today()}
            <!-- Not started: the access does not exist yet, so no link. -->
            <div class="flex flex-wrap items-baseline gap-x-3 py-2 px-1">
              <span class="font-medium">{e.company}</span>
              <span class="text-xs opacity-70">
                {oppdragstype(e.kind)} fra {formatDate(e.valid_from)}
              </span>
            </div>
          {:else}
            <a
              class="flex flex-wrap items-baseline gap-x-3 py-2 px-1 rounded hover:bg-base-200 focus-visible:bg-base-200"
              href={"#/c/" + e.company_id + "/oversikt"}
            >
              <span class="font-medium link link-hover">{e.company}</span>
              <span class="text-xs opacity-70">
                {oppdragstype(e.kind)} siden {formatDate(e.valid_from)}{e.valid_to
                  ? ", avsluttes " + formatDate(e.valid_to)
                  : ""}
              </span>
            </a>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70">
      Ingen aktive klienter ennå. Når byrået godtar en forespørsel, kommer selskapet hit.
    </p>
  {/if}
  {#if delt.avsluttede.length}
    <details class="mt-3">
      <summary class="text-sm cursor-pointer opacity-70">
        Avsluttede oppdrag ({delt.avsluttede.length})
      </summary>
      <ul class="text-sm mt-1" aria-label="Avsluttede oppdrag">
        {#each delt.avsluttede as e (e.engagement_id)}
          <li class="py-1">
            {e.company}
            <span class="text-xs opacity-70">
              {oppdragstype(e.kind)}, {formatDate(e.valid_from)} til {formatDate(e.valid_to)}
            </span>
          </li>
        {/each}
      </ul>
    </details>
  {/if}
</Card>
