<script>
  // Icon style picker. The choice is a UI preference and follows the
  // theme doctrine: per user, localStorage, never through the IdP or a
  // token — so this card configures THIS user's portal, and says so.
  import { IKONSTILER, IKONER } from "../../lib/ikoner.js";
  import { prefs, setIkonstil } from "../../lib/prefs.svelte.js";
  import Card from "../../components/Card.svelte";
  import Ikon from "../../components/Ikon.svelte";

  // A representative strip so the styles can be compared where they are
  // chosen.
  const PROVE = ["oversikt", "faktura", "bank", "rapporter", "timer"].filter(
    (slug) => IKONER[slug],
  );
</script>

<Card title="Utseende">
  <p class="text-sm opacity-70 mb-3">
    Ikonstilen i menyen. Valget gjelder deg og denne nettleseren — som fargetemaet lagres det aldri
    sentralt.
  </p>
  <div class="flex flex-col gap-2 max-w-md">
    {#each IKONSTILER as [slug, navn] (slug)}
      <label
        class="flex items-center gap-3 rounded-box border border-base-300 px-3 py-2 cursor-pointer
               {prefs.ikonstil === slug ? 'border-primary bg-base-200' : ''}"
      >
        <input
          type="radio"
          name="ikonstil"
          class="radio radio-sm"
          value={slug}
          checked={prefs.ikonstil === slug}
          onchange={() => setIkonstil(slug)}
        />
        <span class="w-28">{navn}</span>
        <span class="flex items-center gap-3 opacity-80">
          {#if slug === "ingen"}
            <span class="text-xs opacity-60">bare tekst</span>
          {:else}
            {#each PROVE as ikonNavn (ikonNavn)}
              <Ikon navn={ikonNavn} stil={slug} />
            {/each}
          {/if}
        </span>
      </label>
    {/each}
  </div>
  <p class="text-xs opacity-60 mt-3">
    Fargetemaet velges i menylinjen øverst — samme temanavn ser likt ut her og på innloggingssiden.
  </p>
</Card>
