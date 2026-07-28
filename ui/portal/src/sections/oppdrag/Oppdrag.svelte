<script>
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Oppdragsliste from "./Oppdragsliste.svelte";
  import Byraliste from "./Byraliste.svelte";
  import Tilgang from "./Tilgang.svelte";
  import Roller from "./Roller.svelte";
  import Integrasjoner from "./Integrasjoner.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [engagements, firms, integrasjoner, log, tilgang, invitasjoner, roller] =
      await Promise.all([
        api("/companies/" + id + "/engagements"),
        api("/directory/firms"),
        api("/companies/" + id + "/integrations").catch(() => ({ integrasjoner: [] })),
        api("/companies/" + id + "/integrations/log").catch(() => ({ kall: [] })),
        // Tilgangsstyring krever MEDLEM_ADMIN. En bokfører får 403 her, og
        // da skal kortet være borte — ikke vise en knapp som ikke virker.
        api("/companies/" + id + "/access").catch(() => null),
        api("/companies/" + id + "/invitations").catch(() => ({ invitasjoner: [] })),
        api("/companies/" + id + "/roles").catch(() => null),
      ]);
    data = {
      engagements: engagements.engagements,
      firms: firms.firms,
      integrasjoner: integrasjoner.integrasjoner,
      integrasjonskall: log.kall,
      tilgang,
      invitasjoner: invitasjoner.invitasjoner,
      roller,
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

  {#if data.tilgang}
    <Tilgang
      {companyId}
      tilgang={data.tilgang}
      invitasjoner={data.invitasjoner}
      roller={data.roller}
      onDone={reload}
    />
  {/if}

  {#if data.roller}
    <Roller {companyId} roller={data.roller} onDone={reload} />
  {/if}

  <Integrasjoner
    {companyId}
    integrasjoner={data.integrasjoner}
    kall={data.integrasjonskall}
    onDone={reload}
  />
{/if}
