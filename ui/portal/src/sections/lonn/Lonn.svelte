<script>
  // Lønn (#46): ansattregister + lønnskjøring. Kjøringen er
  // regnskapsføring, ikke rapportering — a-meldingen leveres ikke
  // herfra (docs/lonn.md).
  //
  // Invitasjoner og medlemslisten er valgfrie kilder for
  // koblingskortet (ansatt ↔ portalbruker): de krever MEDLEM_ADMIN, og
  // en lønnsansvarlig uten den retten mister bare de knappene —
  // serveren nekter, portalen skjuler.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Ansatte from "./Ansatte.svelte";
  import Lonnskjoring from "./Lonnskjoring.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [employees, payroll, invitasjoner, medlemmer] = await Promise.all([
      api("/companies/" + id + "/employees"),
      api("/companies/" + id + "/payroll"),
      api("/companies/" + id + "/invitations").catch(() => null),
      api("/companies/" + id + "/access").catch(() => null),
    ]);
    data = {
      ansatte: employees.ansatte,
      kjoringer: payroll.kjoringer,
      invitasjoner: invitasjoner ? invitasjoner.invitasjoner : null,
      medlemmer: medlemmer ? medlemmer.medlemmer : null,
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
  <Ansatte
    {companyId}
    ansatte={data.ansatte}
    invitasjoner={data.invitasjoner}
    medlemmer={data.medlemmer}
    onDone={reload}
  />
  <Lonnskjoring {companyId} ansatte={data.ansatte} kjoringer={data.kjoringer} onDone={reload} />
{/if}
