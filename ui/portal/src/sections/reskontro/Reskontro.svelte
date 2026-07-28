<script>
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Part from "./Part.svelte";
  import NyPart from "./NyPart.svelte";
  import PartListe from "./PartListe.svelte";
  import ImporterCsv from "./ImporterCsv.svelte";

  // extra = party_id fra #/c/{id}/reskontro/{party_id}
  let { companyId, extra } = $props();

  let parties = $state(null);

  function reload() {
    api("/companies/" + companyId + "/parties")
      .then((svar) => (parties = svar.parties))
      .catch((error) => toast(error.message, false));
  }

  $effect(() => {
    parties = null;
    api("/companies/" + companyId + "/parties")
      .then((svar) => (parties = svar.parties))
      .catch((error) => toast(error.message, false));
  });
</script>

{#if !parties}
  <span class="loading loading-spinner loading-lg"></span>
{:else if extra}
  <Part {companyId} party={parties.find((p) => p.party_id === extra)} partyId={extra} />
{:else}
  <NyPart {companyId} onDone={reload} />
  <PartListe {companyId} {parties} />
  <ImporterCsv {companyId} onDone={reload} />
{/if}
