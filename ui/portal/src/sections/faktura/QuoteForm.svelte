<script>
  // A tilbud with as many lines as it needs, new or edited. A tilbud is
  // outside the ledger and editable until accepted or rejected, so this
  // is an ordinary form — no confirmation, the customer has seen nothing
  // until it is sent.
  import { tick, untrack } from "svelte";
  import { post, send } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { VAT_CODES, lineNetOre, vatOfBase } from "./invoiceMath.js";

  // `quote`: null for a new tilbud, else { id, doc_no, party_no, party_name, doc_date, lines }.
  let { companyId, parties, products, quote = null, onDone, onCancel } = $props();

  const editing = untrack(() => !!quote);
  let party = $state(untrack(() => quote?.party_no || ""));
  let docDate = $state(untrack(() => quote?.doc_date || today()));

  let nextKey = 1;
  const blank = () => ({ key: nextKey++, product: "", description: "", quantity: "1", price: "", vat: "3", account: null });
  const fromServer = (l) => ({
    key: nextKey++,
    product: l.produkt || "",
    description: l.description,
    quantity: String(l.quantity_milli / 1000).replace(".", ","),
    price: kr(l.unit_price_ore),
    vat: l.vat_code || "",
    account: l.account,
  });
  let lines = $state(untrack(() => (quote ? quote.lines.map(fromServer) : [blank()])));
  let descInputs = $state({});

  function pickProduct(line) {
    const p = products.find((x) => x.nummer === line.product);
    if (!p) return;
    line.description = p.navn;
    line.price = kr(p.salgspris_ore);
    line.vat = p.vat_code || "3";
    line.account = null;
  }

  async function addLine() {
    lines.push(blank());
    await tick();
    descInputs[lines.at(-1).key]?.focus();
  }

  function removeLine(i) {
    lines.splice(i, 1);
    if (!lines.length) lines.push(blank());
  }

  const isBlank = (l) => !l.product && !l.description.trim() && !l.price.trim();

  let parsed = $derived(
    lines.map((l) => {
      if (isBlank(l)) return { blank: true };
      const quantity = Number(l.quantity.replace(/\s/g, "").replace(",", "."));
      let priceOre = null;
      try {
        if (l.price.trim()) priceOre = parseKr(l.price);
      } catch {
        return { error: "Linje: skriv prisen i kroner, f.eks. 1250 eller 99,50" };
      }
      if (!isFinite(quantity) || quantity === 0) return { error: "Linje: skriv et antall, f.eks. 1 eller 2,5" };
      if (!l.product && (!l.description.trim() || priceOre === null))
        return { error: "En linje uten produkt trenger beskrivelse og pris" };
      const quantityMilli = Math.round(quantity * 1000);
      const net = priceOre === null ? 0 : lineNetOre(quantityMilli, priceOre);
      return { quantityMilli, priceOre, net, vat: vatOfBase(net, VAT_CODES[l.vat]?.bp ?? 0) };
    }),
  );
  let totals = $derived(
    parsed.filter((p) => !p.blank && !p.error).reduce((t, p) => ({ net: t.net + p.net, vat: t.vat + p.vat }), { net: 0, vat: 0 }),
  );

  let busy = $state(false);
  async function save() {
    if (!party) return toast("Velg hvilken kunde tilbudet er til", false);
    const problem = parsed.find((p) => p.error);
    if (problem) return toast(problem.error, false);
    const real = lines.map((l, i) => [l, parsed[i]]).filter(([, p]) => !p.blank);
    if (!real.length) return toast("Legg til minst én linje", false);
    const body = {
      doc_date: docDate,
      lines: real.map(([l, p]) => ({
        produkt: l.product || null,
        description: l.description.trim() || null,
        account: l.account || null,
        quantity_milli: p.quantityMilli,
        unit_price_ore: p.priceOre,
        vat_code: l.vat || null,
      })),
    };
    busy = true;
    try {
      if (editing) {
        await send("PUT", "/companies/" + companyId + "/quotes/" + quote.id, body);
        toast("Tilbud T-" + quote.doc_no + " er lagret", true);
      } else {
        const made = await post("/companies/" + companyId + "/quotes", { ...body, party_no: party });
        toast("Tilbud T-" + made.doc_no + " er laget", true);
      }
      onDone();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<form
  class="border border-base-300 rounded-box p-4 mb-4"
  novalidate
  onsubmit={(e) => {
    e.preventDefault();
    save();
  }}
>
  <h3 class="font-semibold mb-3">{editing ? "Endre tilbud T-" + quote.doc_no : "Nytt tilbud"}</h3>
  <div class="grid gap-3 sm:grid-cols-2 mb-3">
    <label class="flex flex-col gap-1 text-sm">
      Kunde
      {#if editing}
        <input class="input input-sm" value={quote.party_name} disabled />
      {:else}
        <select class="select select-sm" bind:value={party}>
          <option value="" disabled>Velg kunde</option>
          {#each parties as p (p.party_no)}
            <option value={p.party_no}>{p.name} ({p.party_no})</option>
          {/each}
        </select>
      {/if}
    </label>
    <label class="flex flex-col gap-1 text-sm">
      Dato
      <input type="date" class="input input-sm" bind:value={docDate} />
    </label>
  </div>

  {#each lines as line, i (line.key)}
    <fieldset class="grid gap-2 mb-2 sm:grid-cols-[repeat(auto-fit,minmax(7rem,1fr))] items-end border-b border-base-200 pb-2">
      <legend class="sr-only">Linje {i + 1}</legend>
      {#if products.length}
        <label class="flex flex-col gap-1 text-xs">
          Produkt
          <select class="select select-sm" bind:value={line.product} onchange={() => pickProduct(line)}>
            <option value="">Fritekst</option>
            {#each products as p (p.nummer)}
              <option value={p.nummer}>{p.navn}</option>
            {/each}
          </select>
        </label>
      {/if}
      <label class="flex flex-col gap-1 text-xs sm:col-span-2">
        Beskrivelse
        <input class="input input-sm" bind:value={line.description} bind:this={descInputs[line.key]} />
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Antall
        <input class="input input-sm text-right" inputmode="decimal" bind:value={line.quantity} />
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Pris (kr)
        <input class="input input-sm text-right" inputmode="decimal" bind:value={line.price} />
      </label>
      <label class="flex flex-col gap-1 text-xs">
        Mva
        <select class="select select-sm" bind:value={line.vat}>
          {#if line.product}<option value="">Fra produktet</option>{/if}
          {#each Object.entries(VAT_CODES) as [code, v] (code)}
            <option value={code}>{v.label}</option>
          {/each}
        </select>
      </label>
      <div class="flex items-center justify-between gap-2">
        <output class="tabular-nums text-sm" aria-label={"Beløp linje " + (i + 1)}>
          {parsed[i].blank || parsed[i].error ? "" : kr(parsed[i].net)}
        </output>
        <button type="button" class="btn btn-ghost btn-sm btn-square" aria-label={"Fjern linje " + (i + 1)} onclick={() => removeLine(i)}>✕</button>
      </div>
    </fieldset>
  {/each}

  <div class="flex flex-wrap items-center gap-2 mt-3">
    <button type="button" class="btn btn-sm" onclick={addLine}>+ Legg til linje</button>
    <span class="ml-auto text-sm" aria-live="polite">
      Netto <b class="tabular-nums">{kr(totals.net)}</b>, mva <span class="tabular-nums">{kr(totals.vat)}</span>
    </span>
  </div>
  <div class="flex gap-2 mt-3">
    <button class="btn btn-sm btn-primary" disabled={busy}>{editing ? "Lagre tilbudet" : "Lag tilbudet"}</button>
    <button type="button" class="btn btn-sm btn-ghost" onclick={onCancel}>Avbryt</button>
  </div>
</form>
