<script>
  // Brukerne som egen seksjon (#63): medlemmer, invitasjoner og roller
  // samlet der spørsmålet stilles — «hvem har tilgang her, og som hva?»
  // Kortene er de samme som før (Tilgang/Roller, tidligere under
  // Oppdrag); nytt er Leverandørtilgang-kortet, som viser hva
  // plattformen (docs/auth.md §8) har gjort som angår DETTE selskapet.
  // Portalen skjuler, serveren nekter: kort uten tilgang forsvinner.
  import { api } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Tilgang from "../oppdrag/Tilgang.svelte";
  import Roller from "../oppdrag/Roller.svelte";
  import PlattformInnsyn from "./PlattformInnsyn.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const [tilgang, invitasjoner, roller, innsyn] = await Promise.all([
      api("/companies/" + id + "/access").catch(() => null),
      api("/companies/" + id + "/invitations").catch(() => ({ invitasjoner: [] })),
      api("/companies/" + id + "/roles").catch(() => null),
      api("/companies/" + id + "/platform-access").catch(() => null),
    ]);
    data = { tilgang, invitasjoner: invitasjoner.invitasjoner, roller, innsyn };
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

  {#if data.roller}
    <Roller {companyId} roller={data.roller} onDone={reload} />
  {/if}

  {#if data.innsyn}
    <PlattformInnsyn innsyn={data.innsyn.innsyn} />
  {/if}
{/if}
