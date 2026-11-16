<script>
  // The firm's clients. An active client links straight to its books —
  // the reason an accountant opens this page. Ended engagements are
  // history: listed apart, without a link (the access ended with them).
  //
  // Each client also has the byrå's CONTACT: who the client's people can
  // write to in Meldinger (docs/meldinger.md). Until an admin chooses,
  // it is the byrå's admins.
  import { formatDate, today } from "../../lib/format.js";
  import { send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import { oppdragstype, delKlienter } from "./byra.js";

  let { firmId, clients, contacts = {}, admin = false, medlemmer = [], onDone } = $props();

  let delt = $derived(delKlienter(clients, today()));
  const aktive = $derived(medlemmer.filter((m) => m.aktiv));

  function kontakt(e) {
    const valgt = contacts[e.engagement_id] || [];
    if (!valgt.length) return "byråets administratorer";
    const navn = valgt.map((id) => medlemmer.find((m) => m.person_id === id)?.navn).filter(Boolean);
    return navn.length ? navn.join(", ") : valgt.length + " valgt";
  }

  async function velg(e, personId, på) {
    const valgt = new Set(contacts[e.engagement_id] || []);
    if (på) valgt.add(personId);
    else valgt.delete(personId);
    try {
      await send("PUT", "/firms/" + firmId + "/engagements/" + e.engagement_id + "/contacts", {
        person_ids: [...valgt],
      });
      onDone?.();
    } catch (error) {
      toast(error.message, false);
    }
  }
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
            <div class="flex flex-wrap items-center gap-x-2 px-1 pb-2 text-xs opacity-80">
              <span>Kontakt for kunden: {kontakt(e)}</span>
              {#if admin}
                <!-- Inline, not a floating dropdown: the card clips
                     overflow, and on a phone inline is what fits. -->
                <details class="w-full sm:w-auto">
                  <summary class="link">endre</summary>
                  <ul class="menu menu-sm bg-base-200 rounded-box w-60 mt-1">
                    {#each aktive as m (m.person_id)}
                      <li>
                        <label class="flex gap-2">
                          <input
                            type="checkbox"
                            class="checkbox checkbox-xs"
                            checked={(contacts[e.engagement_id] || []).includes(m.person_id)}
                            onchange={(ev) => velg(e, m.person_id, ev.currentTarget.checked)}
                          />
                          {m.navn}
                        </label>
                      </li>
                    {/each}
                  </ul>
                </details>
              {/if}
            </div>
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
