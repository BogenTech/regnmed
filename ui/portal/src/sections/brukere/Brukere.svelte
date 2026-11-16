<script>
  // Brukerne som egen seksjon (#63): medlemmer, invitasjoner og roller
  // samlet der spørsmålet stilles — «hvem har tilgang her, og som hva?»
  // Leverandørtilgang-kortet viser hva plattformen (docs/auth.md §8) har
  // gjort som angår DETTE selskapet.
  //
  // AGENTENE STÅR HER (docs/agent.md §8.6). De er personer med en rolle
  // fra samme vokabular som menneskene, og rollen ER sikkerhetsgrensen
  // — da er «hvem har tilgang her, og som hva?» det samme spørsmålet
  // for en robot som for en ansatt, og det stilles på én side.
  //
  // Only what /me says the viewer may read is fetched, so a refusal never
  // looks like an outage and an outage never looks like a refusal.
  import { api } from "../../lib/api.js";
  import { harRett } from "../../lib/me.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import Tilgang from "../oppdrag/Tilgang.svelte";
  import Roller from "../oppdrag/Roller.svelte";
  import Integrasjoner from "../oppdrag/Integrasjoner.svelte";
  import PlattformInnsyn from "./PlattformInnsyn.svelte";

  let { companyId } = $props();

  let data = $state(null);
  let feil = $state(null);

  async function load() {
    const id = companyId;
    const admin = harRett(id, "MEDLEM_ADMIN");
    const integ = harRett(id, "INTEGRASJON_LES");
    const hent = (ok, sti) => (ok ? api("/companies/" + id + sti) : Promise.resolve(null));
    try {
      const [tilgang, invitasjoner, roller, innsyn, integrasjoner, log] = await Promise.all([
        hent(admin, "/access"),
        hent(admin, "/invitations"),
        hent(admin, "/roles"),
        hent(admin, "/platform-access"),
        hent(integ, "/integrations"),
        hent(integ, "/integrations/log"),
      ]);
      if (id !== companyId) return;
      feil = null;
      data = {
        tilgang,
        invitasjoner: invitasjoner?.invitasjoner || [],
        roller,
        innsyn,
        integrasjoner,
        integrasjonskall: log?.kall || [],
      };
    } catch (error) {
      if (id === companyId) feil = error.message;
    }
  }

  $effect(() => {
    void companyId;
    data = null;
    feil = null;
    load();
  });
</script>

{#if feil}
  <LoadError error={feil} retry={load} />
{:else if !data}
  <span class="loading loading-spinner loading-lg" aria-label="Laster brukerne"></span>
{:else}
  {#if data.tilgang}
    <Tilgang
      {companyId}
      tilgang={data.tilgang}
      invitasjoner={data.invitasjoner}
      roller={data.roller}
      onDone={load}
    />
  {:else}
    <div class="alert alert-info mb-4">
      <span>Bare en administrator kan se og endre hvem som har tilgang til selskapet.</span>
    </div>
  {/if}

  {#if data.integrasjoner}
    <Integrasjoner
      {companyId}
      integrasjoner={data.integrasjoner.integrasjoner}
      kall={data.integrasjonskall}
      roller={data.roller}
      farger={data.integrasjoner.farger || []}
      llmTilgjengelig={!!data.integrasjoner.llm_tilgjengelig}
      leverandorer={data.integrasjoner.leverandorer || []}
      kanOpprette={!!data.integrasjoner.kan_opprette}
      onDone={load}
    />
  {/if}

  {#if data.roller}
    <Roller {companyId} roller={data.roller} onDone={load} />
  {/if}

  {#if data.innsyn}
    <PlattformInnsyn innsyn={data.innsyn.innsyn} />
  {/if}
{/if}
