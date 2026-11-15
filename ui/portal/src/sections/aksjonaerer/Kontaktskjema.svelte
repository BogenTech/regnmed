<script>
  // A shareholder's contact details, edited in place under the row with
  // every field filled in from what is stored — so saving a new name
  // never wipes the address or the country. The identity (fødselsnummer,
  // org.nr., UTL-id) is not here: it is permanent, a change of identity
  // is a new shareholder.
  import { onMount, untrack } from "svelte";
  import { send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, aksjonar, onSaved, onClose } = $props();

  const a = untrack(() => aksjonar);
  let felt = $state({
    navn: a.navn,
    adresse: a.adresse ?? "",
    postnummer: a.postnummer ?? "",
    poststed: a.poststed ?? "",
    landkode: a.landkode ?? "",
  });
  let lagrer = $state(false);
  const id = (navn) => "kontakt-" + a.id + "-" + navn;

  onMount(() => document.getElementById(id("navn"))?.focus());

  async function lagre(event) {
    event.preventDefault();
    if (lagrer) return;
    lagrer = true;
    const tom = (v) => v.trim() || null;
    try {
      await send("PUT", "/companies/" + companyId + "/shareholders/" + a.id + "/contact", {
        navn: felt.navn.trim(),
        adresse: tom(felt.adresse),
        postnummer: tom(felt.postnummer),
        poststed: tom(felt.poststed),
        landkode: tom(felt.landkode)?.toUpperCase() ?? null,
      });
      toast("Kontaktopplysningene til " + felt.navn.trim() + " er lagret", true);
      await onSaved();
      onClose();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }

  function tast(event) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="mt-2 p-3 rounded-box bg-base-200 text-sm" onkeydown={tast}>
  <form class="flex flex-wrap gap-x-3 gap-y-1 items-end" onsubmit={lagre}>
    <label class="fieldset py-0 flex-1 min-w-48">
      <span class="fieldset-legend">Navn</span>
      <input id={id("navn")} class="input input-sm w-full" required bind:value={felt.navn} />
    </label>
    <label class="fieldset py-0 flex-1 min-w-48">
      <span class="fieldset-legend">Adresse</span>
      <input class="input input-sm w-full" autocomplete="street-address" bind:value={felt.adresse} />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Postnummer</span>
      <input class="input input-sm w-24" inputmode="numeric" bind:value={felt.postnummer} />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Poststed</span>
      <input class="input input-sm w-36" bind:value={felt.poststed} />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Landkode</span>
      <input
        class="input input-sm w-20"
        maxlength="2"
        title="To bokstaver, for eksempel SE. Tom for Norge."
        bind:value={felt.landkode}
      />
    </label>
    <div class="flex gap-2 py-1">
      <button class="btn btn-sm btn-primary" disabled={lagrer}>Lagre</button>
      <button type="button" class="btn btn-sm btn-ghost" onclick={onClose}>Avbryt</button>
    </div>
  </form>
</div>
