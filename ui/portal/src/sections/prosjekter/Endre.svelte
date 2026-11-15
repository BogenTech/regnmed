<script>
  // One project or department opened for editing, in place under its row.
  // Name, customer and the billable default go in ONE call; closing is
  // its own button because it is a different decision. The code itself
  // never changes — it is inside the voucher hashes (migration 0018).
  import { onMount, untrack } from "svelte";
  import { api, post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { formatDate, kr, today } from "../../lib/format.js";
  import { lesSats } from "./dimensjon.js";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId, dim, kunder, kanSkrive, kanSatser, kanMedlemmer, onSaved, onClose } = $props();

  let prosjekt = $derived(dim.kind === "prosjekt");
  const sti = () =>
    "/companies/" + companyId + "/dimensions/" + dim.kind + "/" + encodeURIComponent(dim.code);

  // A form copy taken when the panel opens, so an unsaved edit does not
  // leak into the list and a list reload does not wipe it.
  let navn = $state(untrack(() => dim.name));
  let kunde = $state(untrack(() => dim.kunde || ""));
  let fakturerbar = $state(untrack(() => dim.fakturerbar_default));
  let lagrer = $state(false);

  let navnFelt = $state();
  onMount(() => navnFelt?.focus());

  async function lagre(event) {
    event.preventDefault();
    if (lagrer) return;
    const endring = {};
    if (navn.trim() !== dim.name) endring.name = navn.trim();
    if (prosjekt && kunde !== (dim.kunde || "")) endring.kunde = kunde;
    if (prosjekt && fakturerbar !== dim.fakturerbar_default) endring.fakturerbar_default = fakturerbar;
    if (!Object.keys(endring).length) return onClose();
    lagrer = true;
    try {
      await send("PUT", sti(), endring);
      toast((prosjekt ? "Prosjekt " : "Avdeling ") + dim.code + " er lagret", true);
      await onSaved();
      onClose();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }

  async function apneLukk() {
    try {
      await send("PUT", sti(), { active: !dim.active });
      toast(
        (prosjekt ? "Prosjekt " : "Avdeling ") +
          dim.code +
          (dim.active ? " er avsluttet" : " er åpnet igjen"),
        true,
      );
      await onSaved();
      onClose();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Rates: dated and append-only — a change is a new row from its date,
  // and hours already logged keep the rate they were logged at.
  let satser = $state(null); // null = loading, { error } = failed
  let medlemmer = $state([]);
  let nySats = $state({ person_id: "", kr: "", valid_from: today() });

  async function lastSatser() {
    try {
      satser = (await api(sti() + "/satser")).satser;
    } catch (error) {
      satser = { error: error.message };
    }
  }

  onMount(() => {
    if (!prosjekt || !kanSatser) return;
    lastSatser();
    // The person picker needs the member list, which only an admin may
    // read; without it the project's default rate is what can be set.
    if (kanMedlemmer)
      api("/companies/" + companyId + "/access")
        .then((svar) => (medlemmer = svar.medlemmer.filter((m) => m.aktiv)))
        .catch(() => {});
  });

  async function lagreSats(event) {
    event.preventDefault();
    const sats = lesSats(nySats.kr);
    if (sats.feil) return toast(sats.feil, false);
    try {
      await post(sti() + "/satser", {
        person_id: nySats.person_id || null,
        timesats_ore: sats.ore,
        valid_from: nySats.valid_from || null,
      });
      toast("Timesatsen er lagret", true);
      nySats = { person_id: "", kr: "", valid_from: today() };
      await lastSatser();
      onSaved(); // the row shows the caller's rate
    } catch (error) {
      toast(error.message, false);
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
<div
  id={"endre-" + dim.kind + "-" + dim.code}
  class="mt-2 p-3 rounded-box bg-base-200 text-sm"
  onkeydown={tast}
>
  {#if kanSkrive}
    <form class="flex flex-wrap gap-x-3 gap-y-1 items-end" onsubmit={lagre}>
      <label class="fieldset py-0 flex-1 min-w-48">
        <span class="fieldset-legend">Navn</span>
        <input class="input input-sm w-full" required bind:value={navn} bind:this={navnFelt} />
      </label>
      {#if prosjekt}
        <label class="fieldset py-0 min-w-48">
          <span class="fieldset-legend">Kunde</span>
          <select class="select select-sm" bind:value={kunde}>
            <option value="">Ingen kunde</option>
            {#if dim.kunde && !kunder?.some((p) => p.party_no === dim.kunde)}
              <option value={dim.kunde}>{dim.kunde} {dim.kunde_navn}</option>
            {/if}
            {#each kunder || [] as p (p.party_no)}
              <option value={p.party_no}>{p.party_no} {p.name}</option>
            {/each}
          </select>
        </label>
        <label class="label cursor-pointer gap-2 py-2">
          <input type="checkbox" class="checkbox checkbox-sm" bind:checked={fakturerbar} />
          <span>Timene er fakturerbare</span>
        </label>
      {/if}
      <div class="flex gap-2 py-1">
        <button class="btn btn-sm btn-primary" disabled={lagrer}>Lagre</button>
        <button type="button" class="btn btn-sm btn-ghost" onclick={onClose}>Avbryt</button>
      </div>
    </form>
    <div class="mt-2">
      <button type="button" class="btn btn-xs btn-outline" onclick={apneLukk}>
        {#if dim.active}
          Avslutt {prosjekt ? "prosjektet" : "avdelingen"}
        {:else}
          Åpne {prosjekt ? "prosjektet" : "avdelingen"} igjen
        {/if}
      </button>
      {#if dim.active}
        <span class="text-xs opacity-60 ml-1">
          {prosjekt ? "Et avsluttet prosjekt" : "En avsluttet avdeling"} tar ikke imot nye
          posteringer{prosjekt ? " eller timer" : ""}, og kan åpnes igjen.
        </span>
      {/if}
    </div>
  {/if}

  {#if prosjekt && kanSatser}
    <h3 class="font-semibold mt-3 mb-1">Timesatser</h3>
    {#if !satser}
      <span class="loading loading-spinner loading-sm" aria-label="Laster timesatser"></span>
    {:else if satser.error}
      <LoadError error={satser.error} retry={() => ((satser = null), lastSatser())} />
    {:else if satser.length}
      <table class="table table-xs mb-2">
        <thead>
          <tr><th>Hvem</th><th class="text-right">Sats</th><th>Fra</th></tr>
        </thead>
        <tbody>
          {#each satser as rad, i (i)}
            <tr title={"Satt av " + rad.created_by}>
              <td>{rad.person_navn || "Prosjektets standard"}</td>
              <td class="text-right whitespace-nowrap">{kr(rad.timesats_ore)} kr/t</td>
              <td class="whitespace-nowrap">{formatDate(rad.valid_from)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="opacity-70 mb-2">Ingen timesats ennå. Uten sats kan fakturerbare timer ikke føres.</p>
    {/if}
    <p class="text-xs opacity-60 mb-1">
      En ny sats gjelder fra datoen sin. Timer som alt er ført, beholder satsen de ble ført med.
    </p>
    <form class="flex flex-wrap gap-x-3 gap-y-1 items-end" onsubmit={lagreSats}>
      {#if medlemmer.length}
        <label class="fieldset py-0">
          <span class="fieldset-legend">Gjelder</span>
          <select class="select select-sm" bind:value={nySats.person_id}>
            <option value="">Prosjektets standard</option>
            {#each medlemmer as m (m.person_id)}
              <option value={m.person_id}>{m.navn}</option>
            {/each}
          </select>
        </label>
      {/if}
      <label class="fieldset py-0">
        <span class="fieldset-legend">Kroner per time</span>
        <input class="input input-sm w-28" inputmode="decimal" required bind:value={nySats.kr} />
      </label>
      <label class="fieldset py-0">
        <span class="fieldset-legend">Gjelder fra</span>
        <input type="date" class="input input-sm" required bind:value={nySats.valid_from} />
      </label>
      <button class="btn btn-sm mb-1">Lagre sats</button>
    </form>
  {/if}
</div>
