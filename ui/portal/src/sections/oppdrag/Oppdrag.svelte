<script>
  // Oppdrag og maskintilgang. Medlemmer/invitasjoner/roller flyttet til
  // sin egen Brukere-seksjon (#63) — her bor det som følger AVTALER:
  // oppdrag til byråer og integrasjonenes tilganger.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Oppdragsliste from "./Oppdragsliste.svelte";
  import Byraliste from "./Byraliste.svelte";
  import Integrasjoner from "./Integrasjoner.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [engagements, firms, integrasjoner, log] = await Promise.all([
      api("/companies/" + id + "/engagements"),
      api("/directory/firms"),
      api("/companies/" + id + "/integrations").catch(() => ({ integrasjoner: [] })),
      api("/companies/" + id + "/integrations/log").catch(() => ({ kall: [] })),
    ]);
    data = {
      engagements: engagements.engagements,
      firms: firms.firms,
      integrasjoner: integrasjoner.integrasjoner,
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
  <Oppdragsliste {companyId} engagements={data.engagements} onDone={reload} />

  <Byraliste {companyId} firms={data.firms} engagements={data.engagements} onDone={reload} />

  <Integrasjoner
    {companyId}
    integrasjoner={data.integrasjoner}
    kall={data.integrasjonskall}
    onDone={reload}
  />
{/if}
