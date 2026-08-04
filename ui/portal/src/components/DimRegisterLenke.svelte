<script>
  // Where projects are managed, said in one place.
  //
  // The registry is its own Prosjekter section now (it lived under Bilag
  // until the section landed — the last place someone entering hours
  // would look). The link phrases CREATION, so it shows only for someone
  // holding DIMENSJON_SKRIV; everyone else gets the plain sentence, or
  // nothing when there is nothing to say.
  import { harRett } from "../lib/me.svelte.js";

  // `tekst` is the sentence UP TO the link — the link and the full stop
  // are the component's, so the wording of the destination stays here.
  let { companyId, tekst, ansattTekst = null } = $props();

  let kanOpprette = $derived(harRett(companyId, "DIMENSJON_SKRIV"));
</script>

{#if kanOpprette}
  <p class="text-xs opacity-70">
    {tekst}
    <a class="link" href={"#/c/" + companyId + "/prosjekter"}>Prosjekter</a>.
  </p>
{:else if ansattTekst}
  <p class="text-xs opacity-70">{ansattTekst}</p>
{/if}
