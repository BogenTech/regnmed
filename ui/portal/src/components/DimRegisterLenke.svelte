<script>
  // Where projects are managed, said in one place.
  //
  // The registry lives under Bilag, because a dimension is posting data
  // first (#37) — which makes it the last place someone entering hours
  // would look. So the two views that actually think in projects point
  // at it instead of leaving the user to find it.
  //
  // An 'ansatt' gets her own small Bilag page (receipt upload only) and
  // cannot open the registry, so the link would lead into a wall: she
  // gets the plain sentence, or nothing when there is nothing to say.
  import { company } from "../lib/me.svelte.js";

  // `tekst` is the sentence UP TO the link — the link and the full stop
  // are the component's, so the wording of the destination stays here.
  let { companyId, tekst, ansattTekst = null } = $props();

  let kanApne = $derived(company(companyId)?.access !== "ansatt");
</script>

{#if kanApne}
  <p class="text-xs opacity-70">
    {tekst}
    <a class="link" href={"#/c/" + companyId + "/bilag"}>Bilag → Dimensjoner</a>.
  </p>
{:else if ansattTekst}
  <p class="text-xs opacity-70">{ansattTekst}</p>
{/if}
