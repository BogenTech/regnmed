<script>
  // Lønn (#46): ansattregister + lønnskjøring. A-meldingen sendes
  // herfra bare der innsending er satt opp (docs/lonn.md).
  //
  // Invitasjoner og medlemslisten er valgfrie kilder for
  // koblingskortet (ansatt ↔ portalbruker): de krever MEDLEM_ADMIN, og
  // en lønnsansvarlig uten den retten mister bare de knappene —
  // serveren nekter, portalen skjuler.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import LoadError from "../../components/LoadError.svelte";
  import Ansatte from "./Ansatte.svelte";
  import KjorLonn from "./KjorLonn.svelte";
  import Lonnskjoring from "./Lonnskjoring.svelte";
  import Skattekort from "./Skattekort.svelte";

  let { companyId } = $props();

  let data = $state(null);
  let feil = $state(null);

  async function load(id) {
    const [employees, payroll, invitasjoner, medlemmer] = await Promise.all([
      api("/companies/" + id + "/employees"),
      api("/companies/" + id + "/payroll"),
      api("/companies/" + id + "/invitations").catch(() => null),
      api("/companies/" + id + "/access").catch(() => null),
    ]);
    data = {
      ansatte: employees.ansatte,
      koder: employees.koder,
      kjoringer: payroll.kjoringer,
      invitasjoner: invitasjoner ? invitasjoner.invitasjoner : null,
      medlemmer: medlemmer ? medlemmer.medlemmer : null,
    };
  }

  // A failed load says so with a retry, never a spinner that spins on.
  function reload() {
    feil = null;
    return load(companyId).catch((error) =>
      data ? toast(error.message, false) : (feil = error.message),
    );
  }

  $effect(() => {
    companyId;
    data = null;
    reload();
  });
</script>

{#if feil}
  <LoadError error={feil} retry={reload} />
{:else if !data}
  <span class="loading loading-spinner loading-lg" aria-label="Laster lønn"></span>
{:else}
  <!-- The monthly job first: run, then check the a-melding; the
       register and tax cards are set up once and visited rarely. -->
  <KjorLonn {companyId} ansatte={data.ansatte} kjoringer={data.kjoringer} onDone={reload} />
  <Lonnskjoring {companyId} ansatte={data.ansatte} kjoringer={data.kjoringer} />
  <Ansatte
    {companyId}
    ansatte={data.ansatte}
    koder={data.koder}
    invitasjoner={data.invitasjoner}
    medlemmer={data.medlemmer}
    onDone={reload}
  />
  <Skattekort {companyId} onDone={reload} />
{/if}
