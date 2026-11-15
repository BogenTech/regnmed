<script>
  // Avhending er ENVEIS: driftsmidlet lukkes, gevinst eller tap bokføres,
  // og det kan aldri gjenåpnes. So it is two steps: the server does the
  // whole disposal with ?preview=true and rolls it back (a locked period
  // or a missing account shows here, before anything is kept), the
  // person sees the posting, and the confirm carries the book value they
  // saw — refused if a depreciation posted in between.
  import { onMount, tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, oreOrNull, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { gevinstTekst } from "./anlegg.js";

  let { companyId, asset, onClose, onDone } = $props();

  let dato = $state(today());
  let vederlag = $state("");
  let forslag = $state(null);
  let lagrer = $state(false);

  const sti = () => "/companies/" + companyId + "/assets/" + asset.asset_id + "/dispose";
  const id = (s) => asset.asset_id + "-avhend-" + s;
  const fokus = async (s) => (await tick(), document.getElementById(id(s))?.focus());

  onMount(() => fokus("dato"));

  async function seOver(event) {
    event.preventDefault();
    let ore = 0;
    if (vederlag.trim()) {
      ore = oreOrNull(vederlag);
      if (ore == null || ore < 0)
        return toast("Vederlaget må være et beløp i kroner, eller tomt ved utrangering", false);
    }
    const body = { dato, vederlag_ore: ore };
    try {
      forslag = { body, svar: await post(sti() + "?preview=true", body) };
      fokus("oversikt");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avhend() {
    if (lagrer || !forslag) return;
    lagrer = true;
    try {
      const svar = await post(sti(), {
        ...forslag.body,
        forventet_bokfort_ore: forslag.svar.bokfort_ore,
      });
      toast(
        "«" + asset.navn + "» er avhendet" + (svar.voucher ? ", bilag " + svar.voucher : ""),
        true,
      );
      await onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }

  function endre() {
    forslag = null;
    fokus("dato");
  }
</script>

<!-- Esc closes the panel from any field inside it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="mt-2 p-3 rounded-box bg-base-200 text-sm"
  role="region"
  aria-label={"Avhend " + asset.navn}
  onkeydown={(e) => e.key === "Escape" && onClose()}
>
  <form class="flex flex-wrap gap-x-3 gap-y-1 items-end" onsubmit={seOver}>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Avhendet</span>
      <input
        id={id("dato")}
        type="date"
        class="input input-sm"
        required
        bind:value={dato}
        oninput={() => (forslag = null)}
      />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Vederlag (kr, tomt ved utrangering)</span>
      <input
        class="input input-sm w-36"
        inputmode="decimal"
        bind:value={vederlag}
        oninput={() => (forslag = null)}
      />
    </label>
    <button class="btn btn-sm mb-1">Se over</button>
    <button type="button" class="btn btn-sm btn-ghost mb-1" onclick={onClose}>Avbryt</button>
  </form>

  {#if forslag}
    {@const s = forslag.svar}
    <div id={id("oversikt")} tabindex="-1" class="mt-3" role="region" aria-label="Oversikt over avhendingen">
      <p>
        Bokført verdi {kr(s.bokfort_ore)} kr. <span class="font-medium">{gevinstTekst(s.gevinst_ore)}.</span>
      </p>
      {#if s.linjer.length}
        <ul class="my-2">
          {#each s.linjer as l, i (i)}
            <li class="flex gap-2">
              <span class="w-12 tabular-nums">{l.konto}</span>
              <span class="min-w-0 flex-1 truncate">{l.tekst}</span>
              <span class="w-32 text-right tabular-nums">
                {l.belop_ore > 0 ? "debet" : "kredit"} {kr(Math.abs(l.belop_ore))}
              </span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="my-2 opacity-70">Ingenting å bokføre — driftsmidlet lukkes bare.</p>
      {/if}
      <p class="opacity-70">Avhendingen kan ikke angres.</p>
      <div class="flex gap-2 mt-2">
        <button class="btn btn-sm btn-error" disabled={lagrer} onclick={avhend}>
          Avhend og bokfør
        </button>
        <button class="btn btn-sm btn-ghost" onclick={endre}>Endre</button>
      </div>
    </div>
  {/if}
</div>
