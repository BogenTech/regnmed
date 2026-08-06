<script>
  // Renders whatever lib/dialog.svelte.js was asked (bekreft/skjema).
  // A native <dialog> via showModal() so focus trapping, Esc and the
  // backdrop come from the platform; daisyUI's modal classes style it.
  import { dialog } from "../lib/dialog.svelte.js";

  let el = $state(null);
  let verdier = $state({});

  $effect(() => {
    const d = dialog.aktiv;
    if (!d || !el) return;
    const v = {};
    for (const f of d.felter ?? []) {
      v[f.navn] = f.type === "checkbox" ? (f.standard ?? false) : (f.standard ?? "");
    }
    verdier = v;
    if (!el.open) el.showModal();
  });

  // The single resolution point: an explicit answer nulls `aktiv` before
  // closing, so the close event (Esc, backdrop) only resolves when the
  // dialog closed WITHOUT an answer.
  function ferdig(resultat) {
    const d = dialog.aktiv;
    if (!d) return;
    dialog.aktiv = null;
    el?.close();
    d.resolve(resultat);
  }

  function onclose() {
    const d = dialog.aktiv;
    if (!d) return;
    dialog.aktiv = null;
    d.resolve(d.type === "bekreft" ? false : null);
  }

  let mangler = $derived.by(() => {
    const d = dialog.aktiv;
    if (!d || d.type !== "skjema") return false;
    return d.felter.some((f) => {
      if (f.valgfri || f.type === "checkbox" || f.type === "select") return false;
      return String(verdier[f.navn] ?? "").trim() === "";
    });
  });

  function svar() {
    const d = dialog.aktiv;
    if (!d || mangler) return;
    const ut = {};
    for (const f of d.felter) {
      const v = verdier[f.navn];
      ut[f.navn] = typeof v === "string" ? v.trim() : v;
    }
    ferdig(ut);
  }
</script>

<dialog bind:this={el} class="modal" {onclose}>
  {#if dialog.aktiv}
    {@const d = dialog.aktiv}
    <div class="modal-box border border-base-300">
      <h3 class="font-semibold text-lg">{d.tittel}</h3>
      {#if d.type === "bekreft"}
        <p class="py-3 text-sm">{d.melding}</p>
        <div class="modal-action">
          <button class="btn" onclick={() => ferdig(false)}>{d.avbryt}</button>
          <button class="btn {d.farlig ? 'btn-error' : 'btn-primary'}" onclick={() => ferdig(true)}>
            {d.ok}
          </button>
        </div>
      {:else}
        {#if d.melding}
          <p class="py-2 text-sm opacity-70">{d.melding}</p>
        {/if}
        <!-- A form so Enter in a text field answers, like prompt() did. -->
        <form
          onsubmit={(e) => {
            e.preventDefault();
            svar();
          }}
        >
          <div class="flex flex-col gap-3 py-2">
            {#each d.felter as f (f.navn)}
              <div>
                {#if f.type === "checkbox"}
                  <label class="label cursor-pointer justify-start gap-2">
                    <input type="checkbox" class="checkbox" bind:checked={verdier[f.navn]} />
                    <span class="text-sm">{f.etikett}</span>
                  </label>
                {:else}
                  {#if f.etikett}
                    <span class="label text-sm mb-1">{f.etikett}</span>
                  {/if}
                  {#if f.type === "select"}
                    <select class="select w-full" bind:value={verdier[f.navn]}>
                      {#each f.valg as [verdi, navn] (verdi)}
                        <option value={verdi}>{navn}</option>
                      {/each}
                    </select>
                  {:else if f.type === "textarea"}
                    <textarea class="textarea w-full" rows="3" bind:value={verdier[f.navn]}
                    ></textarea>
                  {:else}
                    <input type={f.type ?? "text"} class="input w-full" bind:value={verdier[f.navn]} />
                  {/if}
                {/if}
                {#if f.hjelp}
                  <p class="text-xs opacity-60 mt-1">{f.hjelp}</p>
                {/if}
              </div>
            {/each}
          </div>
          <div class="modal-action">
            <button type="button" class="btn" onclick={() => ferdig(null)}>{d.avbryt}</button>
            <button
              type="submit"
              class="btn {d.farlig ? 'btn-error' : 'btn-primary'}"
              disabled={mangler}
            >
              {d.ok}
            </button>
          </div>
        </form>
      {/if}
    </div>
    <form method="dialog" class="modal-backdrop">
      <button aria-label="Lukk"></button>
    </form>
  {/if}
</dialog>
