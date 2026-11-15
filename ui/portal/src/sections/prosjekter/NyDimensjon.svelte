<script>
  // Create a project or a department in one submit. A project's billable
  // default and first rate are separate calls (the server checks the
  // right for each part), so the rate is validated BEFORE anything is
  // created — otherwise a bad rate would leave a half-made project and a
  // retry would fail on the code that now exists.
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { lesSats } from "./dimensjon.js";

  let { companyId, kind, kunder, kanSatser, onCreated } = $props();

  let prosjekt = $derived(kind === "prosjekt");
  let felt = $state({ code: "", name: "", kunde: "", fakturerbar: false, sats: "" });
  let lagrer = $state(false);
  let kodeFelt = $state();

  async function opprett(event) {
    event.preventDefault();
    if (lagrer) return;
    const code = felt.code.trim();
    const sats = prosjekt && kanSatser && felt.sats.trim() ? lesSats(felt.sats) : null;
    if (sats?.feil) return toast(sats.feil, false);
    lagrer = true;
    try {
      await post("/companies/" + companyId + "/dimensions", {
        kind,
        code,
        name: felt.name.trim(),
        kunde: prosjekt && felt.kunde ? felt.kunde : null,
      });
    } catch (error) {
      lagrer = false;
      return toast(error.message, false);
    }
    // It exists now: whatever follows, the form starts over.
    const navn = (prosjekt ? "Prosjekt " : "Avdeling ") + code;
    const sti = "/companies/" + companyId + "/dimensions/prosjekt/" + encodeURIComponent(code);
    try {
      if (prosjekt && felt.fakturerbar) await send("PUT", sti, { fakturerbar_default: true });
      if (sats) await post(sti + "/satser", { person_id: null, timesats_ore: sats.ore });
      toast(navn + " er opprettet", true);
    } catch (error) {
      toast(navn + " er opprettet, men: " + error.message + " Rett det under Endre.", false);
    }
    felt = { code: "", name: "", kunde: "", fakturerbar: false, sats: "" };
    lagrer = false;
    await onCreated();
    kodeFelt?.focus();
  }
</script>

<form class="flex flex-wrap gap-x-3 gap-y-1 items-end mt-3" onsubmit={opprett}>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Kode</span>
    <input
      class="input input-sm w-24"
      required
      pattern="[A-Za-z0-9\-]+"
      title="A–Z, 0–9 og bindestrek. Koden kan ikke endres senere."
      bind:value={felt.code}
      bind:this={kodeFelt}
    />
  </label>
  <label class="fieldset py-0 flex-1 min-w-40">
    <span class="fieldset-legend">Navn</span>
    <input class="input input-sm w-full" required bind:value={felt.name} />
  </label>
  {#if prosjekt}
    {#if kunder?.length}
      <label class="fieldset py-0">
        <span class="fieldset-legend">Kunde</span>
        <select class="select select-sm" bind:value={felt.kunde}>
          <option value="">Ingen kunde</option>
          {#each kunder as p (p.party_no)}
            <option value={p.party_no}>{p.party_no} {p.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#if kanSatser}
      <label class="fieldset py-0">
        <span class="fieldset-legend">Kr per time</span>
        <input class="input input-sm w-24" inputmode="decimal" bind:value={felt.sats} />
      </label>
    {/if}
    <label class="label cursor-pointer gap-2 py-2">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={felt.fakturerbar} />
      <span class="text-sm">Fakturerbare timer</span>
    </label>
  {/if}
  <button class="btn btn-sm btn-primary mb-1" disabled={lagrer}>
    {prosjekt ? "Opprett prosjekt" : "Opprett avdeling"}
  </button>
</form>
