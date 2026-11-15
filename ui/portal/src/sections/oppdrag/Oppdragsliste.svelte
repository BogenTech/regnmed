<script>
  // Oppdragene selskapet har gitt, og forespørslene det har sendt. Å
  // avslutte setter valid_to, og siden den er EKSKLUSIV i
  // tilgangsoppslaget virker tilbakekallingen straks — derfor må /me
  // hentes på nytt etterpå.
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import { formatDate, formatTime } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import { OPPDRAGSTYPE } from "./byra.js";

  let { companyId, kanOppdrag, engagements, requests, onDone } = $props();

  let ventende = $derived(requests.filter((r) => r.status === "pending"));
  // Accepted requests are the engagements above; the other answers are
  // worth seeing for a while.
  let besvarte = $derived(
    requests.filter((r) => r.status === "declined" || r.status === "withdrawn").slice(0, 5),
  );

  async function avslutt(e) {
    const ok = await bekreft(
      e.firm + " mister tilgangen til selskapet med en gang. Oppdraget blir stående i historikken.",
      { tittel: "Avslutte oppdraget?", ok: "Avslutt oppdraget", farlig: true },
    );
    if (!ok) return;
    try {
      await post("/companies/" + companyId + "/engagements/" + e.engagement_id + "/end", {});
      toast("Oppdraget med " + e.firm + " er avsluttet", true);
      await loadMe(true); // tilgangen er endret nå, ikke ved neste innlogging
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekk(r) {
    try {
      await post(
        "/companies/" + companyId + "/engagement-requests/" + r.request_id + "/withdraw",
        {},
      );
      toast("Forespørselen til " + r.firm + " er trukket tilbake", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Oppdrag">
  {#if engagements.length}
    <table class="table table-sm">
      <thead>
        <tr><th>Byrå</th><th>Type</th><th>Fra</th><th><span class="sr-only">Status</span></th></tr>
      </thead>
      <tbody>
        {#each engagements as e (e.engagement_id)}
          <tr>
            <td>{e.firm}</td>
            <td>{OPPDRAGSTYPE[e.kind] || e.kind}</td>
            <td class="whitespace-nowrap">{formatDate(e.valid_from)}</td>
            <td class="text-right">
              {#if e.valid_to}
                <span class="opacity-60 text-sm">avsluttet {formatDate(e.valid_to)}</span>
              {:else if kanOppdrag}
                <button class="btn btn-xs btn-outline" onclick={() => avslutt(e)}>Avslutt</button>
              {:else}
                <span class="text-sm">aktivt</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if !ventende.length}
    <p class="opacity-70">
      Ingen oppdrag ennå.{#if kanOppdrag}
        Finn et autorisert byrå under og be om et oppdrag.{/if}
    </p>
  {/if}

  {#if ventende.length || besvarte.length}
    <h3 class="font-semibold text-sm mt-3 mb-1">Forespørsler</h3>
    <ul class="text-sm">
      {#each ventende as r (r.request_id)}
        <li class="flex flex-wrap items-center gap-x-2 gap-y-1 py-1">
          <span class="font-medium">{r.firm}</span>
          <span class="badge badge-sm badge-warning badge-soft">venter på svar</span>
          <span class="opacity-60">sendt {formatTime(r.created_at)}</span>
          {#if kanOppdrag}
            <button class="btn btn-xs btn-ghost ml-auto" onclick={() => trekk(r)}>Trekk tilbake</button>
          {/if}
        </li>
      {/each}
      {#each besvarte as r (r.request_id)}
        <li class="flex flex-wrap items-center gap-x-2 py-1 opacity-70">
          <span>{r.firm}</span>
          <span>
            {r.status === "declined" ? "takket nei" : "trukket tilbake"}
            {formatTime(r.decided_at)}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</Card>
