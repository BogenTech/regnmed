<script>
  // Oppgaver til en agent (#103). Bestillingen er uforanderlig når den
  // er gitt, og svarene er et innsettings-bart spor — derfor finnes det
  // ingen «rediger»-knapp her, og det er med vilje: en endret oppgave
  // ville gjort det umulig å se etterpå hva agenten faktisk ble bedt om.
  import { tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { formatDate, formatTime } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import Samtale from "./Samtale.svelte";

  // kanGi: INTEGRASJON_ADMIN — giving, withdrawing and replying to a task
  // all take it (regnmed-api::agent_task); without it the card is read-only.
  let { companyId, kanGi, integrasjoner, oppgaver, onDone } = $props();

  let melding = $state("");
  // Hvilken oppgave som er åpnet i samtalevisningen. Null = ingen.
  let apnet = $state(null);
  let mottaker = $state("");
  let frist = $state("");

  const aktive = $derived(integrasjoner.filter((i) => i.aktiv));
  const apne = $derived(oppgaver.filter((o) => !o.avsluttet));
  const lukkede = $derived(oppgaver.filter((o) => o.avsluttet));
  const valgt = $derived(oppgaver.find((o) => o.oppgave_id === apnet) || null);

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

  const LUKKEDE_VIST = 10;

  // Opening a conversation shows it, under the table it belongs to.
  async function visSamtale(o) {
    apnet = apnet === o.oppgave_id ? null : o.oppgave_id;
    if (!apnet) return;
    await tick();
    document.getElementById("samtale")?.scrollIntoView({ block: "nearest" });
  }

  async function gi(event) {
    event.preventDefault();
    if (!melding.trim()) return;
    try {
      await post("/companies/" + companyId + "/agent/oppgaver", {
        melding: melding.trim(),
        integration_id: mottaker || null,
        frist: frist || null,
      });
      toast("Oppgaven er gitt", true);
      melding = "";
      frist = "";
      onDone();
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

<Card title="Oppgaver til en agent">
  <p class="text-sm opacity-70 mb-2">
    Skriv hva agenten skal gjøre. Den henter køen når den arbeider, og svarer tilbake her — også
    når den trenger en avgjørelse av deg framfor å gjette. Et svar kan ikke trekkes tilbake, så
    sporet viser hele veien fra bestilling til resultat.
  </p>

  {#if apne.length}
    <table class="table table-sm mb-3">
      <thead>
        <tr>
          <th>Oppgave</th><th class="hidden sm:table-cell">Agent</th><th>Status</th>
          <th class="hidden md:table-cell">Siste svar</th><th><span class="sr-only">Handlinger</span></th>
        </tr>
      </thead>
      <tbody>
        {#each apne as o (o.oppgave_id)}
          <tr>
            <td>
              {o.melding}
              {#if o.frist}<br /><span class="text-xs opacity-60">frist {formatDate(o.frist)}</span>{/if}
            </td>
            <td class="hidden sm:table-cell">{o.agent || "enhver agent"}</td>
            <td>
              <span class="badge badge-sm {MERKE[o.status] || 'badge-ghost'}">
                {TEKST[o.status] || o.status}
              </span>
            </td>
            <td class="hidden md:table-cell text-xs opacity-70">
              {#if o.siste_notat}
                {o.siste_notat}<br />
                <span class="opacity-60">{o.siste_av}, {formatTime(o.siste_tidspunkt)}</span>
              {/if}
            </td>
            <td class="text-right">
              <button
                class="btn btn-xs {apnet === o.oppgave_id ? '' : 'btn-ghost'}"
                aria-expanded={apnet === o.oppgave_id}
                aria-controls="samtale"
                onclick={() => visSamtale(o)}
              >
                Samtale
              </button>
              {#if kanGi}
                <button class="btn btn-xs btn-ghost" onclick={() => avslutt(o)}>Trekk tilbake</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen åpne oppgaver.</p>
  {/if}

  {#if valgt}
    <div class="mb-3" id="samtale">
      <Samtale
        {companyId}
        kanSvare={kanGi}
        oppgave={valgt}
        agent={integrasjoner.find((i) => i.navn === valgt.agent) || null}
        onDone={onDone}
      />
    </div>
  {/if}

  {#if !kanGi}
    <!-- Nothing to add: reading the queue is all this person may do. -->
  {:else if !aktive.length}
    <p class="text-sm">
      Selskapet har ingen aktiv agent å gi oppgaver til.
      <a class="link" href={"#/c/" + companyId + "/admin/brukere"}>Legg til en agent under Brukere og roller</a>.
    </p>
  {:else}
  <form class="flex gap-2 items-end flex-wrap" onsubmit={gi}>
    <label class="form-control w-full sm:w-80">
      <span class="label-text text-xs">Oppgave</span>
      <input
        class="input input-sm w-full"
        placeholder="Importer regnskapet fra Conta og avstem mot årsregnskapet"
        bind:value={melding}
      />
    </label>
    <label class="form-control">
      <span class="label-text text-xs">Agent</span>
      <select class="select select-sm" bind:value={mottaker}>
        <option value="">enhver agent</option>
        {#each aktive as i (i.integration_id)}
          <option value={i.integration_id}>{i.navn}</option>
        {/each}
      </select>
    </label>
    <label class="form-control">
      <span class="label-text text-xs">Frist</span>
      <input class="input input-sm" type="date" bind:value={frist} />
    </label>
    <button class="btn btn-sm" disabled={!melding.trim()}>Gi oppgaven</button>
  </form>
  {/if}

  {#if lukkede.length}
    <h3 class="font-semibold text-sm mt-3 mb-1">Avsluttede</h3>
    {#each lukkede.slice(0, LUKKEDE_VIST) as o (o.oppgave_id)}
      <div class="text-xs opacity-70 py-0.5">
        <span class="badge badge-xs {MERKE[o.status] || 'badge-ghost'}">{TEKST[o.status] || o.status}</span>
        <button class="link" aria-controls="samtale" onclick={() => visSamtale(o)}>
          {o.melding}
        </button>
        {#if o.siste_notat}· {o.siste_notat}{/if}
      </div>
    {/each}
    {#if lukkede.length > LUKKEDE_VIST}
      <p class="text-xs opacity-60 mt-1">
        Viser de {LUKKEDE_VIST} siste av {lukkede.length} avsluttede oppgaver.
      </p>
    {/if}
  {/if}
</Card>
