<script>
  // Innboksen er Bilag-seksjonens forside: få dokumentene inn (kamera,
  // filer — gjerne mange, eller dratt inn), og se hva som venter. Hvert
  // dokument åpnes på sin egen side med dokumentet ved siden av
  // bokføringen.
  import { api } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { lastOppKvittering } from "../../lib/ko.js";
  import AttesteringMerke from "./AttesteringMerke.svelte";
  import KoStatus from "./KoStatus.svelte";

  let { companyId } = $props();

  let docs = $state(null);
  let error = $state(null);
  let policyActive = $state(false);
  let mailAddress = $state(null);
  let queueCounter = $state(0);
  let uploading = $state(0);
  let dragging = $state(false);

  const base = $derived("/companies/" + companyId);

  function load() {
    api(base + "/inbox?status=ny")
      .then((r) => (docs = r.documents))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    load();
    api(base + "/attestering/policy")
      .then((r) => (policyActive = !!r.policy?.aktiv))
      .catch(() => {});
    api(base + "/inbox/settings")
      .then((s) => (mailAddress = s.aktiv ? s.adresse : null))
      .catch(() => {});
  });

  async function camera(event) {
    const file = event.target.files[0];
    if (!file) return;
    event.target.value = "";
    try {
      const outcome = await lastOppKvittering(companyId, file);
      toast(
        outcome === "sendt"
          ? "Kvitteringen ligger i innboksen"
          : "Uten dekning — kvitteringen er lagret og sendes automatisk",
        outcome === "sendt",
      );
      queueCounter++;
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  // Several files at once, one request each; a failed one is named and
  // the rest still arrive.
  async function upload(files) {
    const list = [...files];
    if (!list.length) return;
    uploading = list.length;
    const failed = [];
    for (const file of list) {
      try {
        await api(base + "/inbox?filename=" + encodeURIComponent(file.name), {
          method: "POST",
          headers: { "content-type": file.type || "application/octet-stream" },
          body: file,
        });
      } catch (e) {
        failed.push(file.name + ": " + e.message);
      }
      uploading--;
    }
    const ok = list.length - failed.length;
    if (ok) toast(ok === 1 ? "Dokumentet ligger i innboksen" : ok + " dokumenter ligger i innboksen", true);
    for (const f of failed) toast(f, false);
    load();
  }

  function drop(event) {
    event.preventDefault();
    dragging = false;
    upload(event.dataTransfer.files);
  }

  async function copyAddress() {
    try {
      await navigator.clipboard.writeText(mailAddress);
      toast("Adressen er kopiert", true);
    } catch {
      toast("Kunne ikke kopiere — merk adressen og kopier den selv", false);
    }
  }
</script>

<div class="flex flex-wrap items-center gap-3 mb-4">
  <h1 class="text-2xl font-bold mr-auto">Innboks</h1>
  <label class="btn btn-primary">
    Ta bilde av kvittering
    <!-- capture opens the camera on a phone; a file picker on desktop. -->
    <input type="file" class="sr-only" accept="image/*" capture="environment" onchange={camera} />
  </label>
</div>

<div
  class="border-2 border-dashed rounded-box p-6 mb-4 text-center transition-colors {dragging
    ? 'border-primary bg-base-200'
    : 'border-base-300'}"
  role="region"
  aria-label="Last opp dokumenter"
  ondragover={(e) => {
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={drop}
>
  {#if uploading}
    <span class="loading loading-spinner loading-sm"></span>
    Laster opp, {uploading} igjen …
  {:else}
    <p class="mb-2">Dra fakturaer og kvitteringer hit, eller</p>
    <label class="btn btn-sm">
      Velg filer
      <input type="file" class="sr-only" multiple onchange={(e) => { upload(e.target.files); e.target.value = ""; }} />
    </label>
  {/if}
  {#if mailAddress}
    <p class="text-sm opacity-70 mt-3">
      Leverandører kan også sende til
      <code class="bg-base-200 px-1 rounded">{mailAddress}</code>
      <button class="btn btn-ghost btn-xs" onclick={copyAddress}>Kopier</button>
    </p>
  {/if}
  {#key queueCounter}<KoStatus />{/key}
</div>

<div class="card border border-base-300 bg-base-100 mb-6">
  <div class="card-body overflow-x-auto">
    <h2 class="card-title">Venter på bokføring{docs?.length ? " (" + docs.length + ")" : ""}</h2>
    {#if error}
      <div class="alert alert-error" role="alert"><span>{error}</span></div>
    {:else if docs === null}
      <span class="loading loading-spinner"></span>
    {:else if docs.length === 0}
      <p class="opacity-70">Ingenting venter. Nye dokumenter dukker opp her.</p>
    {:else}
      <table class="table">
        <caption class="sr-only">Dokumenter som venter. Velg et for å bokføre det.</caption>
        <thead>
          <tr>
            <th scope="col">Dokument</th>
            <th scope="col" class="hidden sm:table-cell">Mottatt</th>
            {#if policyActive}<th scope="col">Attestering</th>{/if}
          </tr>
        </thead>
        <tbody>
          {#each docs as d (d.document_id)}
            <tr class="relative hover:bg-base-200 focus-within:bg-base-200">
              <td class="min-w-0">
                <a class="font-medium break-all after:absolute after:inset-0" href={"#/c/" + companyId + "/bilag/" + d.document_id}>
                  {d.filename}
                </a>
                <span class="block text-sm opacity-70">
                  {d.uploaded_by}<span class="sm:hidden">, {formatDate(d.uploaded_at)}</span>
                </span>
              </td>
              <td class="hidden sm:table-cell whitespace-nowrap">{formatDate(d.uploaded_at)}</td>
              {#if policyActive}<td><AttesteringMerke document={d} policyAktiv={policyActive} /></td>{/if}
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>
