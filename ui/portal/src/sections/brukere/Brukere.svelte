<script>
  // Brukerne som egen seksjon (#63): medlemmer, invitasjoner og roller
  // samlet der spørsmålet stilles — «hvem har tilgang her, og som hva?»
  // Kortene er de samme som før (Tilgang/Roller, tidligere under
  // Oppdrag); nytt er Leverandørtilgang-kortet, som viser hva
  // plattformen (docs/auth.md §8) har gjort som angår DETTE selskapet.
  // Portalen skjuler, serveren nekter: kort uten tilgang forsvinner.
  //
  // AGENTENE STÅR HER (docs/agent.md §8.6). De er personer med en rolle
  // fra samme vokabular som menneskene, og rollen ER sikkerhetsgrensen
  // — da er «hvem har tilgang her, og som hva?» det samme spørsmålet
  // for en robot som for en ansatt, og det stilles på én side.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Tilgang from "../oppdrag/Tilgang.svelte";
  import Roller from "../oppdrag/Roller.svelte";
  import Integrasjoner from "../oppdrag/Integrasjoner.svelte";
  import PlattformInnsyn from "./PlattformInnsyn.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [tilgang, invitasjoner, roller, innsyn, integrasjoner, log] = await Promise.all([
      api("/companies/" + id + "/access").catch(() => null),
      api("/companies/" + id + "/invitations").catch(() => ({ invitasjoner: [] })),
      api("/companies/" + id + "/roles").catch(() => null),
      api("/companies/" + id + "/platform-access").catch(() => null),
      api("/companies/" + id + "/integrations").catch(() => null),
      api("/companies/" + id + "/integrations/log").catch(() => ({ kall: [] })),
    ]);
    data = {
      tilgang,
      invitasjoner: invitasjoner.invitasjoner,
      roller,
      innsyn,
      integrasjoner,
      integrasjonskall: log.kall,
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
  {#if data.tilgang}
    <Tilgang
      {companyId}
      tilgang={data.tilgang}
      invitasjoner={data.invitasjoner}
      roller={data.roller}
      onDone={reload}
    />
  {:else}
    <div class="alert alert-info">
      <span>Brukeradministrasjon krever medlemsadministrasjon (admin).</span>
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
      onDone={reload}
    />
  {/if}

  {#if data.roller}
    <Roller {companyId} roller={data.roller} onDone={reload} />
  {/if}

  {#if data.innsyn}
    <PlattformInnsyn innsyn={data.innsyn.innsyn} />
  {/if}
{/if}
