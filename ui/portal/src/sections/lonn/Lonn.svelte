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
  import Trekkpalegg from "./Trekkpalegg.svelte";

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
    // A late answer for the previous company must not seed this one's form.
    if (id !== companyId) return;
    data = {
      ansatte: employees.ansatte,
      koder: employees.koder,
      canFetchCard: employees.can_fetch_tax_card,
      kjoringer: payroll.kjoringer,
      ameldingMaaneder: payroll.amelding_maaneder,
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
  <Lonnskjoring
    {companyId}
    ansatte={data.ansatte}
    kjoringer={data.kjoringer}
    maaneder={data.ameldingMaaneder}
    onMaaneder={(m) => (data.ameldingMaaneder = m)}
  />
  <Ansatte
    {companyId}
    ansatte={data.ansatte}
    koder={data.koder}
    canFetchCard={data.canFetchCard}
    invitasjoner={data.invitasjoner}
    medlemmer={data.medlemmer}
    onDone={reload}
  />
  <Skattekort {companyId} ansatte={data.ansatte} onDone={reload} />
  <Trekkpalegg {companyId} employees={data.ansatte} />
{/if}
