<script>
  // Ny faktura: kunde, datoer, så linjene — så mange som trengs — med
  // summen regnet mens man skriver. En utstedt faktura kan aldri endres
  // (feil rettes med kreditnota), så utstedelsen viser en oppsummering
  // først og lander på fakturaen med «Send til kunde» som neste steg.
  import { tick, untrack } from "svelte";
  import { api, post } from "../../lib/api.js";
  import { kr, parseKr, today, minutterTilTimer, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import DimSelect from "../../components/DimSelect.svelte";
  import { VAT_CODES, lineNetOre, vatOfBase, milliHours } from "./invoiceMath.js";

  // `?kunde=<party number>` (from a customer's page) preselects that
  // customer; otherwise nothing is preselected on purpose.
  let { companyId, query = "" } = $props();
  const presetParty = untrack(() => new URLSearchParams(query).get("kunde"));

  const PAYMENT_DAYS = 14;

  let parties = $state(null);
  let products = $state([]);
  let dims = $state([]);
  let currencies = $state([]);
  let unbilled = $state([]);

  $effect(() => {
    const c = "/companies/" + companyId;
    api(c + "/parties?kind=kunde")
      .then((r) => {
        parties = r.parties;
        const preset = r.parties.find((p) => p.party_no === presetParty);
        if (preset && !customer) customer = partyLabel(preset);
      })
      .catch((e) => toast(e.message, false));
    api(c + "/products").then((r) => (products = r.products.filter((p) => p.aktiv))).catch(() => {});
    api(c + "/dimensions").then((r) => (dims = r.dimensions)).catch(() => {});
    api(c + "/currency/rates")
      .then((r) => (currencies = [...new Set(r.rates.map((x) => x.valuta))]))
      .catch(() => {});
    api(c + "/timesheet/unbilled").then((r) => (unbilled = r.groups)).catch(() => {});
  });

  // ---- Header ----
  let customer = $state("");
  const partyLabel = (p) => p.name + " (" + p.party_no + ")";
  let party = $derived(
    parties?.find((p) => {
      const t = customer.trim().toLowerCase();
      return t && (partyLabel(p).toLowerCase() === t || p.party_no === t || p.name.toLowerCase() === t);
    }) ?? null,
  );

  const addDays = (iso, n) => {
    const d = new Date(iso + "T12:00");
    d.setDate(d.getDate() + n);
    return d.toISOString().slice(0, 10);
  };
  let invoiceDate = $state(today());
  // Due date and delivery date follow the invoice date until the user
  // sets them; after that they are a choice and are left alone.
  let dueTouched = $state(false);
  let dueSet = $state("");
  let dueDate = $derived(dueTouched ? dueSet : addDays(invoiceDate, PAYMENT_DAYS));
  let deliveryTouched = $state(false);
  let deliverySet = $state("");
  let deliveryDate = $derived(deliveryTouched ? deliverySet : invoiceDate);
  let deliveryPlace = $state("");
  // The buyer's own reference (BT-10). Empty = the customer's standing
  // one, which the server applies; shown as the placeholder so it is
  // visible before issuing.
  let buyerReference = $state("");
  let currency = $state("");
  let dueDays = $derived(
    Math.round((Date.parse(dueDate + "T12:00") - Date.parse(invoiceDate + "T12:00")) / 864e5),
  );

  // ---- Lines ----
  let nextKey = 1;
  const blankLine = () => ({
    key: nextKey++,
    product: "",
    description: "",
    quantity: "1",
    price: "",
    vat: "3",
    avdeling: "",
    prosjekt: "",
  });
  let lines = $state([blankLine()]);
  let descInputs = $state({});

  function pickProduct(line) {
    const p = products.find((x) => x.nummer === line.product);
    if (!p) return;
    line.description = p.navn;
    line.price = kr(p.salgspris_ore);
    line.vat = p.vat_code || "3";
  }

  async function addLine(focus = true) {
    lines.push(blankLine());
    if (!focus) return;
    await tick();
    descInputs[lines.at(-1).key]?.focus();
  }

  async function removeLine(i) {
    lines.splice(i, 1);
    if (lines.length === 0) lines.push(blankLine());
    await tick();
    descInputs[lines[Math.min(i, lines.length - 1)].key]?.focus();
  }

  // Enter moves down the invoice like a spreadsheet: to the next line,
  // or a new one after the last. Ctrl/Cmd+Enter issues.
  function lineKey(e, i) {
    if (e.key !== "Enter" || e.isComposing) return;
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) return issue();
    if (i === lines.length - 1) addLine();
    else descInputs[lines[i + 1].key]?.focus();
  }

  const isBlank = (l) => !l.product && !l.description.trim() && !l.price.trim();

  // Parsed lines with their errors; blank lines are ignored.
  let parsed = $derived(
    lines.map((l) => {
      if (isBlank(l)) return { blank: true, errors: {} };
      const errors = {};
      const quantity = Number(l.quantity.replace(/\s/g, "").replace(",", "."));
      if (!isFinite(quantity) || quantity === 0) errors.quantity = "Skriv et antall, f.eks. 1 eller 2,5";
      let priceOre = null;
      try {
        priceOre = parseKr(l.price);
        if (!l.price.trim()) throw new Error();
      } catch {
        errors.price = "Skriv en pris i kroner, f.eks. 1250 eller 99,50";
      }
      if (!l.description.trim()) errors.description = "Skriv hva kunden betaler for";
      const quantityMilli = Math.round(quantity * 1000);
      const net = errors.quantity || errors.price ? 0 : lineNetOre(quantityMilli, priceOre);
      return { blank: false, errors, quantityMilli, priceOre, net, vat: vatOfBase(net, VAT_CODES[l.vat]?.bp ?? 0) };
    }),
  );

  // ---- Hours ----
  let pickedHours = $state([]);
  const hourKey = (g, p) => g + ":" + p.person_id;
  let hourLines = $derived(
    currency
      ? []
      : unbilled
          .map((g, gi) => {
            const picked = (g.personer || []).filter((p) => pickedHours.includes(hourKey(gi, p)));
            if (!picked.length) return null;
            const minutes = picked.reduce((s, p) => s + p.minutter, 0);
            const net = lineNetOre(milliHours(minutes), g.timesats_ore);
            return {
              entryIds: picked.flatMap((p) => p.entry_ids),
              text: "Timer" + (g.prosjekt ? ", prosjekt " + g.prosjekt : "") + " (" + minutterTilTimer(minutes) + " t)",
              net,
              vat: vatOfBase(net, VAT_CODES["3"].bp), // hour lines are always code 3
              code: "3",
            };
          })
          .filter(Boolean),
  );

  // ---- Totals, mirroring regnmed-core::invoice::compute ----
  let totals = $derived.by(() => {
    const byCode = {};
    let net = 0;
    lines.forEach((l, i) => {
      const p = parsed[i];
      if (p.blank) return;
      net += p.net;
      byCode[l.vat] = (byCode[l.vat] || 0) + p.vat;
    });
    for (const h of hourLines) {
      net += h.net;
      byCode[h.code] = (byCode[h.code] || 0) + h.vat;
    }
    const vat = Object.values(byCode).reduce((a, b) => a + b, 0);
    // A code this page has no rate for is computed by the server on issue;
    // the page says so instead of showing it as 0 %.
    const unknown = lines.some((l, i) => !parsed[i].blank && !VAT_CODES[l.vat]);
    return { net, vat, gross: net + vat, byCode, unknown };
  });

  // ---- Issue ----
  let submitted = $state(false);
  let busy = $state(false);
  let formErrors = $derived.by(() => {
    const list = [];
    if (!party) list.push(["customer", parties?.length ? "Velg en kunde fra listen" : "Opprett en kunde først"]);
    if (dueDate < invoiceDate) list.push(["due", "Forfall kan ikke være før fakturadato"]);
    const real = parsed.filter((p) => !p.blank);
    if (!real.length && !hourLines.length) list.push(["line-0-description", "Legg til minst én linje"]);
    parsed.forEach((p, i) => {
      for (const [field, msg] of Object.entries(p.errors)) list.push(["line-" + i + "-" + field, "Linje " + (i + 1) + ": " + msg]);
    });
    return list;
  });
  const fieldError = (id) => (submitted ? formErrors.find(([f]) => f === id)?.[1] : undefined);

  async function issue() {
    submitted = true;
    if (busy) return;
    if (formErrors.length) {
      await tick();
      document.getElementById(formErrors[0][0])?.focus();
      return;
    }
    const count = parsed.filter((p) => !p.blank).length + hourLines.length;
    const ok = await bekreft(
      "Faktura til " + party.name + ", " + count + (count === 1 ? " linje" : " linjer") + ", " +
        kr(totals.gross) + (currency ? " " + currency : " kr") +
        (totals.unknown ? " pluss mva som regnes ved utstedelse" : " inkl. mva") + ", forfall om " + dueDays + " dager. En utstedt faktura kan ikke endres; feil rettes med kreditnota.",
      { tittel: "Utstede fakturaen?", ok: "Utsted faktura" },
    );
    if (!ok) return;
    busy = true;
    try {
      const issued = await post("/companies/" + companyId + "/invoices", {
        party_no: party.party_no,
        invoice_date: invoiceDate,
        due_date: dueDate,
        leveringsdato: deliveryDate,
        leveringssted: deliveryPlace.trim() || null,
        buyer_reference: buyerReference.trim() || null,
        valuta: currency || null,
        lines: lines
          .map((l, i) => [l, parsed[i]])
          .filter(([, p]) => !p.blank)
          .map(([l, p]) => ({
            produkt: l.product || null,
            description: l.description.trim(),
            quantity_milli: p.quantityMilli,
            unit_price_ore: p.priceOre,
            vat_code: l.vat || null,
            avdeling: l.avdeling || null,
            prosjekt: l.prosjekt || null,
          })),
        timer_entry_ids: hourLines.length ? hourLines.flatMap((h) => h.entryIds) : null,
      });
      toast("Faktura " + issued.invoice_no + " er utstedt", true);
      done = true;
      location.hash = "#/c/" + companyId + "/faktura/" + issued.invoice_id;
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  // Something typed that issuing has not saved. Leaving asks first; a
  // reload or a closed tab gets the browser's own warning.
  let done = false;
  let dirty = $derived(
    (customer.trim() !== "" && party?.party_no !== presetParty) ||
      lines.some((l) => !isBlank(l)) ||
      buyerReference.trim() !== "" ||
      pickedHours.length > 0,
  );
  function beforeUnload(e) {
    if (dirty && !done) e.preventDefault();
  }
  async function leave(e) {
    if (!dirty || done) return;
    e.preventDefault();
    const href = e.currentTarget.getAttribute("href");
    const ok = await bekreft("Det du har skrevet på fakturaen, blir ikke lagret.", {
      tittel: "Forkaste fakturaen?",
      ok: "Forkast",
      avbryt: "Fortsett å skrive",
      farlig: true,
    });
    if (ok) location.hash = href;
  }

  const vatLabel = (code) => VAT_CODES[code]?.label ?? "kode " + code;

  // Header fields: Enter must not submit an invoice that cannot be
  // undone. Lines handle Enter themselves; Ctrl/Cmd+Enter issues anywhere.
  function formKey(e) {
    if (e.key !== "Enter" || e.defaultPrevented || e.isComposing) return;
    if (e.target.tagName === "BUTTON" || e.target.tagName === "TEXTAREA") return;
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) issue();
  }
</script>

<svelte:window onbeforeunload={beforeUnload} />

<nav class="mb-3">
  <a class="link link-hover text-sm" href={"#/c/" + companyId + "/faktura"} onclick={leave}>← Alle fakturaer</a>
</nav>
<h1 class="text-2xl font-bold mb-4">Ny faktura</h1>

{#if parties === null}
  <div class="flex justify-center p-12"><span class="loading loading-spinner loading-lg"></span></div>
{:else if parties.length === 0}
  <div class="alert alert-info">
    <span>Du trenger en kunde før du kan fakturere.</span>
    <a class="btn btn-sm" href={"#/c/" + companyId + "/kunder/ny"}>Opprett kunde</a>
  </div>
{:else}
  <form
    novalidate
    onkeydown={formKey}
    onsubmit={(e) => {
      e.preventDefault();
      issue();
    }}
  >
    <div class="card border border-base-300 bg-base-100 mb-4">
      <div class="card-body grid gap-4 sm:grid-cols-2">
        <label class="fieldset sm:col-span-2">
          <span class="fieldset-legend">Kunde</span>
          <input
            id="customer"
            class="input w-full {fieldError('customer') ? 'input-error' : ''}"
            list="customers"
            autocomplete="off"
            placeholder="Søk på navn eller kundenummer"
            bind:value={customer}
            aria-invalid={!!fieldError("customer")}
            aria-describedby="customer-help"
          />
          <datalist id="customers">
            {#each parties as p (p.party_no)}
              <option value={partyLabel(p)}></option>
            {/each}
          </datalist>
          <span id="customer-help" class="label whitespace-normal {fieldError('customer') ? 'text-error' : ''}">
            {#if fieldError("customer")}
              {fieldError("customer")}
            {:else if party}
              {party.email ? "Sendes til " + party.email : "Kunden har ingen e-postadresse ennå"}
            {:else}
              <a class="link" href={"#/c/" + companyId + "/kunder/ny"}>Ny kunde?</a>
            {/if}
          </span>
        </label>

        <label class="fieldset">
          <span class="fieldset-legend">Fakturadato</span>
          <input type="date" class="input w-full" bind:value={invoiceDate} />
        </label>
        <label class="fieldset">
          <span class="fieldset-legend">Forfall</span>
          <input
            id="due"
            type="date"
            class="input w-full {fieldError('due') ? 'input-error' : ''}"
            value={dueDate}
            oninput={(e) => {
              dueSet = e.currentTarget.value;
              dueTouched = true;
            }}
            aria-describedby="due-help"
            aria-invalid={!!fieldError("due")}
          />
          <span id="due-help" class="label whitespace-normal {fieldError('due') ? 'text-error' : ''}">
            {fieldError("due") || dueDays + " dagers betalingsfrist"}
          </span>
        </label>

        <label class="fieldset sm:col-span-2">
          <span class="fieldset-legend">Deres referanse (valgfritt)</span>
          <input
            class="input w-full"
            maxlength="70"
            placeholder={party?.buyer_reference || "F.eks. bestillingsnummer eller kontaktperson"}
            bind:value={buyerReference}
            aria-describedby="buyer-reference-help"
          />
          <span id="buyer-reference-help" class="label whitespace-normal">
            {party?.buyer_reference && !buyerReference.trim()
              ? "Tomt felt bruker kundens faste referanse"
              : "Kundens egen referanse. Offentlige og større kunder krever den på e-faktura."}
          </span>
        </label>

        <details class="sm:col-span-2">
          <summary class="cursor-pointer text-sm">
            Levering: {deliveryDate === invoiceDate ? "samme dag som fakturadato" : formatDate(deliveryDate)}{deliveryPlace.trim() ? ", " + deliveryPlace.trim() : ""}{currency ? " · valuta " + currency : ""}
          </summary>
          <div class="grid gap-4 sm:grid-cols-3 mt-3">
            <label class="fieldset">
              <span class="fieldset-legend">Leveringsdato</span>
              <input
                type="date"
                class="input w-full"
                value={deliveryDate}
                oninput={(e) => {
                  deliverySet = e.currentTarget.value;
                  deliveryTouched = true;
                }}
              />
            </label>
            <label class="fieldset">
              <span class="fieldset-legend">Leveringssted (valgfritt)</span>
              <input class="input w-full" placeholder="F.eks. Lagerveien 3, Oslo" bind:value={deliveryPlace} />
            </label>
            {#if currencies.length}
              <label class="fieldset">
                <span class="fieldset-legend">Valuta</span>
                <select class="select w-full" bind:value={currency}>
                  <option value="">NOK</option>
                  {#each currencies as c (c)}
                    <option value={c}>{c}</option>
                  {/each}
                </select>
              </label>
            {/if}
          </div>
        </details>
      </div>
    </div>

    <div class="card border border-base-300 bg-base-100 mb-4">
      <div class="card-body">
        <h2 class="card-title">Linjer</h2>
        <p class="text-sm opacity-70 -mt-1">
          Enter går til neste linje og lager en ny etter den siste.
        </p>

        <!-- Column headers for wide screens; on a phone each line is its
             own stacked group with visible labels. -->
        <div
          class="hidden sm:grid gap-2 text-sm opacity-70 px-1 {products.length
            ? 'sm:grid-cols-[10rem_minmax(0,1fr)_5rem_7rem_8rem_7rem_2.5rem]'
            : 'sm:grid-cols-[minmax(0,1fr)_5rem_7rem_8rem_7rem_2.5rem]'}"
          aria-hidden="true"
        >
          {#if products.length}<span>Produkt</span>{/if}
          <span>Beskrivelse</span><span class="text-right">Antall</span><span class="text-right">Pris (kr)</span>
          <span>Mva</span><span class="text-right">Beløp</span><span></span>
        </div>

        {#each lines as line, i (line.key)}
          <fieldset
            class="grid gap-2 border-b border-base-200 sm:border-0 pb-3 sm:pb-0 {products.length
              ? 'sm:grid-cols-[10rem_minmax(0,1fr)_5rem_7rem_8rem_7rem_2.5rem]'
              : 'sm:grid-cols-[minmax(0,1fr)_5rem_7rem_8rem_7rem_2.5rem]'} sm:items-start"
          >
            <legend class="text-sm font-medium sm:sr-only mb-1">Linje {i + 1}</legend>
            {#if products.length}
              <label>
                <span class="text-xs opacity-70 sm:sr-only">Produkt</span>
                <select class="select w-full" bind:value={line.product} onchange={() => pickProduct(line)}>
                  <option value="">Fritekst</option>
                  {#each products as p (p.nummer)}
                    <option value={p.nummer}>{p.navn}</option>
                  {/each}
                </select>
              </label>
            {/if}
            <label>
              <span class="text-xs opacity-70 sm:sr-only">Beskrivelse</span>
              <input
                id={"line-" + i + "-description"}
                class="input w-full {fieldError('line-' + i + '-description') ? 'input-error' : ''}"
                bind:value={line.description}
                bind:this={descInputs[line.key]}
                onkeydown={(e) => lineKey(e, i)}
                aria-invalid={!!fieldError("line-" + i + "-description")}
              />
            </label>
            <label>
              <span class="text-xs opacity-70 sm:sr-only">Antall</span>
              <input
                id={"line-" + i + "-quantity"}
                class="input w-full text-right {fieldError('line-' + i + '-quantity') ? 'input-error' : ''}"
                inputmode="decimal"
                bind:value={line.quantity}
                onkeydown={(e) => lineKey(e, i)}
                aria-invalid={!!fieldError("line-" + i + "-quantity")}
              />
            </label>
            <label>
              <span class="text-xs opacity-70 sm:sr-only">Pris (kr)</span>
              <input
                id={"line-" + i + "-price"}
                class="input w-full text-right {fieldError('line-' + i + '-price') ? 'input-error' : ''}"
                inputmode="decimal"
                bind:value={line.price}
                onkeydown={(e) => lineKey(e, i)}
                aria-invalid={!!fieldError("line-" + i + "-price")}
              />
            </label>
            <label>
              <span class="text-xs opacity-70 sm:sr-only">Mva</span>
              <select class="select w-full" bind:value={line.vat}>
                {#if !VAT_CODES[line.vat]}
                  <option value={line.vat}>Kode {line.vat} (fra produktet)</option>
                {/if}
                {#each Object.entries(VAT_CODES) as [code, v] (code)}
                  <option value={code}>{v.label}</option>
                {/each}
              </select>
            </label>
            <output class="self-center text-right tabular-nums" aria-label={"Beløp linje " + (i + 1)}>
              {parsed[i].blank ? "" : kr(parsed[i].net)}
            </output>
            <button
              type="button"
              class="btn btn-ghost btn-square btn-sm self-center justify-self-end"
              aria-label={"Fjern linje " + (i + 1)}
              title="Fjern linjen"
              onclick={() => removeLine(i)}
            >
              ✕
            </button>
            {#if dims.length}
              <div class="flex gap-2 sm:col-span-full">
                <DimSelect {dims} kind="avdeling" label={"Avdeling, linje " + (i + 1)} bind:value={line.avdeling} />
                <DimSelect {dims} kind="prosjekt" label={"Prosjekt, linje " + (i + 1)} bind:value={line.prosjekt} />
              </div>
            {/if}
          </fieldset>
        {/each}

        <div>
          <button type="button" class="btn btn-sm" onclick={() => addLine()}>+ Legg til linje</button>
        </div>

        {#if unbilled.length && !currency}
          <fieldset class="border border-base-300 rounded-box p-3 mt-2">
            <legend class="text-sm font-medium px-1">Ta med ufakturerte timer</legend>
            {#each unbilled as g, gi (gi)}
              {#each g.personer || [] as p (hourKey(gi, p))}
                <label class="flex items-center gap-2 text-sm cursor-pointer py-1">
                  <input
                    type="checkbox"
                    class="checkbox checkbox-sm"
                    checked={pickedHours.includes(hourKey(gi, p))}
                    onchange={(e) =>
                      (pickedHours = e.currentTarget.checked
                        ? [...pickedHours, hourKey(gi, p)]
                        : pickedHours.filter((k) => k !== hourKey(gi, p)))}
                  />
                  <span>
                    {p.navn}, {g.prosjekt || "uten prosjekt"}: {minutterTilTimer(p.minutter)} t à {kr(g.timesats_ore)}
                  </span>
                </label>
              {/each}
            {/each}
          </fieldset>
        {/if}
        {#each hourLines as h, hi (hi)}
          <div class="flex justify-between text-sm px-1">
            <span>{h.text}</span><span class="tabular-nums">{kr(h.net)}</span>
          </div>
        {/each}
      </div>
    </div>

    <div class="card border border-base-300 bg-base-100 mb-6">
      <div class="card-body sm:flex-row sm:items-end sm:justify-between gap-4">
        <dl class="grid grid-cols-[auto_auto] gap-x-6 gap-y-1 text-sm sm:order-2" aria-live="polite">
          <dt class="opacity-70">Netto</dt>
          <dd class="text-right tabular-nums">{kr(totals.net)}</dd>
          {#each Object.entries(totals.byCode).filter(([c, v]) => v && VAT_CODES[c]) as [code, v] (code)}
            <dt class="opacity-70">Mva {vatLabel(code)}</dt>
            <dd class="text-right tabular-nums">{kr(v)}</dd>
          {/each}
          {#if totals.unknown}
            <dt class="opacity-70">Mva for øvrige koder</dt>
            <dd class="text-right">regnes ved utstedelse</dd>
          {/if}
          <dt class="font-semibold text-base">Å betale{currency ? " (" + currency + ")" : ""}</dt>
          <dd class="text-right tabular-nums font-bold text-base">
            {kr(totals.gross)}{totals.unknown ? " + mva" : ""}
          </dd>
        </dl>
        <div class="flex flex-col gap-2 sm:order-1">
          {#if submitted && formErrors.length}
            <div class="text-error text-sm" role="alert">
              {#each formErrors as [id, msg] (id + msg)}
                <button type="button" class="block link text-left" onclick={() => document.getElementById(id)?.focus()}>{msg}</button>
              {/each}
            </div>
          {/if}
          <div class="flex flex-wrap gap-2">
            <button class="btn btn-primary" disabled={busy} aria-keyshortcuts="Control+Enter Meta+Enter">
              {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
              Utsted faktura
            </button>
            <a class="btn btn-ghost" href={"#/c/" + companyId + "/faktura"} onclick={leave}>Avbryt</a>
          </div>
        </div>
      </div>
    </div>
  </form>
{/if}
