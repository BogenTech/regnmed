<script>
  // Attesteringspolicyen (#47): den som godkjenner en kostnad skal ikke
  // være den som bokfører eller betaler den. Køen av dokumenter som
  // venter på attestering ligger i innboksen; dette er bare regelen.
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, parseKr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, policy, members, onSaved } = $props();

  let active = $state(untrack(() => !!policy?.aktiv));
  let limit = $state(untrack(() => (policy?.belopsgrense_ore != null ? kr(policy.belopsgrense_ore) : "")));
  let approver = $state(untrack(() => policy?.attestant_person_id || ""));
  let requireForMachine = $state(untrack(() => !!policy?.krev_for_maskin));

  async function save(event) {
    event.preventDefault();
    try {
      await post("/companies/" + companyId + "/attestering/policy", {
        aktiv: active,
        belopsgrense_ore: limit.trim() ? parseKr(limit) : null,
        attestant_person_id: approver || null,
        krev_for_maskin: requireForMachine,
      });
      toast("Attesteringsregelen er lagret", true);
      onSaved();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Attestering">
  <p class="text-sm opacity-70">
    Med attestering på må bilag over beløpsgrensen godkjennes av en <em>annen</em> person før de
    bokføres. Betalingslister godkjennes alltid av en annen enn den som lagde dem, og utlegg kan ikke
    godkjennes av den som sendte dem. Beslutningene slettes aldri.
  </p>
  <form class="grid gap-3 max-w-lg" onsubmit={save}>
    <label class="label cursor-pointer justify-start gap-2">
      <input type="checkbox" class="toggle toggle-sm" bind:checked={active} />
      <span>Krev attestering</span>
    </label>
    <label class="fieldset">
      <span class="fieldset-legend">Beløpsgrense (kr)</span>
      <input class="input w-48" inputmode="decimal" bind:value={limit} />
      <span class="label whitespace-normal">Tom = alle bilag må attesteres</span>
    </label>
    <label class="fieldset">
      <span class="fieldset-legend">Hvem attesterer</span>
      <select class="select w-full" bind:value={approver}>
        <option value="">Alle med bokføringstilgang</option>
        {#each members as m (m.person_id)}
          <option value={m.person_id}>{m.name}</option>
        {/each}
      </select>
    </label>
    <label class="label cursor-pointer justify-start gap-2 whitespace-normal">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={requireForMachine} />
      <span>Alt en agent bokfører må attesteres av et menneske</span>
    </label>
    <div><button class="btn btn-sm">Lagre</button></div>
  </form>
  <p class="text-xs opacity-60">
    {policy
      ? "Gjeldende regel satt av " + policy.created_by + " " + formatDate(policy.created_at) + "."
      : "Ingen regel registrert — attestering er frivillig."}
  </p>
</Card>
