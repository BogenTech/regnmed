<script>
  // Trekkpålegg fetched from Skatteetaten (docs/trekkpalegg.md §2, #334) and
  // registered by hand (§6, #339), from the letter, with the letter stored
  // beside it. A change or a stop is a new
  // version; nothing is edited afterwards. Visible only with LONN_LES —
  // without it the server says no and the card does not show.
  import { api, post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, sporsmal } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { kr, formatDate, today } from "../../lib/format.js";
  import { strictOre } from "../../lib/planer.js";
  import Card from "../../components/Card.svelte";

  let { companyId, employees } = $props();

  const KINDS = [
    ["samordnet", "Samordnet trekk (ny ordning)"],
    ["skatt", "Skatt og avgift (gammel ordning)"],
    ["ovrige", "Øvrige krav (gammel ordning)"],
  ];
  const kindLabel = (k) => KINDS.find((x) => x[0] === k)?.[1] ?? k;

  let data = $state(null);
  let busy = $state(false);

  async function load() {
    try {
      data = await api("/companies/" + companyId + "/payroll/garnishments");
    } catch {
      data = null; // no LONN_LES: the card is not for this person
    }
  }
  $effect(() => {
    companyId;
    load();
  });

  const emptyDraft = () => ({
    editing: null, // the order being changed, or null for a new one
    employeeId: "",
    kind: "samordnet",
    sizeKind: "amount",
    amount: "",
    percent: "",
    from: today(),
    to: "",
    kid: "",
    payeeOrgnumber: "",
    payeeAccount: "",
    dueDays: "",
    note: "",
    file: null,
  });
  let draft = $state(null);

  function edit(order) {
    const v = order.versjoner[0];
    const text = (n) => (n / 100).toFixed(2).replace(".", ",");
    draft = {
      ...emptyDraft(),
      editing: order.id,
      employeeId: order.employee_id,
      kind: order.type,
      sizeKind: v.prosent_bp != null ? "percent" : "amount",
      amount: v.belop_ore != null ? text(v.belop_ore) : "",
      percent: v.prosent_bp != null ? text(v.prosent_bp) : "",
      from: v.gjelder_fra,
      to: v.gjelder_til ?? "",
      kid: v.kid ?? "",
      payeeOrgnumber: v.mottaker_orgnr ?? "",
      payeeAccount: v.mottaker_konto ?? "",
      dueDays: v.frist_dager != null ? String(v.frist_dager) : "",
    };
  }

  async function save(event) {
    event.preventDefault();
    const d = draft;
    const sizeKind = d.kind === "ovrige" ? "amount" : d.sizeKind;
    // Money and percent are read as TEXT, never through a float: "12,34"
    // is 1 234 øre or 1 234 basis points.
    const value = sizeKind === "amount" ? strictOre(d.amount) : strictOre(d.percent);
    if (value == null) {
      toast(sizeKind === "amount" ? "Skriv beløpet i kroner, f.eks. 4 000 eller 4 000,50" : "Skriv prosenten med høyst to desimaler, f.eks. 12,5", false);
      return;
    }
    if (!d.editing && !d.file) {
      toast("Last opp brevet pålegget kom i", false);
      return;
    }
    busy = true;
    try {
      let letterId = null;
      if (d.file) {
        const reply = await api(
          "/companies/" + companyId + "/payroll/garnishments/letters?filename=" + encodeURIComponent(d.file.name),
          { method: "POST", headers: { "content-type": d.file.type || "application/octet-stream" }, body: d.file },
        );
        letterId = reply.brev_id;
      }
      const hasPayee = d.kind !== "skatt";
      const body = {
        employee_id: d.employeeId || null,
        type: d.kind,
        belop_ore: sizeKind === "amount" ? value : null,
        prosent_bp: sizeKind === "percent" ? value : null,
        gjelder_fra: d.from,
        gjelder_til: d.to || null,
        kid: hasPayee ? d.kid : null,
        mottaker_orgnr: hasPayee ? d.payeeOrgnumber : null,
        mottaker_konto: hasPayee ? d.payeeAccount : null,
        frist_dager: d.kind === "ovrige" && d.dueDays !== "" ? Number(d.dueDays) : null,
        brev_id: letterId,
        notat: d.note || null,
      };
      const base = "/companies/" + companyId + "/payroll/garnishments";
      if (d.editing) await send("PUT", base + "/" + d.editing, body);
      else await post(base, body);
      toast(d.editing ? "Ny versjon av pålegget er registrert" : "Pålegget er registrert", true);
      draft = null;
      await load();
    } catch (error) {
      toast(error.message, false);
    } finally {
      busy = false;
    }
  }

  async function stop(order) {
    const note = await sporsmal("Stoppe trekkpålegget for " + order.navn + "?", {
      melding: "Fra neste lønnskjøring trekkes ingenting. Stoppen lagres som en ny versjon og kan ikke angres — et nytt brev registreres som et nytt pålegg.",
      valgfri: true,
      ok: "Stopp pålegget",
      farlig: true,
    });
    if (note === null) return;
    try {
      await post("/companies/" + companyId + "/payroll/garnishments/" + order.id + "/stop", { notat: note || null });
      toast("Pålegget er stoppet", true);
      await load();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function attest(order, v, approved) {
    let note = null;
    if (approved) {
      if (!(await bekreft("Godkjenne versjon " + v.versjon + " av trekkpålegget for " + order.navn + "? Sammenlign beløp og KID med brevet først."))) return;
    } else {
      note = await sporsmal("Hvorfor avvises pålegget?", { ok: "Avvis" });
      if (!note) return;
    }
    try {
      await post(
        "/companies/" + companyId + "/payroll/garnishments/" + order.id + "/versions/" + v.versjon + "/attest",
        { godkjent: approved, note },
      );
      await load();
    } catch (error) {
      toast(error.message, false);
    }
  }

  const sizeText = (v) =>
    v.belop_ore != null ? kr(v.belop_ore) + " kr/mnd" : (v.prosent_bp / 100).toFixed(2).replace(".", ",") + " %";

  // Fetched from Skatteetaten (§2, #334): the newest version of each order,
  // shown with the period that covers today. Nothing here is edited: a
  // change comes as a new version on the next fetch.
  const periodNow = (f) =>
    (f.perioder ?? []).filter((p) => p.fra <= today() && (!p.til || today() <= p.til)).at(-1);
  const lastAttempt = $derived(data?.henting?.forsok?.[0]);

  async function fetchNow() {
    busy = true;
    try {
      const r = await post("/companies/" + companyId + "/payroll/garnishments/fetch", {});
      toast(r.nye_versjoner ? r.nye_versjoner + " nye eller endrede pålegg hentet" : "Ingen endringer fra Skatteetaten", true);
      await load();
    } catch (e) {
      toast(e.message, false);
      await load();
    } finally {
      busy = false;
    }
  }

  // Payroll settings: the account and the company's own KID for skatt. The
  // object's keys are the request's wire names.
  let settings = $state(null);
  function openSettings() {
    const i = data.innstillinger ?? {};
    settings = {
      konto: i.konto ?? "",
      skatt_kid: i.skatt_kid ?? "",
      skatt_konto: i.skatt_konto ?? "",
      skatt_mottaker_orgnr: i.skatt_mottaker_orgnr ?? "",
      gjelder_fra: today(),
    };
  }
  async function saveSettings(event) {
    event.preventDefault();
    try {
      await send("PUT", "/companies/" + companyId + "/payroll/garnishment-settings", settings);
      toast("Innstillingene er lagret", true);
      settings = null;
      await load();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if data}
  <Card title="Trekkpålegg">
    <p class="text-sm opacity-80 mb-3 max-w-3xl">
      Pålegg om utleggstrekk som kommer i brev, registreres her og trekkes i lønnskjøringen: bokført
      som leverandørpost med KID, betalt fra betalingslisten og rapportert i a-meldingen. Pålegg
      fra Skatteetaten hentes hver natt og trekkes uten at noen må godkjenne dem; et hentet pålegg
      med samme KID som et registrert, avløser det registrerte.
    </p>

    <section class="mb-4" aria-labelledby="trekk-hentet">
      <div class="flex flex-wrap items-center gap-3 mb-2">
        <h3 id="trekk-hentet" class="font-semibold">Fra Skatteetaten</h3>
        <span class="text-xs opacity-70">
          {data.henting.siste_vellykkede
            ? "Sist hentet " + formatDate(data.henting.siste_vellykkede)
            : "Aldri hentet"}
        </span>
        {#if data.henting.tilgjengelig}
          <button class="btn btn-sm" disabled={busy} onclick={fetchNow}>Hent trekkpålegg</button>
        {/if}
      </div>
      {#if lastAttempt && lastAttempt.ok === false}
        <div role="alert" class="alert alert-warning alert-soft text-sm mb-2">
          Siste henting {formatDate(lastAttempt.tidspunkt)} feilet: {lastAttempt.feil}
        </div>
      {/if}
      {#if data.hentede.length}
        <div class="overflow-x-auto">
          <table class="table table-sm">
            <thead>
              <tr>
                <th>Ansatt</th>
                <th>Trekkpålegg</th>
                <th class="text-right">Trekk nå</th>
                <th>KID</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {#each data.hentede as f (f.trekkid)}
                {@const p = periodNow(f)}
                <tr>
                  <td class="whitespace-nowrap">
                    {#if f.navn}{f.navn}{:else}<span class="badge badge-sm badge-warning">ikke i ansattregisteret</span>{/if}
                  </td>
                  <td class="text-xs">{f.trekkid} v{f.trekkversjon}{f.saksnummer ? ", sak " + f.saksnummer : ""}</td>
                  <td class="text-right whitespace-nowrap">{p && !p.feil ? sizeText(p) : "–"}</td>
                  <td class="font-mono text-xs">{f.kid ?? ""}</td>
                  <td>
                    {#if f.feil}
                      <span class="badge badge-sm badge-warning">kan ikke leses</span>
                    {:else}
                      <span class="badge badge-sm" class:badge-warning={f.status === "aktiv" && !f.i_siste_liste}>
                        {f.status === "aktiv" && !f.i_siste_liste ? "aktiv, ikke i siste liste" : f.status}
                      </span>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="text-sm opacity-70">Ingen pålegg fra Skatteetaten.</p>
      {/if}
    </section>

    {#if data.palegg.length}
      <div class="overflow-x-auto mb-3">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Ansatt</th>
              <th>Type</th>
              <th class="text-right">Trekk</th>
              <th>Gjelder</th>
              <th>KID</th>
              <th>Status</th>
              <th><span class="sr-only">Handlinger</span></th>
            </tr>
          </thead>
          <tbody>
            {#each data.palegg as p (p.id)}
              {@const v = p.versjoner[0]}
              <tr>
                <td class="whitespace-nowrap">{p.navn}</td>
                <td class="text-xs">{kindLabel(p.type)}</td>
                <td class="text-right whitespace-nowrap">{sizeText(v)}</td>
                <td class="whitespace-nowrap text-xs">
                  {formatDate(v.gjelder_fra)}{v.gjelder_til ? " – " + formatDate(v.gjelder_til) : ""}
                </td>
                <td class="font-mono text-xs">{v.kid ?? "selskapets"}</td>
                <td>
                  <span class="badge badge-sm" class:badge-warning={p.status !== "aktiv" && p.status !== "stoppet"}>
                    {p.status}
                  </span>
                  <span class="text-xs opacity-60">v{v.versjon}</span>
                </td>
                <td class="whitespace-nowrap">
                  <button class="btn btn-ghost btn-xs" aria-label={"Brevet for " + p.navn} onclick={() => download(v.brev_url, v.brev_filnavn)}>
                    Brevet
                  </button>
                  {#if p.status.includes("venter på attestering")}
                    <button class="btn btn-ghost btn-xs" aria-label={"Godkjenn pålegget for " + p.navn} onclick={() => attest(p, v, true)}>Godkjenn</button>
                    <button class="btn btn-ghost btn-xs" aria-label={"Avvis pålegget for " + p.navn} onclick={() => attest(p, v, false)}>Avvis</button>
                  {/if}
                  {#if p.status !== "stoppet"}
                    <button class="btn btn-ghost btn-xs" aria-label={"Endre pålegget for " + p.navn} onclick={() => edit(p)}>Endre</button>
                    <button class="btn btn-ghost btn-xs text-error" aria-label={"Stopp pålegget for " + p.navn} onclick={() => stop(p)}>Stopp</button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    {#if draft}
      <form class="border border-base-300 rounded-box p-3 mb-3 grid gap-3 max-w-3xl" onsubmit={save}>
        <h3 class="font-semibold">{draft.editing ? "Ny versjon av pålegget" : "Registrer pålegg fra brev"}</h3>
        <div class="grid sm:grid-cols-2 gap-3">
          <label class="flex flex-col gap-1 text-sm">
            Ansatt
            <select class="select select-sm" required disabled={!!draft.editing} bind:value={draft.employeeId}>
              <option value="" disabled>Velg ansatt</option>
              {#each employees as a (a.id)}<option value={a.id}>{a.navn}</option>{/each}
            </select>
          </label>
          <label class="flex flex-col gap-1 text-sm">
            Type (står i brevet)
            <select class="select select-sm" disabled={!!draft.editing} bind:value={draft.kind}>
              {#each KINDS as t (t[0])}<option value={t[0]}>{t[1]}</option>{/each}
            </select>
          </label>
        </div>
        <fieldset class="grid sm:grid-cols-2 gap-3">
          <legend class="text-sm mb-1">Trekk per måned</legend>
          <div class="flex gap-3 text-sm">
            <label class="label cursor-pointer gap-2">
              <input type="radio" class="radio radio-sm" value="amount" bind:group={draft.sizeKind} /> Beløp
            </label>
            <label class="label cursor-pointer gap-2" class:opacity-50={draft.kind === "ovrige"}>
              <input type="radio" class="radio radio-sm" value="percent" disabled={draft.kind === "ovrige"} bind:group={draft.sizeKind} />
              Prosent
            </label>
          </div>
          {#if draft.sizeKind === "amount" || draft.kind === "ovrige"}
            <label class="flex flex-col gap-1 text-sm">
              Beløp per måned (kr)
              <input class="input input-sm text-right" inputmode="decimal" required bind:value={draft.amount} />
            </label>
          {:else}
            <label class="flex flex-col gap-1 text-sm">
              Prosent av bruttolønn (høyst to desimaler)
              <input class="input input-sm text-right" inputmode="decimal" required bind:value={draft.percent} />
            </label>
          {/if}
        </fieldset>
        <div class="grid sm:grid-cols-2 gap-3">
          <label class="flex flex-col gap-1 text-sm">
            Gjelder fra
            <input type="date" class="input input-sm" required bind:value={draft.from} />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            Gjelder til (valgfri)
            <input type="date" class="input input-sm" bind:value={draft.to} />
          </label>
        </div>
        {#if draft.kind === "skatt"}
          <p class="text-sm opacity-80">
            Utleggstrekk skatt betales samlet med selskapets egen KID for utleggstrekk skatt, ikke en KID
            per ansatt. Den legges inn under «Innstillinger» nedenfor.
          </p>
        {:else}
          <div class="grid sm:grid-cols-3 gap-3">
            <label class="flex flex-col gap-1 text-sm">
              KID
              <input class="input input-sm font-mono" inputmode="numeric" required bind:value={draft.kid} />
            </label>
            <label class="flex flex-col gap-1 text-sm">
              Mottakers org.nr.
              <input class="input input-sm font-mono" inputmode="numeric" required bind:value={draft.payeeOrgnumber} />
            </label>
            <label class="flex flex-col gap-1 text-sm">
              Mottakers kontonummer
              <input class="input input-sm font-mono" inputmode="numeric" required bind:value={draft.payeeAccount} />
            </label>
          </div>
        {/if}
        {#if draft.kind === "ovrige"}
          <label class="flex flex-col gap-1 text-sm max-w-xs">
            Betalingsfrist (dager etter utbetaling)
            <input type="number" min="0" max="365" class="input input-sm" required bind:value={draft.dueDays} />
          </label>
        {/if}
        <label class="flex flex-col gap-1 text-sm">
          Brevet (PDF eller bilde){draft.editing ? " — valgfritt, ellers beholdes forrige" : ""}
          <input
            type="file"
            accept="application/pdf,image/*"
            class="file-input file-input-sm"
            required={!draft.editing}
            onchange={(e) => (draft.file = e.currentTarget.files?.[0] ?? null)}
          />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          Notat (valgfritt)
          <input class="input input-sm" bind:value={draft.note} />
        </label>
        <div class="flex gap-2">
          <button class="btn btn-primary btn-sm" disabled={busy}>
            {draft.editing ? "Lagre ny versjon" : "Registrer pålegget"}
          </button>
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => (draft = null)}>Avbryt</button>
        </div>
      </form>
    {:else}
      <button class="btn btn-sm w-fit mb-3" onclick={() => (draft = emptyDraft())}>Registrer pålegg fra brev</button>
    {/if}

    <details class="text-sm" open={!!settings}>
      <summary class="cursor-pointer">Innstillinger for utleggstrekk</summary>
      {#if settings}
        <form class="grid gap-3 mt-2 max-w-3xl" onsubmit={saveSettings}>
          <div class="grid sm:grid-cols-2 gap-3">
            <label class="flex flex-col gap-1">
              Konto for utleggstrekk (leverandørreskontro)
              <input class="input input-sm font-mono" placeholder="2610 velges automatisk" bind:value={settings.konto} />
            </label>
            <label class="flex flex-col gap-1">
              Gjelder fra
              <input type="date" class="input input-sm" required bind:value={settings.gjelder_fra} />
            </label>
          </div>
          <p class="opacity-80">Fra Skatteetatens betalingsinformasjon for utleggstrekk skatt:</p>
          <div class="grid sm:grid-cols-3 gap-3">
            <label class="flex flex-col gap-1">
              KID for utleggstrekk skatt
              <input class="input input-sm font-mono" inputmode="numeric" bind:value={settings.skatt_kid} />
            </label>
            <label class="flex flex-col gap-1">
              Kontonummer
              <input class="input input-sm font-mono" inputmode="numeric" bind:value={settings.skatt_konto} />
            </label>
            <label class="flex flex-col gap-1">
              Mottakers org.nr.
              <input class="input input-sm font-mono" inputmode="numeric" bind:value={settings.skatt_mottaker_orgnr} />
            </label>
          </div>
          <div class="flex gap-2">
            <button class="btn btn-primary btn-sm">Lagre innstillingene</button>
            <button type="button" class="btn btn-ghost btn-sm" onclick={() => (settings = null)}>Avbryt</button>
          </div>
        </form>
      {:else}
        <dl class="grid grid-cols-2 gap-x-4 gap-y-1 mt-2 max-w-xl">
          <dt class="opacity-70">Konto</dt>
          <dd>{data.innstillinger?.konto ?? "velges ved første trekk (2610)"}</dd>
          <dt class="opacity-70">KID skatt</dt>
          <dd class="font-mono">{data.innstillinger?.skatt_kid ?? "mangler"}</dd>
          <dt class="opacity-70">Konto skatt</dt>
          <dd class="font-mono">{data.innstillinger?.skatt_konto ?? "mangler"}</dd>
          <dt class="opacity-70">Mottaker skatt</dt>
          <dd class="font-mono">{data.innstillinger?.skatt_mottaker_orgnr ?? "mangler"}</dd>
        </dl>
        <button class="btn btn-ghost btn-sm mt-2" onclick={openSettings}>Endre innstillingene</button>
      {/if}
    </details>
  </Card>
{/if}
