<script>
  // Agentenes oppgaver (#103): SPORET, ikke samtalen. En agent får en
  // oppgave ved at noen skriver til den i Meldinger, som til en kollega,
  // og svarer i samme samtale (docs/meldinger.md) — her står hva hver
  // agent er bedt om og hvor det endte, for den som skal se etter.
  // Bestillingen er uforanderlig og svarene et innsettings-bart spor, så
  // det finnes ingen «rediger»-knapp, og det er med vilje.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { formatDate, formatTime } from "../../lib/format.js";
  import { renderMarkdown } from "../../lib/markdown.js";
  import Card from "../../components/Card.svelte";
  import Agentansikt from "../../components/Agentansikt.svelte";

  // kanGi: INTEGRASJON_ADMIN — withdrawing a task takes it
  // (regnmed-api::agent_task); without it the card is read-only.
  let { companyId, kanGi, integrasjoner, oppgaver, onDone } = $props();

  const aktive = $derived(integrasjoner.filter((i) => i.aktiv));
  const LUKKEDE_VIST = 10;
  const lukkede = $derived(oppgaver.filter((o) => o.avsluttet));
  // What is still in play first, then the newest of what is done.
  const rader = $derived([
    ...oppgaver.filter((o) => !o.avsluttet),
    ...lukkede.slice(0, LUKKEDE_VIST),
  ]);

  // Spørsmål venter på et menneske, så det er den statusen som skal
  // være lettest å se.
  const MERKE = {
    ny: "badge-ghost",
    pagar: "badge-info",
    sporsmal: "badge-warning",
    utfort: "badge-success",
    avvist: "badge-error",
  };
  const TEKST = {
    ny: "ny",
    pagar: "pågår",
    sporsmal: "spør",
    utfort: "utført",
    avvist: "avvist",
  };

  // The trail of the one task that is opened. Null = none / loading.
  let apnet = $state(null);
  let spor = $state(null);

  async function visSpor(o) {
    spor = null;
    apnet = apnet === o.oppgave_id ? null : o.oppgave_id;
    if (!apnet) return;
    const id = apnet;
    try {
      const svar = await api("/companies/" + companyId + "/agent/oppgaver/" + id);
      if (id === apnet) spor = svar.historikk;
    } catch (error) {
      toast(error.message, false);
    }
  }

  // The conversation is between the reader and the agent; opening it is
  // idempotent, so this is also how an existing one is found.
  async function skrivTil(i) {
    try {
      const opened = await post("/chat/conversations", {
        person_id: i.person_id,
        company_id: companyId,
      });
      location.hash = "#/c/" + companyId + "/meldinger/" + opened.conversation_id;
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avslutt(o) {
    const ok = await bekreft(
      "Agenten får beskjed om at oppgaven er trukket tilbake, og den kan ikke åpnes igjen. " +
        "En ny bestilling blir en ny oppgave.",
      { tittel: "Trekke tilbake oppgaven?", ok: "Trekk tilbake", farlig: true },
    );
    if (!ok) return;
    try {
      await post("/companies/" + companyId + "/agent/oppgaver/" + o.oppgave_id + "/svar", {
        status: "avvist",
        notat: "Trukket tilbake av oppdragsgiver",
      });
      toast("Oppgaven er trukket tilbake", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Agentenes oppgaver">
  <p class="text-sm opacity-70 mb-3">
    Du gir en agent en oppgave ved å skrive til den i Meldinger, som til en kollega. Svar og
    spørsmål kommer i samme samtale. Her står sporet: hva hver agent er bedt om, og hvor det endte.
  </p>

  {#if aktive.length}
    <div class="flex flex-wrap gap-2 mb-4">
      {#each aktive as i (i.grant_id)}
        <button class="btn btn-sm gap-2" onclick={() => skrivTil(i)}>
          <Agentansikt navn={i.navn} farge={i.avatar_farge} ikon={i.avatar_ikon} storrelse="w-6" />
          Skriv til {i.navn}
        </button>
      {/each}
    </div>
  {:else}
    <p class="text-sm mb-3">
      Selskapet har ingen aktiv agent.
      <a class="link" href={"#/c/" + companyId + "/admin/brukere"}>Legg til en under Brukere og roller</a>.
    </p>
  {/if}

  {#if rader.length}
    <div class="overflow-x-auto">
      <table class="table table-sm">
        <thead>
          <tr>
            <th>Gitt</th>
            <th class="hidden sm:table-cell">Agent</th>
            <th>Bestilling</th>
            <th>Status</th>
            <th class="hidden lg:table-cell">Siste svar</th>
            <th><span class="sr-only">Handlinger</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rader as o (o.oppgave_id)}
            <tr class="align-top">
              <td class="whitespace-nowrap text-xs">
                {formatTime(o.opprettet)}<br />
                <span class="opacity-60">{o.opprettet_av}</span>
              </td>
              <td class="hidden sm:table-cell">{o.agent || "enhver agent"}</td>
              <td class="max-w-md">
                <!-- From a conversation: what the person wrote, not the
                     order wrapped around it (that is in the trail). -->
                <span class="line-clamp-2">{o.fra_melding || o.melding}</span>
                {#if o.frist}<span class="text-xs opacity-60">frist {formatDate(o.frist)}</span>{/if}
              </td>
              <td>
                <span class="badge badge-sm {MERKE[o.status] || 'badge-ghost'}">
                  {TEKST[o.status] || o.status}
                </span>
              </td>
              <td class="hidden lg:table-cell max-w-sm text-xs opacity-70">
                {#if o.siste_notat}
                  <span class="line-clamp-2">{o.siste_notat}</span>
                  <span class="opacity-60">{o.siste_av}, {formatTime(o.siste_tidspunkt)}</span>
                {/if}
              </td>
              <td class="text-right whitespace-nowrap">
                <button
                  class="btn btn-xs {apnet === o.oppgave_id ? '' : 'btn-ghost'}"
                  aria-expanded={apnet === o.oppgave_id}
                  onclick={() => visSpor(o)}
                >
                  Spor
                </button>
                {#if kanGi && !o.avsluttet}
                  <button class="btn btn-xs btn-ghost" onclick={() => avslutt(o)}>Trekk tilbake</button>
                {/if}
              </td>
            </tr>
            {#if apnet === o.oppgave_id}
              <tr>
                <td colspan="6" class="bg-base-200">
                  {#if !spor}
                    <span class="loading loading-spinner loading-sm"></span>
                  {:else}
                    <ol class="flex flex-col gap-2 text-sm">
                      {#each spor as h, n (n)}
                        <li>
                          <span class="text-xs opacity-60">{formatTime(h.tidspunkt)} · {h.av}</span>
                          <span class="badge badge-xs {MERKE[h.status] || 'badge-ghost'}">
                            {TEKST[h.status] || h.status}
                          </span>
                          {#if h.notat}
                            <div class="break-words [&_p]:my-1 [&_ul]:list-disc [&_ul]:ml-5 [&_ol]:list-decimal [&_ol]:ml-5">
                              {@html renderMarkdown(h.notat)}
                            </div>
                          {/if}
                        </li>
                      {:else}
                        <li class="opacity-70">Agenten har ikke svart ennå.</li>
                      {/each}
                    </ol>
                    <details class="mt-2 text-xs">
                      <summary class="cursor-pointer opacity-70">Bestillingen slik agenten fikk den</summary>
                      <p class="whitespace-pre-wrap mt-1">{o.melding}</p>
                    </details>
                  {/if}
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
    {#if lukkede.length > LUKKEDE_VIST}
      <p class="text-xs opacity-60 mt-1">
        Viser de {LUKKEDE_VIST} siste av {lukkede.length} avsluttede oppgaver.
      </p>
    {/if}
  {:else}
    <p class="text-sm opacity-70">Ingen oppgaver ennå.</p>
  {/if}
</Card>
