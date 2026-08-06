<script>
  // Til attestering (#47): bilag som venter på en godkjenning.
  // Portalen viser køen — HÅNDHEVINGEN ligger i transaksjonene
  // (bokfor_inbox_document nekter selv-attestering over grensen).
  import { untrack } from "svelte";
  import { post } from "../../lib/api.js";
  import { kr, parseKr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { sporsmal } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import AttesteringMerke from "./AttesteringMerke.svelte";

  let { companyId, inbox, policy, members, onDone } = $props();

  let policyAktiv = $derived(!!policy?.aktiv);
  let venter = $derived(
    inbox.filter((d) => d.status === "ny" && d.attestering !== "godkjent"),
  );

  let aktiv = $state(untrack(() => !!policy?.aktiv));
  let grense = $state(untrack(() => policy?.belopsgrense_ore !== null && policy?.belopsgrense_ore !== undefined
      ? kr(policy.belopsgrense_ore)
      : "",));
  let attestant = $state(untrack(() => policy?.attestant_person_id || ""));

  async function godkjenn(d) {
    try {
      await post("/companies/" + companyId + "/inbox/" + d.document_id + "/attester", {
        godkjent: true,
      });
      toast("Attestert", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avvis(d) {
    const note = await sporsmal("Hvorfor avvises bilaget i attesteringen?", {
      type: "textarea",
      ok: "Avvis",
      farlig: true,
    });
    if (!note) return;
    try {
      await post("/companies/" + companyId + "/inbox/" + d.document_id + "/attester", {
        godkjent: false,
        note,
      });
      toast("Avvist i attestering", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function lagrePolicy() {
    try {
      await post("/companies/" + companyId + "/attestering/policy", {
        aktiv,
        belopsgrense_ore: grense.trim() ? parseKr(grense) : null,
        attestant_person_id: attestant || null,
      });
      toast("Policy lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Til attestering">
  {#if policyAktiv}
    <span class="badge badge-primary badge-sm w-fit">policy aktiv</span>
  {/if}
  <p class="text-sm opacity-70 mb-2">
    Den som godkjenner en kostnad skal ikke være den som bokfører eller betaler den. Med aktiv
    policy krever bilag over beløpsgrensen en godkjent attestering av en <em>annen</em> person før
    bokføring; betalingslister må godkjennes av en annen enn oppretteren, og utleggskrav kan ikke
    godkjennes av innsenderen. Beslutningene er et spor som aldri slettes.
  </p>
  {#if venter.length}
    <table class="table table-sm mb-3">
      <thead>
        <tr><th>Dokument</th><th>Mottatt</th><th>Fra</th><th>Status</th><th></th></tr>
      </thead>
      <tbody>
        {#each venter as d (d.document_id)}
          <tr>
            <td>{d.filename}</td>
            <td>{d.uploaded_at.slice(0, 10)}</td>
            <td>{d.uploaded_by}</td>
            <td><AttesteringMerke document={d} {policyAktiv} /></td>
            <td>
              <button class="btn btn-xs btn-success" onclick={() => godkjenn(d)}>Godkjenn</button>
              <button class="btn btn-xs btn-ghost" onclick={() => avvis(d)}>Avvis</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen bilag venter på attestering.</p>
  {/if}
  <div class="flex gap-2 items-center flex-wrap">
    <label class="label cursor-pointer gap-2">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={aktiv} />
      <span>Krev attestering</span>
    </label>
    <input
      class="input input-sm w-36"
      placeholder="Beløpsgrense (kr)"
      bind:value={grense}
    />
    <select class="select select-sm" bind:value={attestant}>
      <option value="">Alle med bokføringstilgang</option>
      {#each members as m (m.person_id)}
        <option value={m.person_id}>{m.name}</option>
      {/each}
    </select>
    <button class="btn btn-sm" onclick={lagrePolicy}>Lagre policy</button>
  </div>
  {#if policy}
    <p class="text-xs opacity-60 mt-2">
      Gjeldende policy satt av {policy.created_by} {policy.created_at.slice(0, 10)}.
    </p>
  {:else}
    <p class="text-xs opacity-60 mt-2">
      Ingen policy registrert — attestering er frivillig.
    </p>
  {/if}
</Card>
