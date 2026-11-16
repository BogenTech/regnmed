<script>
  // Å godta en forespørsel oppretter oppdraget i én transaksjon —
  // tilgangen er levende med det samme, uten ny innlogging, så /me må
  // hentes på nytt.
  import { post } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";
  import { oppdragstype } from "./byra.js";

  // `admin`: engagement decisions commit the firm to a client, so they
  // are admin-only (#78). Members see the queue; the buttons would 404
  // for them, so they are not offered.
  let { firmId, pending, admin, onDone } = $props();

  let opptatt = $state(false);

  async function avgjor(r, accept) {
    const melding = accept
      ? "Godta oppdraget for " + r.company + "? Alle i byrået får tilgang til selskapet med én gang."
      : "Avslå forespørselen fra " + r.company + "? Selskapet ser at den er avslått.";
    if (
      !(await bekreft(melding, {
        tittel: accept ? "Godta oppdrag" : "Avslå forespørsel",
        ok: accept ? "Godta" : "Avslå",
        farlig: !accept,
      }))
    )
      return;
    opptatt = true;
    try {
      await post("/firms/" + firmId + "/requests/" + r.request_id + "/decision", { accept });
      toast(accept ? r.company + " er lagt til som klient" : "Forespørselen er avslått", true);
      if (accept) await loadMe(true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      opptatt = false;
      onDone();
    }
  }
</script>

<Card title="Innkommende forespørsler">
  {#if pending.length}
    <ul class="divide-y divide-base-200" aria-label="Innkommende forespørsler">
      {#each pending as r (r.request_id)}
        <li class="py-2 flex flex-wrap items-center gap-x-3 gap-y-2">
          <div class="min-w-0 flex-1 basis-56">
            <div class="font-medium">{r.company}</div>
            <div class="text-xs opacity-70">
              Org.nr. {r.orgnr}. {oppdragstype(r.kind)}. Sendt {formatDate(r.created_at)}.
            </div>
            {#if r.message}<p class="text-sm mt-1 whitespace-pre-line">{r.message}</p>{/if}
          </div>
          <div class="flex gap-2 ml-auto">
            {#if admin}
              <button class="btn btn-sm btn-primary" disabled={opptatt} onclick={() => avgjor(r, true)}>
                Godta
              </button>
              <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={() => avgjor(r, false)}>
                Avslå
              </button>
            {:else}
              <span class="opacity-70 text-xs">Avgjøres av byråets administrator.</span>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70">
      Ingen forespørsler venter. Selskaper finner byrået i katalogen og ber om oppdrag derfra.
    </p>
  {/if}
</Card>
