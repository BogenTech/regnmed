<script>
  // Oppdrag og maskintilgang. Medlemmer/invitasjoner/roller flyttet til
  // sin egen Brukere-seksjon (#63) — her bor det som følger AVTALER:
  // oppdrag til byråer og integrasjonenes tilganger.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Oppdragsliste from "./Oppdragsliste.svelte";
  import Byraliste from "./Byraliste.svelte";
  import Integrasjoner from "./Integrasjoner.svelte";
  import Agentoppgaver from "./Agentoppgaver.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [engagements, firms, integrasjoner, log, roller, oppgaver] = await Promise.all([
      api("/companies/" + id + "/engagements"),
      api("/directory/firms"),
      api("/companies/" + id + "/integrations").catch(() => ({ integrasjoner: [] })),
      api("/companies/" + id + "/integrations/log").catch(() => ({ kall: [] })),
      // Rollevokabularet: integrasjoner får nå en ekte rolle (#101).
      // Krever MEDLEM_ADMIN, så en rolle med bare IntegrasjonAdmin
      // faller tilbake på de innebygde navnene velgeren kjenner.
      api("/companies/" + id + "/roles").catch(() => null),
      api("/companies/" + id + "/agent/oppgaver?apne=false").catch(() => ({ oppgaver: [] })),
    ]);
    data = {
      engagements: engagements.engagements,
      firms: firms.firms,
      integrasjoner: integrasjoner.integrasjoner,
      farger: integrasjoner.farger || [],
      llmTilgjengelig: !!integrasjoner.llm_tilgjengelig,
      leverandorer: integrasjoner.leverandorer || [],
      integrasjonskall: log.kall,
      roller,
      oppgaver: oppgaver.oppgaver,
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Oppdragsliste {companyId} engagements={data.engagements} onDone={reload} />

  <Byraliste {companyId} firms={data.firms} engagements={data.engagements} onDone={reload} />

  <Integrasjoner
    {companyId}
    integrasjoner={data.integrasjoner}
    kall={data.integrasjonskall}
    roller={data.roller}
    farger={data.farger}
    llmTilgjengelig={data.llmTilgjengelig}
    leverandorer={data.leverandorer}
    onDone={reload}
  />

  <Agentoppgaver
    {companyId}
    integrasjoner={data.integrasjoner}
    oppgaver={data.oppgaver}
    onDone={reload}
  />
{/if}
