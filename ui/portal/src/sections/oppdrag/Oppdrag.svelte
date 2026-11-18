<script>
  // Oppdrag: det som følger AVTALER — oppdrag til byråer, og jobbene
  // som gis til en agent. Agentene SELV står i Brukere og roller, for
  // en agent er en bruker (docs/agent.md §8.6); det var feil sted å
  // sette og endre rollen som er sikkerhetsgrensen.
  //
  // Three parts, loaded separately: a failure in one does not stop the
  // others, and an action reloads only the part it changed — not the
  // whole firm directory because an agent was given a task.
  import { api } from "../../lib/api.js";
  import { harRett } from "../../lib/me.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import Oppdragsliste from "./Oppdragsliste.svelte";
  import Byraliste from "./Byraliste.svelte";
  import Agentoppgaver from "./Agentoppgaver.svelte";
  import { hentVarsler } from "../../lib/varsler.svelte.js";

  let { companyId } = $props();

  let kanOppdrag = $derived(harRett(companyId, "OPPDRAG_ADMIN"));
  let serAgenter = $derived(harRett(companyId, "INTEGRASJON_LES"));

  // Each part: null = loading, { error } = failed, otherwise the data.
  let oppdrag = $state(null);
  let firms = $state(null);
  let agent = $state(null);

  // A slow answer for a company the person has since left is dropped.
  async function hent(id, sett, last) {
    try {
      const data = await last(id);
      if (id === companyId) sett(data);
    } catch (error) {
      if (id === companyId) sett({ error: error.message });
    }
  }

  const lastOppdrag = (id = companyId) =>
    hent(
      id,
      (v) => (oppdrag = v),
      async (id) => {
        const [e, r] = await Promise.all([
          api("/companies/" + id + "/engagements"),
          api("/companies/" + id + "/engagement-requests"),
        ]);
        return { engagements: e.engagements, requests: r.requests };
      },
    );

  const lastByraer = (id = companyId) =>
    hent(
      id,
      (v) => (firms = v),
      async () => ({ firms: (await api("/directory/firms")).firms }),
    );

  const lastAgent = (id = companyId) =>
    hent(
      id,
      (v) => (agent = v),
      async (id) => {
        const [i, o] = await Promise.all([
          // Agentoppgaver trenger listen for å vite hvem en oppgave kan
          // stiles til; selve agentkortet bor i Brukere og roller.
          api("/companies/" + id + "/integrations"),
          api("/companies/" + id + "/agent/oppgaver?apne=false"),
        ]);
        return { integrasjoner: i.integrasjoner, oppgaver: o.oppgaver };
      },
    );

  $effect(() => {
    const id = companyId;
    oppdrag = agent = null;
    lastOppdrag(id);
    if (serAgenter) lastAgent(id);
  });

  // When this card's list changes -- an answer sent, or the agent asking --
  // the bell and the menu marker follow at once instead of within a minute.
  const sporsmalHer = $derived(
    agent?.oppgaver?.filter((o) => o.status === "sporsmal").length ?? null,
  );
  $effect(() => {
    if (sporsmalHer !== null) hentVarsler();
  });

  // An agent answers in the background, and a task given from this card
  // has no chat thread to announce it -- so while one is being worked on
  // (`ny`/`pagar`), the list asks again. `sporsmal` waits for a human and
  // the rest are final, so nothing polls once the agent is done. A failed
  // poll keeps what is shown: the next one, or a reload, says more.
  const arbeider = $derived(
    !!agent?.oppgaver?.some((o) => o.status === "ny" || o.status === "pagar"),
  );
  $effect(() => {
    if (!arbeider) return;
    const id = companyId;
    const timer = setInterval(async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const o = await api("/companies/" + id + "/agent/oppgaver?apne=false");
        if (id === companyId && agent && !agent.error) agent = { ...agent, oppgaver: o.oppgaver };
      } catch {
        // keep the list as it is
      }
    }, 4000);
    return () => clearInterval(timer);
  });

  // The directory is the same for every company, and only for whoever
  // may ask a firm for an oppdrag.
  $effect(() => {
    if (kanOppdrag && !firms) lastByraer();
  });
</script>

{#snippet laster(del, retry)}
  {#if !del}
    <div class="mb-4"><span class="loading loading-spinner loading-md" aria-label="Laster"></span></div>
  {:else}
    <div class="mb-4"><LoadError error={del.error} {retry} /></div>
  {/if}
{/snippet}

{#if oppdrag && !oppdrag.error}
  <Oppdragsliste
    {companyId}
    {kanOppdrag}
    engagements={oppdrag.engagements}
    requests={oppdrag.requests}
    onDone={() => lastOppdrag()}
  />

  {#if kanOppdrag}
    {#if firms && !firms.error}
      <Byraliste
        {companyId}
        firms={firms.firms}
        engagements={oppdrag.engagements}
        requests={oppdrag.requests}
        onDone={() => lastOppdrag()}
      />
    {:else}
      <!-- Clearing it is enough: the effect above fetches again. -->
      {@render laster(firms, () => (firms = null))}
    {/if}
  {/if}
{:else}
  {@render laster(oppdrag, () => ((oppdrag = null), lastOppdrag()))}
{/if}

{#if serAgenter}
  {#if agent && !agent.error}
    <Agentoppgaver
      {companyId}
      kanGi={harRett(companyId, "INTEGRASJON_ADMIN")}
      integrasjoner={agent.integrasjoner}
      oppgaver={agent.oppgaver}
      onDone={() => lastAgent()}
    />
  {:else}
    {@render laster(agent, () => ((agent = null), lastAgent()))}
  {/if}
{/if}
