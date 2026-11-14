<script>
  // A draft claim: everything may still change, nothing is promised until
  // «Send inn». An utlegg shows its receipt (rotate/crop while it is a
  // draft); a trip has a route, vehicle and passengers — the documentation
  // that keeps the allowance trekkfri — and its tolls, ferries and parking
  // as costs of their own, each with its receipt. `expense` is null for a
  // trip not saved yet; the first save gives it an address.
  import { tick, untrack } from "svelte";
  import { api, post, send } from "../../lib/api.js";
  import { kr, oreOrNull, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { previewOf } from "../../lib/preview.js";
  import DimSelect from "../../components/DimSelect.svelte";
  import AddressInput from "../../components/AddressInput.svelte";
  import RouteMap from "../../components/RouteMap.svelte";
  import ReceiptEditor from "./ReceiptEditor.svelte";
  import { parseKm, STATUS, VEHICLES, COST_KINDS } from "./expenseMath.js";

  let { companyId, expense = null, kind = expense?.kind || "utlegg", onchanged } = $props();

  const base = $derived("/companies/" + companyId + "/expenses");
  const list = $derived("#/c/" + companyId + "/utlegg");
  const e = expense || {};
  const isTrip = kind === "kjoring";

  let date = $state(e.dato || (isTrip ? today() : ""));
  let amount = $state(e.belop_ore ? kr(e.belop_ore) : "");
  let description = $state(e.beskrivelse || "");
  let from = $state(e.route_from || "");
  let to = $state(e.route_to || "");
  let roundTrip = $state(!!e.round_trip);
  let km = $state(e.km ? String(e.round_trip ? e.km / 2 : e.km) : "");
  let vehicle = $state(e.vehicle || "bil");
  let passengers = $state([...(e.passengers || [])]);
  let department = $state(e.department || "");
  let project = $state(e.project || "");
  let billable = $state(!!e.billable);
  let claimant = $state("");
  // Paid with a company card: the company's money, nothing to reimburse.
  let card = $state(e.company_card_id || "");
  let cards = $state([]);
  const fromLine = !!e.bank_transaction_id;

  let dims = $state([]);
  let people = $state([]);
  let rate = $state(null);
  let costs = $state([]);
  let busy = $state(false);
  let submitted = $state(false);
  let preview = $state(null);
  let editing = $state(false);
  let receiptVersion = $state(0);
  let receiptName = $derived(expense?.receipt_filename);
  let editButton = $state(null);
  // Leaving the editor gives focus back to where it was opened from.
  async function closeEditor() {
    editing = false;
    await tick();
    editButton?.focus();
  }

  const canFileForOthers = harRett(companyId, "UTLEGG_GODKJENN");

  $effect(() => {
    if (!isTrip) api("/companies/" + companyId + "/cards?usable=true").then((r) => (cards = r.cards)).catch(() => {});
    api("/companies/" + companyId + "/dimensions").then((r) => (dims = r.dimensions)).catch(() => {});
    if (!expense && canFileForOthers) {
      api(base + "/claimants").then((r) => (people = r.people)).catch(() => {});
    }
  });

  function loadCosts() {
    if (isTrip && expense) api(base + "?trip=" + expense.expense_id).then((r) => (costs = r.expenses)).catch(() => {});
  }
  $effect(loadCosts);

  // The receipt, re-read after every replacement.
  $effect(() => {
    receiptVersion;
    if (isTrip || !receiptName) return;
    let url = null;
    api(base + "/" + expense.expense_id + "/receipt")
      .then((r) => r.blob())
      .then(previewOf)
      .then((p) => ((url = p.url || null), (preview = p)))
      .catch(() => (preview = { kind: "none" }));
    return () => url && URL.revokeObjectURL(url);
  });

  // What the receipt says, read by the server (text layer or OCR) and
  // put into the fields that are still empty — or still hold what was
  // read last time, so a re-read after rotating can correct itself, but
  // a value the user typed is never overwritten. Nothing is saved until
  // the user saves: a suggestion is not a fact.
  let reading = $state(false);
  let readResult = $state(null); // { filled, warnings }
  let auto = $state({}); // field -> { value, why }
  const suggestedWhy = (field, value) => (auto[field] && auto[field].value === value ? auto[field].why : null);
  // Runs when the receipt arrives or changes — never as the user types,
  // hence the untrack around everything that reads the fields.
  $effect(() => {
    receiptVersion;
    if (isTrip || !receiptName) return;
    return untrack(readReceipt);
  });
  function readReceipt() {
    const id = expense.expense_id;
    const open = (field, value) => !value.trim() || auto[field]?.value === value;
    if (!open("date", date) && !open("amount", amount) && !open("description", description)) return;
    let current = true;
    reading = true;
    api(base + "/" + id + "/suggestion")
      .then((r) => {
        if (!current) return;
        const why = Object.fromEntries(r.reasons.map((x) => [x.field, x.why]));
        const next = {};
        const fill = (field, value, now, set) => {
          if (value && open(field, now)) {
            set(value);
            next[field] = { value, why: why[field === "amount" ? "amount_ore" : field] };
          }
        };
        if (r.date && r.date <= today()) fill("date", r.date, date, (v) => (date = v));
        if (r.amount_ore > 0) fill("amount", kr(r.amount_ore), amount, (v) => (amount = v));
        fill("description", r.description, description, (v) => (description = v));
        auto = next;
        readResult = { filled: Object.keys(next).length, warnings: r.warnings };
      })
      .catch((err) => current && (readResult = { filled: 0, warnings: [err.message] }))
      .finally(() => current && (reading = false));
    return () => (current = false);
  }

  // A project that is billed by default makes its costs billable too.
  function projectChanged() {
    const p = dims.find((d) => d.kind === "prosjekt" && d.code === project);
    billable = !!p?.fakturerbar_default;
  }

  // The driving distance from the two addresses, once both are filled
  // in. It fills the km field while that is empty or still holds the
  // previous suggestion — never over a number the user typed.
  let route = $state(null); // { available, km, line, from, to } | { error }
  let autoKm = null;
  let routeKey = "";
  $effect(() => {
    if (!isTrip) return;
    const f = from.trim();
    const t = to.trim();
    if (!f || !t) {
      route = null;
      return;
    }
    const key = f + "|" + t;
    const timer = setTimeout(() => {
      routeKey = key;
      api(base + "/route?from=" + encodeURIComponent(f) + "&to=" + encodeURIComponent(t))
        .then((r) => {
          if (routeKey !== key) return;
          route = r;
          if (r.available && (!km.trim() || Number(km) === autoKm)) {
            km = String(r.km);
            autoKm = r.km;
          }
        })
        .catch((e) => routeKey === key && (route = { error: e.message }));
    }, 600);
    return () => clearTimeout(timer);
  });

  let oneWay = $derived(parseKm(km));
  let totalKm = $derived(oneWay === null ? null : roundTrip ? oneWay * 2 : oneWay);
  let amountOre = $derived(oreOrNull(amount));

  let rateFor = "";
  $effect(() => {
    if (!isTrip || !date) return;
    const asked = date + vehicle;
    rateFor = asked;
    rate = null;
    api(base + "/kjoring?dato=" + date + "&vehicle=" + vehicle)
      .then((r) => rateFor === asked && (rate = r))
      .catch((err) => rateFor === asked && (rate = { error: err.message }));
  });
  let named = $derived(passengers.filter((p) => p.trim()).length);
  let estimate = $derived.by(() => {
    if (!totalKm || !rate?.sats_ore_per_km) return null;
    const per = rate.sats_ore_per_km + named * (rate.passenger_rate_ore_per_km || 0);
    const free = rate.trekkfri_ore_per_km + named * (rate.passenger_tax_free_ore_per_km || 0);
    const total = totalKm * per;
    return { total, taxable: total - Math.min(total, totalKm * free), per };
  });

  function body() {
    return {
      dato: date || null,
      beskrivelse: description,
      belop_ore: isTrip ? null : amountOre,
      km: isTrip ? totalKm : null,
      route_from: isTrip ? from : null,
      route_to: isTrip ? to : null,
      round_trip: isTrip && roundTrip,
      vehicle: isTrip ? vehicle : null,
      passengers: isTrip ? passengers.filter((p) => p.trim()) : [],
      department: department || null,
      project: project || null,
      billable,
      company_card_id: isTrip ? null : card || null,
    };
  }

  // Saves; returns the claim's id. A new trip gets one here, ONCE: a
  // submit or a cost that fails after it must not make the next try
  // create a second trip.
  let createdId = null;
  async function save() {
    const id = expense?.expense_id || createdId;
    if (id) {
      await send("PUT", base + "/" + id, body());
      return id;
    }
    const made = await post(base, { ...body(), kind, ...(claimant ? { person_id: claimant } : {}) });
    createdId = made.expense_id;
    return createdId;
  }
  // A trip that now exists belongs at its own address, whatever went
  // wrong after it was created.
  const settle = () => !expense && createdId && (location.hash = list + "/" + createdId);

  async function act(what) {
    if (busy) return;
    if (what === "submit") {
      submitted = true;
      const first = Object.keys(problems).find((k) => problems[k]);
      if (first) return document.getElementById("utkast-" + first)?.focus();
    }
    busy = true;
    try {
      const id = await save();
      if (what === "submit") {
        const r = await post(base + "/" + id + "/submit", {});
        toast(isTrip ? "Kjøringen er sendt inn: " + kr(r.belop_ore) + " kr" : "Utlegget er sendt inn", true);
        if (r.costs_submitted) toast(r.costs_submitted + " kostnad(er) på turen ble sendt inn samtidig", true);
        for (const w of r.costs_waiting || []) toast("Venter fortsatt: " + w, false);
      } else {
        toast("Utkastet er lagret", true);
      }
      if (!expense) settle();
      else onchanged?.();
    } catch (err) {
      toast(err.message, false);
      settle();
    }
    busy = false;
  }

  async function discard() {
    const ok = await bekreft(
      isTrip && costs.some((c) => c.status === "utkast")
        ? "Forkaste kjøringen? Utkast til bompenger og parkering på turen forkastes også."
        : "Forkaste utkastet?",
      { ok: "Forkast", farlig: true },
    );
    if (!ok) return;
    try {
      await post(base + "/" + expense.expense_id + "/discard", {});
      toast("Utkastet er forkastet", true);
      location.hash = list + "/utkast";
    } catch (err) {
      toast(err.message, false);
    }
  }

  async function putReceipt(id, blob, filename) {
    await api(base + "/" + id + "/receipt?filename=" + encodeURIComponent(filename), {
      method: "PUT",
      headers: { "content-type": blob.type || "application/octet-stream" },
      body: blob,
    });
  }

  async function replaceReceipt(blob, filename) {
    try {
      await putReceipt(expense.expense_id, blob, filename);
      await closeEditor();
      toast("Kvitteringen er byttet", true);
      receiptVersion += 1;
      onchanged?.();
    } catch (err) {
      toast(err.message, false);
    }
  }

  // A cost of the trip: tolls, rush-hour fee, ferry, parking.
  let costKind = $state(COST_KINDS[0]);
  let costAmount = $state("");
  let costFile = $state(null);
  async function addCost(event) {
    event.preventDefault();
    const ore = oreOrNull(costAmount);
    if (!ore || ore <= 0) return toast("Skriv beløpet i kroner", false);
    try {
      const id = await save();
      const made = await post(base, { trip_expense_id: id, dato: date || null, beskrivelse: costKind, belop_ore: ore });
      if (costFile) await putReceipt(made.expense_id, costFile, costFile.name);
      costAmount = "";
      costFile = null;
      event.target.reset();
      if (!expense) settle();
      else loadCosts();
    } catch (err) {
      toast(err.message, false);
      settle();
    }
  }

  // What stops «Send inn», per field — the server says the same, but the
  // form shows it where the field is, and moves focus there.
  let problems = $derived({
    date: !date,
    amount: !isTrip && !(amountOre > 0),
    from: isTrip && !from.trim(),
    to: isTrip && !to.trim(),
    km: isTrip && oneWay === null,
    description: !description.trim(),
  });
  const err = (key) => submitted && problems[key];
</script>

<div class="grid gap-6 items-start {isTrip ? '' : 'lg:grid-cols-2'}">
  {#if !isTrip}
    <section class="card border border-base-300 bg-base-100 lg:sticky lg:top-4" aria-label="Kvitteringen">
      <div class="card-body p-2 gap-3">
        {#if editing && preview?.kind === "image"}
          <ReceiptEditor url={preview.url} onsave={(blob) => replaceReceipt(blob, (receiptName || "kvittering").replace(/\.[^.]*$/, "") + ".jpg")} oncancel={closeEditor} />
        {:else if !receiptName}
          <p class="p-4 opacity-70">Ingen kvittering ennå. Legg den ved før du sender inn.</p>
        {:else if preview === null}
          <div class="flex justify-center p-12"><span class="loading loading-spinner"></span></div>
        {:else if preview.kind === "pdf"}
          <iframe src={preview.url} title={"Kvittering: " + receiptName} class="w-full h-[60vh] rounded"></iframe>
        {:else if preview.kind === "image"}
          <img src={preview.url} alt={"Kvittering: " + receiptName} class="w-full max-h-[70vh] object-contain rounded" />
        {/if}
        {#if !editing}
          <div class="flex flex-wrap gap-2">
            {#if preview?.kind === "image"}
              <button bind:this={editButton} type="button" class="btn btn-sm" onclick={() => (editing = true)}>Roter eller beskjær</button>
            {/if}
            <label class="btn btn-sm btn-ghost focus-within:outline-2 focus-within:outline-offset-2">
              {receiptName ? "Bytt kvittering" : "Legg ved kvittering"}
              <input type="file" class="sr-only" accept="image/*,application/pdf" onchange={(ev) => ev.target.files[0] && replaceReceipt(ev.target.files[0], ev.target.files[0].name)} />
            </label>
          </div>
        {/if}
      </div>
    </section>
  {/if}

  <form class="card border border-base-300 bg-base-100" aria-label="Opplysninger" novalidate onsubmit={(ev) => (ev.preventDefault(), act("submit"))}>
    <div class="card-body grid gap-4">
      {#if people.length > 1}
        <label class="fieldset">
          <span class="fieldset-legend">Hvem skal ha pengene?</span>
          <select class="select w-full" bind:value={claimant}>
            <option value="">Meg selv</option>
            {#each people as p (p.person_id)}<option value={p.person_id}>{p.name}</option>{/each}
          </select>
        </label>
      {/if}

      {#if !isTrip && (cards.length || card)}
        <label class="fieldset">
          <span class="fieldset-legend">Betalt med</span>
          <select class="select w-full" bind:value={card} disabled={fromLine}>
            <option value="">Egne penger — jeg skal ha pengene tilbake</option>
            {#each cards as c (c.card_id)}<option value={c.card_id}>{c.label}</option>{/each}
            {#if card && !cards.some((c) => c.card_id === card)}<option value={card}>{e.card_label || "Firmakort"}</option>{/if}
          </select>
          <span class="label whitespace-normal">
            {#if fromLine}Fra kontoutskriften for {e.card_label}: dato og beløp må stemme med kortlinjen.
            {:else if card}Firmaets penger — kvitteringen er dokumentasjonen, ingenting utbetales.{/if}
          </span>
        </label>
      {/if}

      {#if reading}
        <div role="status" class="flex items-center gap-2 text-sm"><span class="loading loading-spinner loading-sm"></span>Leser kvitteringen …</div>
      {:else if readResult}
        <div role="status" class="alert alert-info alert-soft text-sm">
          <div>
            <p>
              {readResult.filled
                ? "Fylte inn " + readResult.filled + (readResult.filled === 1 ? " felt" : " felter") + " fra kvitteringen. Sjekk dem før du sender inn."
                : "Fant ingenting å fylle inn fra kvitteringen."}
            </p>
            {#each readResult.warnings as w (w)}<p class="opacity-80">{w}</p>{/each}
          </div>
        </div>
      {/if}

      <div class="grid gap-3 sm:grid-cols-2">
        <label class="fieldset">
          <span class="fieldset-legend">{isTrip ? "Kjøredato" : "Dato på kvitteringen"}</span>
          <input id="utkast-date" type="date" class="input w-full" max={today()} bind:value={date} aria-invalid={err("date")} />
          {#if err("date")}<span class="label text-error">Velg dato</span>
          {:else if suggestedWhy("date", date)}<span class="label whitespace-normal">Lest fra kvitteringen: {suggestedWhy("date", date)}</span>{/if}
        </label>
        {#if !isTrip}
          <label class="fieldset">
            <span class="fieldset-legend">Beløp (kr)</span>
            <input id="utkast-amount" class="input w-full" inputmode="decimal" autocomplete="off" bind:value={amount} aria-invalid={err("amount")} />
            <span class="label whitespace-normal {err('amount') ? 'text-error' : ''}">{err("amount")
                ? "Skriv beløpet, for eksempel 249,50"
                : suggestedWhy("amount", amount)
                  ? "Lest fra kvitteringen: " + suggestedWhy("amount", amount)
                  : "Hele beløpet, med mva"}</span>
          </label>
        {:else}
          <label class="fieldset">
            <span class="fieldset-legend">Kjøretøy</span>
            <select class="select w-full" bind:value={vehicle}>
              {#each VEHICLES as [key, label] (key)}<option value={key}>{label}</option>{/each}
            </select>
          </label>
        {/if}
      </div>

      {#if isTrip}
        <div class="grid gap-3 sm:grid-cols-2">
          <div class="fieldset">
            <label class="fieldset-legend" for="utkast-from">Fra</label>
            <AddressInput id="utkast-from" bind:value={from} lookup={base + "/addresses"} placeholder="Adresse eller firmanavn" invalid={err("from")} />
            {#if err("from")}<span class="label text-error">Oppgi hvor du kjørte fra</span>{/if}
          </div>
          <div class="fieldset">
            <label class="fieldset-legend" for="utkast-to">Til</label>
            <AddressInput id="utkast-to" bind:value={to} lookup={base + "/addresses"} placeholder="Adresse eller firmanavn" invalid={err("to")} />
            {#if err("to")}<span class="label text-error">Oppgi hvor du kjørte til</span>{/if}
          </div>
        </div>
        <div class="grid gap-3 sm:grid-cols-2 items-end">
          <label class="fieldset">
            <span class="fieldset-legend">Kilometer {roundTrip ? "én vei" : ""}</span>
            <input id="utkast-km" class="input w-full" inputmode="numeric" autocomplete="off" bind:value={km} aria-invalid={err("km")} />
            <span class="label whitespace-normal {err('km') ? 'text-error' : ''}">
              {err("km") ? "Skriv hele kilometer, for eksempel 42" : (roundTrip && oneWay ? "Tur/retur: " + totalKm + " km i alt" : "Hele kilometer")}
            </span>
          </label>
          <label class="label cursor-pointer gap-2 pb-6">
            <input type="checkbox" class="checkbox checkbox-sm" bind:checked={roundTrip} />
            <span class="text-base-content">Tur/retur</span>
          </label>
        </div>
        {#if route?.available}
          <div class="flex flex-wrap items-center gap-2 text-sm" role="status" aria-live="polite">
            <span>
              Kjøreruten er {route.km} km én vei.
              {#if oneWay && oneWay !== route.km}Du har skrevet {oneWay} km — det er greit når turen hadde en omvei, men si hvorfor i formålet.{/if}
            </span>
            <RouteMap load={() => Promise.resolve(route)} />
          </div>
        {:else if route?.error}
          <p class="text-sm opacity-70" role="status">{route.error}</p>
        {/if}

        <fieldset class="fieldset">
          <legend class="fieldset-legend">Passasjerer</legend>
          <span class="label whitespace-normal">Kollegaer på samme oppdrag gir passasjertillegg. Navnet er dokumentasjonen.</span>
          {#each passengers as _, i (i)}
            <div class="flex gap-2">
              <input id={"passasjer-" + i} class="input flex-1" aria-label={"Passasjer " + (i + 1)} placeholder="Navn" bind:value={passengers[i]} />
              <button type="button" class="btn btn-ghost" aria-label={"Fjern passasjer " + (i + 1)} onclick={() => passengers.splice(i, 1)}>✕</button>
            </div>
          {/each}
          <button type="button" class="btn btn-sm w-fit" onclick={async () => {
              passengers.push("");
              await tick();
              document.getElementById("passasjer-" + (passengers.length - 1))?.focus();
            }}>Legg til passasjer</button>
        </fieldset>
      {/if}

      <label class="fieldset">
        <span class="fieldset-legend">{isTrip ? "Formål" : "Hva gjaldt det?"}</span>
        <input class="input w-full" placeholder={isTrip ? "F.eks. kundemøte hos Nordvik AS" : "F.eks. taxi til kundemøte"} bind:value={description} id="utkast-description" aria-invalid={err("description")} />
        {#if err("description")}<span class="label text-error">{isTrip ? "Skriv formålet med turen" : "Skriv hva utlegget gjaldt"}</span>
        {:else if suggestedWhy("description", description)}<span class="label whitespace-normal">Lest fra kvitteringen: {suggestedWhy("description", description)}. Skriv gjerne hva det gjaldt.</span>{/if}
      </label>

      {#if dims.some((d) => d.active)}
        <div class="grid gap-3 sm:grid-cols-2">
          <DimSelect {dims} kind="avdeling" label="Avdeling" alleLabel="Ingen avdeling" cls="select w-full" bind:value={department} />
          <div onchange={projectChanged}>
            <DimSelect {dims} kind="prosjekt" label="Prosjekt" alleLabel="Ingen prosjekt" cls="select w-full" bind:value={project} />
          </div>
        </div>
        {#if project}
          <label class="label cursor-pointer gap-2">
            <input type="checkbox" class="checkbox checkbox-sm" bind:checked={billable} />
            <span class="text-base-content">Viderefaktureres kunden</span>
          </label>
        {/if}
      {/if}

      {#if isTrip}
        <div class="alert alert-soft text-sm" role="status" aria-live="polite">
          {#if rate?.error}
            <span>{rate.error}</span>
          {:else if !rate}
            <span>Henter satsen for kjøredatoen …</span>
          {:else if estimate}
            <span>
              {totalKm} km × {kr(estimate.per)} kr{named ? " (med " + named + " passasjertillegg)" : ""} = <strong>{kr(estimate.total)} kr</strong>.
              {#if estimate.taxable > 0}Av dette er {kr(estimate.taxable)} kr over den trekkfrie satsen og trekkpliktig — det trekkes skatt av når kravet utbetales med lønnen.{/if}
            </span>
          {:else}
            <span>Satsen på kjøredatoen er {kr(rate.sats_ore_per_km)} kr per km.</span>
          {/if}
        </div>
      {/if}

      <div class="flex flex-wrap gap-2">
        <button class="btn btn-primary" type="submit" disabled={busy}>
          {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
          Send inn
        </button>
        <button class="btn" type="button" disabled={busy} onclick={() => act("save")}>Lagre utkast</button>
        {#if expense}
          <button class="btn btn-ghost text-error ml-auto" type="button" onclick={discard}>Forkast</button>
        {/if}
      </div>
    </div>
  </form>

  {#if isTrip}
    <section class="card border border-base-300 bg-base-100" aria-labelledby="kostnader-tittel">
      <div class="card-body grid gap-3">
        <h2 id="kostnader-tittel" class="card-title">Bompenger, ferje og parkering</h2>
        <p class="text-sm opacity-70">
          Dekkes etter faktisk kostnad, med kvittering eller AutoPASS-oversikt. Mangler kvitteringen ennå, legg den
          ved senere — kjøringen kan sendes inn uten å vente.
        </p>
        {#if costs.length}
          <ul class="grid gap-1">
            {#each costs as c (c.expense_id)}
              <li class="flex flex-wrap gap-2 items-center">
                <a class="link mr-auto" href={list + "/" + c.expense_id}>{c.beskrivelse || "Utlegg"}</a>
                {#if c.status === "utkast" && !c.receipt_filename}<span class="badge badge-sm badge-warning badge-soft">Mangler kvittering</span>{/if}
                <span class="badge badge-sm badge-soft {STATUS[c.status]?.badge}">{STATUS[c.status]?.text || c.status}</span>
                <span class="tabular-nums">{c.belop_ore ? kr(c.belop_ore) + " kr" : ""}</span>
              </li>
            {/each}
          </ul>
        {/if}
        <form class="grid gap-2 sm:grid-cols-[1fr_8rem_1fr_auto] items-end" onsubmit={addCost}>
          <label class="fieldset">
            <span class="fieldset-legend">Hva</span>
            <select class="select w-full" bind:value={costKind}>
              {#each COST_KINDS as k (k)}<option>{k}</option>{/each}
            </select>
          </label>
          <label class="fieldset">
            <span class="fieldset-legend">Beløp (kr)</span>
            <input class="input w-full" inputmode="decimal" bind:value={costAmount} />
          </label>
          <label class="fieldset">
            <span class="fieldset-legend">Kvittering (valgfri nå)</span>
            <input type="file" class="file-input w-full" accept="image/*,application/pdf" onchange={(ev) => (costFile = ev.target.files[0] || null)} />
          </label>
          <button class="btn" type="submit">Legg til</button>
        </form>
      </div>
    </section>
  {/if}
</div>
