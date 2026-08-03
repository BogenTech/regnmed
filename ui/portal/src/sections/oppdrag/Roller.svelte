<script>
  // Roller er SETT av rettigheter, ikke en stige. De innebygde er
  // reservert; selskapet kan komponere sine egne av det samme
  // vokabularet.
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import EgenRolle from "./EgenRolle.svelte";
  import Rettighetsvelger from "./Rettighetsvelger.svelte";

  let { companyId, roller, onDone } = $props();

  let nyNavn = $state("");
  let nyValgte = $state([]);

  function beskrivelser(r) {
    return r.rettigheter
      .map((x) => {
        const v = roller.vokabular.find((o) => o.rett === x);
        return v ? v.beskrivelse : x;
      })
      .join(" · ");
  }

  async function opprett() {
    try {
      await post("/companies/" + companyId + "/roles", {
        navn: nyNavn.trim(),
        rettigheter: nyValgte,
      });
      toast("Rollen er opprettet", true);
      nyNavn = "";
      nyValgte = [];
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Roller">
  <p class="text-sm opacity-70 mb-3">
    De innebygde rollene dekker de vanlige tilfellene. Trenger dere noe annet — «en som bare
    fakturerer», «en controller uten lønn» — kan dere sette det sammen selv av rettighetene under.
  </p>
  <h3 class="font-semibold text-sm mb-1">Innebygde</h3>
  <div class="mb-3">
    {#each roller.innebygde as r (r.navn)}
      <details class="collapse collapse-arrow border border-base-300 mb-1">
        <summary class="collapse-title min-h-0 py-2 pl-3 pr-8 text-sm">
          {r.navn}
          <span class="opacity-70">— {r.rettigheter.length} rettigheter</span>
        </summary>
        <div class="collapse-content pl-3">
          {#if r.beskrivelse}
            <p class="text-sm mb-1">{r.beskrivelse}</p>
          {/if}
          <p class="text-xs opacity-70">{beskrivelser(r)}</p>
        </div>
      </details>
    {/each}
  </div>
  <h3 class="font-semibold text-sm mb-1">Egne roller</h3>
  {#if roller.egne.length}
    {#each roller.egne as r (r.id)}
      <EgenRolle {companyId} rolle={r} vokabular={roller.vokabular} {onDone} />
    {/each}
  {:else}
    <p class="text-sm opacity-70 mb-2">Ingen egne roller ennå.</p>
  {/if}
  <details class="collapse collapse-arrow border border-base-300 mt-2">
    <summary class="collapse-title min-h-0 py-2 pl-3 pr-8 text-sm font-semibold">
      Ny rolle
    </summary>
    <div class="collapse-content pl-3">
      <input
        class="input input-sm w-64 my-2"
        placeholder="Navn på rollen"
        bind:value={nyNavn}
      />
      <Rettighetsvelger vokabular={roller.vokabular} bind:valgte={nyValgte} />
      <button class="btn btn-sm mt-2" onclick={opprett}>Opprett</button>
    </div>
  </details>
</Card>
