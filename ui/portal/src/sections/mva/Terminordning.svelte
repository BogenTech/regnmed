<script>
  // The terminordning is dated, insert-only history (#51): a new row
  // records the decision Skatteetaten has made, in force from a date.
  // Nothing is derived — the specification, the return and the deadlines
  // follow the registered scheme, and the system never judges eligibility.
  import { post } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";

  let { companyId, scheme, canEdit = false, onDone = () => {} } = $props();

  const NAME = { "to-maneder": "To-månedersterminer", arlig: "Årstermin", primaernaering: "Årstermin (primærnæring)" };

  let kind = $state("to-maneder");
  let from = $state(new Date().getFullYear() + 1 + "-01-01");
  let note = $state("");
  let busy = $state(false);

  async function save(event) {
    event.preventDefault();
    if (busy) return;
    const ok = await bekreft(
      NAME[kind] + " fra " + formatDate(from) + ". Spesifikasjon, melding og frister følger ordningen fra den datoen. Raden kan ikke endres, bare erstattes av en ny.",
      { tittel: "Registrer terminordning?", ok: "Registrer" },
    );
    if (!ok) return;
    busy = true;
    try {
      await post("/companies/" + companyId + "/mva/terminordning", { ordning: kind, valid_from: from, note: note.trim() || null });
      toast("Terminordningen er registrert", true);
      note = "";
      onDone();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<div class="grid gap-3 mt-3 max-w-2xl">
  <p class="text-sm opacity-80">
    Ordningen Skatteetaten har innvilget, med virkning fra en dato. Uten registrert ordning gjelder to-månedersterminer.
    Systemet vurderer aldri om selskapet har rett til en ordning.
  </p>
  {#if scheme.history.length}
    <ul class="grid gap-1 text-sm">
      {#each scheme.history as h, i (i)}
        <li>
          Fra {formatDate(h.valid_from)}: {NAME[h.ordning] || h.ordning}{h.note ? " (" + h.note + ")" : ""}
          <span class="opacity-70">, registrert av {h.created_by}</span>
        </li>
      {/each}
    </ul>
  {/if}
  {#if canEdit}
    <form class="grid gap-3 sm:grid-cols-[1fr_11rem] items-end border border-base-300 rounded-box p-4" onsubmit={save}>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="to-ordning">Ny ordning</label>
        <select id="to-ordning" class="select w-full" bind:value={kind}>
          {#each Object.entries(NAME) as [key, label] (key)}
            <option value={key}>{label}</option>
          {/each}
        </select>
      </div>
      <div class="fieldset py-0">
        <label class="fieldset-legend pt-0" for="to-fra">Gjelder fra</label>
        <input id="to-fra" type="date" class="input w-full" bind:value={from} />
      </div>
      <div class="fieldset py-0 sm:col-span-2">
        <label class="fieldset-legend pt-0" for="to-note">Vedtaksreferanse</label>
        <input id="to-note" class="input w-full" placeholder="F.eks. Skatteetatens brev av 12.01.2026" bind:value={note} />
      </div>
      <button class="btn w-fit" type="submit" disabled={busy}>Registrer</button>
    </form>
  {/if}
</div>
