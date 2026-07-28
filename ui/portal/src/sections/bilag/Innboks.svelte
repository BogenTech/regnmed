<script>
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import { lastOppKvittering } from "../../lib/ko.js";
  import Card from "../../components/Card.svelte";
  import AttesteringMerke from "./AttesteringMerke.svelte";
  import BokforForm from "./BokforForm.svelte";
  import KoStatus from "./KoStatus.svelte";
  import { forslagTilSkjema } from "./forslag.js";

  let { companyId, inbox, dims, policy, onDone } = $props();

  let policyAktiv = $derived(!!policy?.aktiv);
  let open = $derived(inbox.filter((d) => d.status === "ny"));
  let decided = $derived(inbox.filter((d) => d.status !== "ny"));

  // Åpent bokføringsskjema: {documentId, forslag|null}. Nøkkelen i
  // markup gjør at et nytt forslag bygger skjemaet på nytt.
  let bokfor = $state(null);
  let koTeller = $state(0); // bumpes for å tvinge KoStatus til å lese på nytt

  async function kameraValgt(event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      const utfall = await lastOppKvittering(companyId, file);
      toast(
        utfall === "sendt"
          ? "Kvitteringen ligger i innboksen"
          : "Uten dekning — kvitteringen er lagret og sendes automatisk",
        utfall === "sendt",
      );
      koTeller++;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function filValgt(event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      await api("/companies/" + companyId + "/inbox?filename=" + encodeURIComponent(file.name), {
        method: "POST",
        headers: { "content-type": file.type || "application/octet-stream" },
        body: file,
      });
      toast("Dokument mottatt i innboksen", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function hentForslag(d) {
    try {
      const f = await api("/companies/" + companyId + "/inbox/" + d.document_id + "/forslag");
      if (f.kilde === "ingen") {
        toast(f.warnings.join(" · ") || "fant ingenting å foreslå", false);
        return;
      }
      bokfor = { documentId: d.document_id, forslag: forslagTilSkjema(f) };
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Etter en bokføring er dokumentet avgjort: skjemaet skal lukkes, ikke
  // stå igjen med tallene til et bilag som allerede er ført.
  function bokfortFerdig() {
    bokfor = null;
    onDone();
  }

  async function avvis(d) {
    const note = prompt("Hvorfor avvises dokumentet?");
    if (!note) return;
    try {
      await post("/companies/" + companyId + "/inbox/" + d.document_id + "/avvis", { note });
      toast("Avvist", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Innboks — dokumentasjon som venter på bokføring">
  <div class="flex gap-2 flex-wrap mb-3">
    <!-- capture="environment" åpner kameraet direkte på telefon; på
         desktop blir det en helt vanlig filvelger. -->
    <label class="btn btn-sm btn-primary">
      Ta bilde av kvittering
      <input
        type="file"
        class="hidden"
        accept="image/*"
        capture="environment"
        onchange={kameraValgt}
      />
    </label>
    <label class="btn btn-sm btn-outline">
      Last opp bilag
      <input type="file" class="hidden" onchange={filValgt} />
    </label>
  </div>
  {#key koTeller}
    <KoStatus />
  {/key}
  {#if open.length}
    <table class="table table-sm">
      <thead>
        <tr><th>Dokument</th><th>Mottatt</th><th>Fra</th><th>Attestering</th><th></th></tr>
      </thead>
      <tbody>
        {#each open as d (d.document_id)}
          <tr>
            <td>
              <button
                class="link"
                onclick={() =>
                  download(
                    "/companies/" + companyId + "/inbox/" + d.document_id + "/content",
                    d.filename,
                  )}
              >
                {d.filename}
              </button>
            </td>
            <td>{d.uploaded_at.slice(0, 10)}</td>
            <td>{d.uploaded_by}</td>
            <td><AttesteringMerke document={d} {policyAktiv} /></td>
            <td>
              <button
                class="btn btn-xs btn-outline"
                title="Les dokumentet og fyll ut bokføringen"
                onclick={() => hentForslag(d)}
              >
                Forslag
              </button>
              <button
                class="btn btn-xs btn-primary"
                onclick={() => (bokfor = { documentId: d.document_id, forslag: null })}
              >
                Bokfør
              </button>
              <button class="btn btn-xs btn-ghost" onclick={() => avvis(d)}>Avvis</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="text-sm opacity-70">Ingen dokumenter venter.</p>
  {/if}
  {#if bokfor}
    {#key bokfor}
      <BokforForm
        {companyId}
        documentId={bokfor.documentId}
        {dims}
        forslag={bokfor.forslag}
        onDone={bokfortFerdig}
      />
    {/key}
  {/if}
  {#each decided.slice(0, 6) as d (d.document_id)}
    <div class="text-xs opacity-60 py-0.5">
      {d.filename} — {d.status}{d.note ? " (" + d.note + ")" : ""} · {d.decided_by || ""}
    </div>
  {/each}
</Card>
