<script>
  // A business trip as one reiseregning: when it started and ended (to the
  // minute — the døgn are counted from departure), where, and per døgn
  // where the traveller was, where they slept and which meals someone else
  // paid for. The amounts are the SERVER's (…/reise/preview runs the same
  // code as submission); the form only counts which døgn have a night.
  // The trip's costs (hotel, tickets, taxi) and drives hang under it and
  // go in with it. `expense` is null for a trip not saved yet.
  import { api, post, send } from "../../lib/api.js";
  import { kr, oreOrNull } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import DimSelect from "../../components/DimSelect.svelte";
  import { STATUS, LODGING, DAY_KIND, nights, withSeconds } from "./expenseMath.js";

  let { companyId, expense = null, onchanged } = $props();

  const base = $derived("/companies/" + companyId + "/expenses");
  const list = $derived("#/c/" + companyId + "/utlegg");
  const e = expense || {};

  let description = $state(e.beskrivelse || "");
  let destination = $state(e.destination || "");
  let departure = $state((e.departure_at || "").slice(0, 16));
  let ret = $state((e.return_at || "").slice(0, 16));
  let overnight = $state(e.overnight ?? true);
  let over15 = $state(!!e.over_15_km);
  let days = $state((e.travel_days || []).map((d) => ({ place: "NO", breakfast: false, lunch: false, dinner: false, ...d })));
  let department = $state(e.department || "");
  let project = $state(e.project || "");
  let billable = $state(!!e.billable);
  let claimant = $state("");

  let dims = $state([]);
  let people = $state([]);
  let rates = $state(null);
  let countError = $state(null);
  let preview = $state(null); // { belop_ore, trekkfri_ore, trekkpliktig_ore, lines } | { error }
  let children = $state([]);
  let busy = $state(false);
  let submitted = $state(false);

  const canFileForOthers = harRett(companyId, "UTLEGG_GODKJENN");

  $effect(() => {
    api("/companies/" + companyId + "/dimensions").then((r) => (dims = r.dimensions)).catch(() => {});
    if (!expense && canFileForOthers) {
      api(base + "/claimants").then((r) => (people = r.people)).catch(() => {});
    }
  });

  function loadChildren() {
    if (expense) api(base + "?trip=" + expense.expense_id).then((r) => (children = r.expenses)).catch(() => {});
  }
  $effect(loadChildren);

  // The rates and the state's table abroad, for the departure date.
  let ratesFor = "";
  $effect(() => {
    const date = departure.slice(0, 10);
    if (!date || date === ratesFor) return;
    ratesFor = date;
    api(base + "/reise?dato=" + date)
      .then((r) => ratesFor === date && (rates = r))
      .catch((err) => ratesFor === date && (rates = { error: err.message }));
  });
  let regions = $derived.by(() => {
    const out = new Map();
    for (const r of rates?.abroad || []) {
      if (!out.has(r.region)) out.set(r.region, []);
      out.get(r.region).push(r);
    }
    // Nearest first: most trips abroad go to Europe.
    const order = ["Europa", "Amerika", "Asia", "Afrika", "Australia/Oceania"];
    return [...out].sort(([a], [b]) => order.indexOf(a) - order.indexOf(b));
  });
  const placeLabel = (r) => r.country + (r.area ? ", " + r.area : "");

  // How many døgn to ask about: the server's count, so the form never cuts
  // the trip differently from the submission. Existing answers are kept.
  $effect(() => {
    const d = withSeconds(departure);
    const r = withSeconds(ret);
    const o = overnight;
    if (!d || !r) return;
    const timer = setTimeout(() => {
      api(base + "/reise/days?departure_at=" + d + "&return_at=" + r + "&overnight=" + o)
        .then((x) => {
          countError = null;
          const last = days[days.length - 1];
          while (days.length < x.days) days.push({ place: last?.place || "NO", lodging: null, breakfast: false, lunch: false, dinner: false });
          days.length = x.days;
        })
        .catch((err) => (countError = err.message));
    }, 300);
    return () => clearTimeout(timer);
  });
  let hasNight = $derived(nights(departure, ret, overnight, days.length));
  let missingLodging = $derived(hasNight.findIndex((n, i) => n === true && !days[i]?.lodging));
  // A day without a night has no lodging to remember.
  $effect(() => {
    hasNight.forEach((night, i) => !night && days[i]?.lodging && (days[i].lodging = null));
  });

  const daysBody = () =>
    days.map((d, i) => ({
      place: d.place || "NO",
      lodging: hasNight[i] ? d.lodging || null : null,
      breakfast: !!d.breakfast,
      lunch: !!d.lunch,
      dinner: !!d.dinner,
    }));

  // What it comes to, from the server, as the traveller fills it in.
  $effect(() => {
    const body = {
      departure_at: withSeconds(departure),
      return_at: withSeconds(ret),
      overnight,
      travel_days: daysBody(),
    };
    if (!body.departure_at || !body.return_at || countError || !days.length) {
      preview = null;
      return;
    }
    const timer = setTimeout(() => {
      post(base + "/reise/preview", body)
        .then((r) => (preview = r))
        .catch((err) => (preview = { error: err.message }));
    }, 400);
    return () => clearTimeout(timer);
  });

  function body() {
    return {
      beskrivelse: description,
      destination,
      departure_at: withSeconds(departure),
      return_at: withSeconds(ret),
      overnight,
      over_15_km: over15,
      travel_days: daysBody(),
      department: department || null,
      project: project || null,
      billable,
    };
  }

  // A new trip gets its id ONCE, however the steps after it go.
  let createdId = null;
  async function save() {
    const id = expense?.expense_id || createdId;
    if (id) {
      await send("PUT", base + "/" + id, body());
      return id;
    }
    const made = await post(base, { ...body(), kind: "reise", ...(claimant ? { person_id: claimant } : {}) });
    createdId = made.expense_id;
    return createdId;
  }
  const settle = () => !expense && createdId && (location.hash = list + "/" + createdId);

  let problems = $derived({
    description: !description.trim(),
    destination: !destination.trim(),
    departure: !departure,
    ret: !ret,
    over15: !over15,
    lodging: missingLodging >= 0,
  });
  const err = (key) => submitted && problems[key];

  async function act(what) {
    if (busy) return;
    if (what === "submit") {
      submitted = true;
      const first = Object.keys(problems).find((k) => problems[k]);
      if (first) return document.getElementById(first === "lodging" ? "reise-lodging-" + missingLodging : "reise-" + first)?.focus();
    }
    busy = true;
    try {
      const id = await save();
      if (what === "submit") {
        const r = await post(base + "/" + id + "/submit", {});
        toast("Reisen er sendt inn: " + kr(r.belop_ore) + " kr i diett", true);
        if (r.costs_submitted) toast(r.costs_submitted + " kostnad(er) og kjøring(er) ble sendt inn samtidig", true);
        for (const w of r.costs_waiting || []) toast("Venter fortsatt: " + w, false);
      } else {
        toast("Utkastet er lagret", true);
      }
      if (!expense) settle();
      else onchanged?.();
    } catch (error) {
      toast(error.message, false);
      settle();
    }
    busy = false;
  }

  async function discard() {
    const ok = await bekreft(
      children.some((c) => c.status === "utkast")
        ? "Forkaste reisen? Utkastene til utlegg og kjøringer på reisen forkastes også."
        : "Forkaste utkastet?",
      { ok: "Forkast", farlig: true },
    );
    if (!ok) return;
    try {
      await post(base + "/" + expense.expense_id + "/discard", {});
      toast("Utkastet er forkastet", true);
      location.hash = list + "/utkast";
    } catch (error) {
      toast(error.message, false);
    }
  }

  // A cost of the trip: hotel, tickets, taxi — each with its receipt.
  let costText = $state("");
  let costAmount = $state("");
  let costFile = $state(null);
  async function addCost(event) {
    event.preventDefault();
    const ore = oreOrNull(costAmount);
    if (!costText.trim()) return toast("Skriv hva kostnaden gjaldt", false);
    if (!ore || ore <= 0) return toast("Skriv beløpet i kroner", false);
    try {
      const id = await save();
      const made = await post(base, {
        trip_expense_id: id,
        dato: departure.slice(0, 10) || null,
        beskrivelse: costText.trim(),
        belop_ore: ore,
      });
      if (costFile) {
        await api(base + "/" + made.expense_id + "/receipt?filename=" + encodeURIComponent(costFile.name), {
          method: "PUT",
          headers: { "content-type": costFile.type || "application/octet-stream" },
          body: costFile,
        });
      }
      costText = "";
      costAmount = "";
      costFile = null;
      event.target.reset();
      if (!expense) settle();
      else loadChildren();
    } catch (error) {
      toast(error.message, false);
      settle();
    }
  }

  // A drive on the trip opens as its own draft.
  async function addDrive() {
    try {
      const id = await save();
      const made = await post(base, { kind: "kjoring", trip_expense_id: id, dato: departure.slice(0, 10) || null });
      location.hash = list + "/" + made.expense_id;
    } catch (error) {
      toast(error.message, false);
      settle();
    }
  }

  const fmt = (iso) =>
    new Date(iso + "Z").toLocaleString("nb-NO", { timeZone: "UTC", weekday: "short", day: "numeric", month: "numeric", hour: "2-digit", minute: "2-digit" });
</script>

<div class="grid gap-6 items-start">
  <form class="card border border-base-300 bg-base-100" aria-label="Reisen" novalidate onsubmit={(ev) => (ev.preventDefault(), act("submit"))}>
    <div class="card-body grid gap-4">
      {#if people.length > 1}
        <label class="fieldset">
          <span class="fieldset-legend">Hvem reiste?</span>
          <select class="select w-full" bind:value={claimant}>
            <option value="">Meg selv</option>
            {#each people as p (p.person_id)}<option value={p.person_id}>{p.name}</option>{/each}
          </select>
        </label>
      {/if}

      <div class="grid gap-3 sm:grid-cols-2">
        <label class="fieldset">
          <span class="fieldset-legend">Formål</span>
          <input id="reise-description" class="input w-full" placeholder="F.eks. messe, kundemøte hos Nordvik AS" bind:value={description} aria-invalid={err("description")} />
          {#if err("description")}<span class="label text-error">Skriv formålet med reisen</span>{/if}
        </label>
        <label class="fieldset">
          <span class="fieldset-legend">Reisemål</span>
          <input id="reise-destination" class="input w-full" placeholder="By, og land om det er i utlandet" bind:value={destination} aria-invalid={err("destination")} />
          {#if err("destination")}<span class="label text-error">Oppgi hvor du reiste</span>{/if}
        </label>
      </div>

      <div class="grid gap-3 sm:grid-cols-2">
        <label class="fieldset">
          <span class="fieldset-legend">Avreise</span>
          <input id="reise-departure" type="datetime-local" class="input w-full" bind:value={departure} aria-invalid={err("departure")} />
          <span class="label whitespace-normal {err('departure') ? 'text-error' : ''}">Når du dro hjemmefra eller fra jobben</span>
        </label>
        <label class="fieldset">
          <span class="fieldset-legend">Hjemkomst</span>
          <input id="reise-ret" type="datetime-local" class="input w-full" bind:value={ret} aria-invalid={err("ret")} />
          <span class="label whitespace-normal {err('ret') ? 'text-error' : ''}">Når du var tilbake</span>
        </label>
      </div>

      <div class="grid gap-2">
        <label class="label cursor-pointer gap-2 w-fit">
          <input type="checkbox" class="checkbox checkbox-sm" bind:checked={overnight} />
          <span class="text-base-content">Jeg overnattet borte</span>
        </label>
        <label class="label cursor-pointer gap-2 w-fit">
          <input id="reise-over15" type="checkbox" class="checkbox checkbox-sm" bind:checked={over15} aria-invalid={err("over15")} />
          <span class="text-base-content whitespace-normal">Reisen gikk minst 15 km fra der jeg startet</span>
        </label>
        {#if err("over15")}<span class="label text-error whitespace-normal">Under 15 km gir ingen diett</span>{/if}
      </div>

      {#if countError}
        <div class="alert alert-warning alert-soft text-sm" role="status"><span>{countError}</span></div>
      {:else if days.length}
        <fieldset class="grid gap-3">
          <legend class="font-semibold mb-1">Døgn for døgn</legend>
          <p class="text-sm opacity-70">
            Et døgn er 24 timer regnet fra avreise. Velg landet der du var lengst, hvor du sov, og hvilke måltider
            andre betalte (hotellfrokost, middag på konferansen).
          </p>
          {#each days as day, i (i)}
            {@const line = preview?.lines?.[i]}
            <fieldset class="border border-base-300 rounded-box p-3 grid gap-2">
              <legend class="px-1 text-sm font-medium">
                Døgn {i + 1}{line ? ": " + fmt(line.start) + " – " + fmt(line.end) + " · " + DAY_KIND[line.kind] : ""}
              </legend>
              <div class="grid gap-2 sm:grid-cols-2">
                <label class="fieldset">
                  <span class="fieldset-legend">Hvor</span>
                  <select class="select w-full" bind:value={day.place}>
                    <option value="NO">Norge</option>
                    {#each regions as [region, rows] (region)}
                      <optgroup label={region}>
                        {#each rows as r (r.key)}<option value={r.key}>{placeLabel(r)}</option>{/each}
                      </optgroup>
                    {/each}
                  </select>
                </label>
                {#if hasNight[i]}
                  <label class="fieldset">
                    <span class="fieldset-legend">Overnatting</span>
                    <select id={"reise-lodging-" + i} class="select w-full" bind:value={day.lodging} aria-invalid={submitted && hasNight[i] === true && !day.lodging}>
                      <option value={null}>{hasNight[i] === true ? "Velg …" : "Ingen natt i dette døgnet"}</option>
                      {#each LODGING as [key, label] (key)}<option value={key}>{label}</option>{/each}
                    </select>
                    {#if submitted && hasNight[i] === true && !day.lodging}<span class="label text-error">Velg hvor du sov</span>{/if}
                  </label>
                {/if}
              </div>
              <div class="flex flex-wrap gap-4" role="group" aria-label={"Måltider andre betalte, døgn " + (i + 1)}>
                <span class="text-sm opacity-70">Betalt av andre:</span>
                {#each [["breakfast", "Frokost"], ["lunch", "Lunsj"], ["dinner", "Middag"]] as [key, label] (key)}
                  <label class="label cursor-pointer gap-1">
                    <input type="checkbox" class="checkbox checkbox-xs" bind:checked={day[key]} />
                    <span class="text-base-content text-sm">{label}</span>
                  </label>
                {/each}
                {#if line}<span class="ml-auto tabular-nums text-sm">{kr(line.kost_paid_ore + line.night_paid_ore)} kr</span>{/if}
              </div>
            </fieldset>
          {/each}
        </fieldset>
      {/if}

      {#if dims.some((d) => d.active)}
        <div class="grid gap-3 sm:grid-cols-2">
          <DimSelect {dims} kind="avdeling" label="Avdeling" alleLabel="Ingen avdeling" cls="select w-full" bind:value={department} />
          <DimSelect {dims} kind="prosjekt" label="Prosjekt" alleLabel="Ingen prosjekt" cls="select w-full" bind:value={project} />
        </div>
        {#if project}
          <label class="label cursor-pointer gap-2">
            <input type="checkbox" class="checkbox checkbox-sm" bind:checked={billable} />
            <span class="text-base-content">Viderefaktureres kunden</span>
          </label>
        {/if}
      {/if}

      <div class="alert alert-soft text-sm" role="status" aria-live="polite">
        {#if preview?.error}
          <span>{preview.error}</span>
        {:else if preview}
          <span>
            Diett: <strong>{kr(preview.belop_ore)} kr</strong>
            {preview.scheme === "statens" ? "etter statens satser" : "etter skattefrie satser"}.
            {#if preview.trekkpliktig_ore > 0}
              {kr(preview.trekkpliktig_ore)} kr av dette er over den skattefrie satsen og trekkpliktig — det trekkes skatt
              av når kravet utbetales med lønnen.
            {/if}
          </span>
        {:else}
          <span>Fyll inn avreise og hjemkomst, så regnes dietten ut.</span>
        {/if}
      </div>

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

  <section class="card border border-base-300 bg-base-100" aria-labelledby="reisekost-tittel">
    <div class="card-body grid gap-3">
      <h2 id="reisekost-tittel" class="card-title">Hotell, billetter og kjøring</h2>
      <p class="text-sm opacity-70">
        Hotell og billetter dekkes etter kvittering, kjøring med kilometergodtgjørelse. Legg dem til her, så sendes de
        inn sammen med reisen — kvitteringer som mangler kan legges ved senere.
      </p>
      {#if children.length}
        <ul class="grid gap-1">
          {#each children as c (c.expense_id)}
            <li class="flex flex-wrap gap-2 items-center">
              <a class="link mr-auto" href={list + "/" + c.expense_id}>{c.kind === "kjoring" ? "Kjøring: " : ""}{c.beskrivelse || (c.kind === "kjoring" ? "uten formål ennå" : "Utlegg")}</a>
              {#if c.status === "utkast" && c.kind === "utlegg" && !c.receipt_filename}<span class="badge badge-sm badge-warning badge-soft">Mangler kvittering</span>{/if}
              <span class="badge badge-sm badge-soft {STATUS[c.status]?.badge}">{STATUS[c.status]?.text || c.status}</span>
              <span class="tabular-nums">{c.belop_ore ? kr(c.belop_ore) + " kr" : ""}</span>
            </li>
          {/each}
        </ul>
      {/if}
      <form class="grid gap-2 sm:grid-cols-[1fr_8rem_1fr_auto] items-end" onsubmit={addCost}>
        <label class="fieldset">
          <span class="fieldset-legend">Hva</span>
          <input class="input w-full" placeholder="Hotell, fly, tog, taxi …" bind:value={costText} />
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
      <button class="btn btn-sm w-fit" type="button" onclick={addDrive}>Legg til kjøring på reisen</button>
    </div>
  </section>
</div>
