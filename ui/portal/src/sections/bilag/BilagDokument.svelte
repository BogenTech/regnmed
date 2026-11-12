<script>
  // Ett innboksdokument: dokumentet selv til venstre, bokføringen til
  // høyre (stablet på telefon). Forslaget hentes med en gang, attestering
  // og avvisning bor her, og etter bokføring går siden rett til neste
  // dokument som venter.
  //
  // Forhåndsvisningen viser BARE PDF og bilder, og bare når bytene selv
  // sier det (%PDF-, PNG/JPEG/GIF/WebP-signatur): blob-URL-en får typen
  // vi har sjekket, aldri den opplasteren oppga, så et dokument kan ikke
  // kjøre noe i siden. Alt annet lastes ned (docs/bilagsinnboks.md).
  import { api, post } from "../../lib/api.js";
  import { kr, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { sporsmal } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { harRett } from "../../lib/me.svelte.js";
  import BookingForm from "./BookingForm.svelte";

  let { companyId, documentId } = $props();

  const base = $derived("/companies/" + companyId);

  let doc = $state(null);
  let waiting = $state([]);
  let error = $state(null);
  let ctx = $state(null); // { dims, kontoer, standard, suppliers, policy }
  let forslag = $state(undefined); // undefined = loading, null = none
  let warnings = $state([]); // what the suggestion could not do, and why
  let preview = $state(null); // { kind: "pdf" | "image", url } | { kind: "none" }

  $effect(() => {
    const c = base;
    api(c + "/inbox?status=ny")
      .then((r) => {
        waiting = r.documents;
        doc = r.documents.find((d) => d.document_id === documentId) || null;
        if (!doc) error = "Dokumentet venter ikke lenger på bokføring.";
      })
      .catch((e) => (error = e.message));
    Promise.all([
      api(c + "/dimensions").catch(() => ({ dimensions: [] })),
      api(c + "/accounts").catch(() => ({ kontoer: [], standard: [] })),
      api(c + "/parties?kind=leverandor").catch(() => ({ parties: [] })),
      api(c + "/attestering/policy").catch(() => ({ policy: null })),
    ]).then(([d, a, s, p]) => {
      ctx = { dims: d.dimensions, kontoer: a.kontoer, standard: a.standard, suppliers: s.parties, policy: p.policy };
    });
    api(c + "/inbox/" + documentId + "/forslag")
      .then((r) => {
        warnings = r.warnings || [];
        forslag = r.kilde === "ingen" ? null : r;
      })
      .catch(() => (forslag = null));
  });

  // Preview from the bytes' own signature, never the declared type.
  $effect(() => {
    let url = null;
    api(base + "/inbox/" + documentId + "/content")
      .then((r) => r.blob())
      .then(async (blob) => {
        const head = new Uint8Array(await blob.slice(0, 12).arrayBuffer());
        const starts = (...b) => b.every((x, i) => head[i] === x);
        const type = starts(0x25, 0x50, 0x44, 0x46, 0x2d)
          ? "application/pdf"
          : starts(0x89, 0x50, 0x4e, 0x47)
            ? "image/png"
            : starts(0xff, 0xd8, 0xff)
              ? "image/jpeg"
              : starts(0x47, 0x49, 0x46, 0x38)
                ? "image/gif"
                : starts(0x52, 0x49, 0x46, 0x46) && head[8] === 0x57 && head[9] === 0x45
                  ? "image/webp"
                  : null;
        if (!type) return (preview = { kind: "none" });
        url = URL.createObjectURL(new Blob([blob], { type }));
        preview = { kind: type === "application/pdf" ? "pdf" : "image", url };
      })
      .catch(() => (preview = { kind: "none" }));
    return () => url && URL.revokeObjectURL(url);
  });

  function refreshSuppliers() {
    if (!ctx) return;
    api(base + "/parties?kind=leverandor")
      .then((s) => (ctx = { ...ctx, suppliers: s.parties }))
      .catch(() => {});
  }
  let policyActive = $derived(!!ctx?.policy?.aktiv);
  let canBook = $derived(harRett(companyId, "BILAG_BOKFOR"));
  let canAttest = $derived(harRett(companyId, "ATTESTERING_UTFOR"));

  // The waiting list is re-read: a colleague may have booked the one we
  // would have gone to.
  async function goNext() {
    let next = null;
    try {
      const r = await api(base + "/inbox?status=ny");
      next = r.documents.find((d) => d.document_id !== documentId) || null;
    } catch {}
    location.hash = next ? "#/c/" + companyId + "/bilag/" + next.document_id : "#/c/" + companyId + "/bilag";
  }

  const limitText = (p) =>
    p?.belopsgrense_ore != null
      ? "Bilag på " + kr(p.belopsgrense_ore) + " kr eller mer krever attestering av en annen person før bokføring."
      : "Alle bilag krever attestering av en annen person før bokføring.";

  async function reject() {
    const note = await sporsmal("Hvorfor avvises dokumentet?", { type: "textarea", ok: "Avvis", farlig: true });
    if (!note) return;
    try {
      await post(base + "/inbox/" + documentId + "/avvis", { note });
      toast("Dokumentet er avvist", true);
      goNext();
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function attest(approved) {
    let note = null;
    if (!approved) {
      note = await sporsmal("Hvorfor avvises bilaget i attesteringen?", { type: "textarea", ok: "Avvis", farlig: true });
      if (!note) return;
    }
    try {
      await post(base + "/inbox/" + documentId + "/attester", { godkjent: approved, ...(note ? { note } : {}) });
      toast(approved ? "Attestert" : "Avvist i attestering", true);
      doc = { ...doc, attestering: approved ? "godkjent" : "avvist" };
    } catch (e) {
      toast(e.message, false);
    }
  }

  const SOURCE = { ehf: "lest fra EHF-fakturaen", "pdf-tekst": "tolket fra PDF-teksten", tekst: "tolket fra teksten" };
</script>

<svelte:window onfocus={refreshSuppliers} />

<nav class="flex flex-wrap items-center gap-3 mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/bilag"}>← Innboksen</a>
  {#if waiting.length > 1}
    <span class="text-sm opacity-70">{waiting.length} dokumenter venter</span>
  {/if}
</nav>

{#if error}
  <div class="alert alert-info" role="status">
    <span>{error}</span>
    <a class="btn btn-sm" href={"#/c/" + companyId + "/bilag"}>Til innboksen</a>
  </div>
{:else if !doc}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else}
  <div class="flex flex-wrap items-start gap-3 mb-4">
    <div class="mr-auto min-w-0">
      <h1 class="text-2xl font-bold break-words">{doc.filename}</h1>
      <p class="opacity-70">Mottatt {formatDate(doc.uploaded_at)} fra {doc.uploaded_by}</p>
    </div>
    <button class="btn btn-ghost btn-sm" onclick={() => download(base + "/inbox/" + documentId + "/content", doc.filename)}>
      Last ned
    </button>
    {#if canBook}
      <button class="btn btn-ghost btn-sm text-error" onclick={reject}>Avvis dokumentet</button>
    {/if}
  </div>

  <div class="grid gap-6 lg:grid-cols-2 items-start">
    <section class="card border border-base-300 bg-base-100 lg:sticky lg:top-4" aria-label="Dokumentet">
      <div class="card-body p-2">
        {#if preview === null}
          <div class="flex justify-center p-12"><span class="loading loading-spinner"></span></div>
        {:else if preview.kind === "pdf"}
          <iframe src={preview.url} title={"Dokument: " + doc.filename} class="w-full h-[60vh] lg:h-[75vh] rounded"></iframe>
        {:else if preview.kind === "image"}
          <img src={preview.url} alt={"Dokument: " + doc.filename} class="w-full max-h-[75vh] object-contain rounded" />
        {:else}
          <div class="p-6 text-center">
            <p class="opacity-70 mb-3">
              {forslag?.kilde === "ehf"
                ? "Elektronisk faktura (EHF). Tallene er lest direkte fra den."
                : "Dette dokumentet kan ikke vises her."}
            </p>
            <button class="btn btn-sm" onclick={() => download(base + "/inbox/" + documentId + "/content", doc.filename)}>
              Last ned og se
            </button>
          </div>
        {/if}
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="bokfor-tittel">
      <div class="card-body">
        <h2 id="bokfor-tittel" class="card-title">Bokfør</h2>

        {#if policyActive}
          <div class="alert {doc.attestering === 'godkjent' ? 'alert-success' : doc.attestering === 'avvist' ? 'alert-error' : 'alert-warning'} alert-soft">
            <span>
              {doc.attestering === "godkjent"
                ? "Attestert" + (doc.attestert_av ? " av " + doc.attestert_av : "") + "."
                : doc.attestering === "avvist"
                  ? "Avvist i attestering."
                  : limitText(ctx?.policy)}
            </span>
            {#if canAttest && doc.attestering !== "godkjent"}
              <div class="flex gap-2">
                <button class="btn btn-sm btn-success" onclick={() => attest(true)}>Godkjenn</button>
                <button class="btn btn-sm btn-ghost" onclick={() => attest(false)}>Avvis</button>
              </div>
            {/if}
          </div>
        {/if}

        {#if forslag === undefined || !ctx}
          <div class="flex items-center gap-2 text-sm opacity-70" role="status">
            <span class="loading loading-dots loading-sm"></span> Leser dokumentet …
          </div>
        {:else}
          {#if !forslag && warnings.length}
            <div class="alert alert-soft text-sm" role="status">
              <div>
                <strong>Ingen forslag.</strong>
                {#each warnings as w (w)}<span class="block">{w}</span>{/each}
              </div>
            </div>
          {/if}
          {#if forslag}
            <div class="alert alert-info alert-soft text-sm">
              <div>
                <strong>Forslag</strong> {SOURCE[forslag.kilde] || ""} — kontroller før du bokfører.
                {#each warnings as w (w)}<span class="block mt-1">{w}</span>{/each}
                {#if forslag.begrunnelser?.length}
                  <details class="mt-1">
                    <summary class="cursor-pointer">Hvorfor</summary>
                    {#each forslag.begrunnelser as b (b.felt)}
                      <span class="block text-xs">{b.felt}: {b.hvorfor}</span>
                    {/each}
                  </details>
                {/if}
              </div>
            </div>
          {/if}
          {#if canBook}
            {#key documentId}
              <BookingForm
                {companyId}
                {documentId}
                dims={ctx.dims}
                kontoer={ctx.kontoer}
                standard={ctx.standard}
                suppliers={ctx.suppliers}
                {forslag}
                onBooked={goNext}
              />
            {/key}
          {:else}
            <p class="opacity-70">Du har ikke tilgang til å bokføre.</p>
          {/if}
        {/if}
      </div>
    </section>
  </div>
{/if}
