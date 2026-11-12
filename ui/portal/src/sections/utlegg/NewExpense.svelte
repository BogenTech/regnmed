<script>
  // Nytt krav: utlegg med kvittering, eller kjøring. Kvitteringen først —
  // slik man faktisk gjør det (bilde, så detaljer) — og beløpet for en
  // kjøring vises før innsending, med den trekkpliktige delen sagt rett
  // ut. Knappen er opptatt til svaret kommer, så ett trykk er ett krav;
  // serveren nekter også samme kvittering to ganger.
  import { api, post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { mileage, parseKm } from "./expenseMath.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/expenses");
  const list = $derived("#/c/" + companyId + "/utlegg");

  let kind = $state("utlegg");
  let file = $state(null);
  let date = $state(today());
  let amount = $state("");
  let km = $state("");
  let description = $state("");
  let submitted = $state(false);
  let busy = $state(false);
  let rate = $state(null); // { sats_ore_per_km, trekkfri_ore_per_km } | { error }

  let amountOre = $derived.by(() => {
    try {
      const v = parseKr(amount);
      return amount.trim() && v > 0 ? v : null;
    } catch {
      return null;
    }
  });
  let kmValue = $derived(parseKm(km));

  // The km-sats is dated: a trip in March is paid March's rate.
  $effect(() => {
    if (kind !== "kjoring" || !date) return;
    rate = null;
    api(base + "/kjoring?dato=" + date)
      .then((r) => (rate = r))
      .catch((e) => (rate = { error: e.message }));
  });
  let estimate = $derived(
    kmValue && rate?.sats_ore_per_km ? mileage(kmValue, rate.sats_ore_per_km, rate.trekkfri_ore_per_km) : null,
  );

  let problems = $derived({
    file: kind === "utlegg" && !file ? "Legg ved kvitteringen" : null,
    date: !date ? "Velg dato" : null,
    amount: kind === "utlegg" && amountOre === null ? "Skriv beløpet i kroner, for eksempel 249,50" : null,
    km: kind === "kjoring" && kmValue === null ? "Skriv hele kilometer, for eksempel 120" : null,
    description: !description.trim()
      ? kind === "utlegg"
        ? "Skriv hva utlegget gjaldt"
        : "Skriv strekning og formål"
      : null,
  });
  let firstProblem = $derived(Object.entries(problems).find(([, v]) => v)?.[0]);

  function pick(event) {
    file = event.target.files[0] || null;
  }

  async function submit(event) {
    event.preventDefault();
    submitted = true;
    if (firstProblem) {
      document.getElementById("ny-" + firstProblem)?.focus();
      return;
    }
    if (busy) return;
    busy = true;
    try {
      let id;
      if (kind === "utlegg") {
        const made = await api(
          base +
            "/utlegg?filename=" + encodeURIComponent(file.name) +
            "&dato=" + date +
            "&belop_ore=" + amountOre +
            "&beskrivelse=" + encodeURIComponent(description.trim()),
          {
            method: "POST",
            headers: { "content-type": file.type || "application/octet-stream" },
            body: file,
          },
        );
        id = made.expense_id;
        toast("Utlegget er sendt inn", true);
      } else {
        const made = await post(base + "/kjoring", { dato: date, km: kmValue, beskrivelse: description.trim() });
        id = made.expense_id;
        toast("Kjøringen er sendt inn: " + kr(made.belop_ore) + " kr", true);
      }
      location.hash = list + "/" + id;
    } catch (e) {
      toast(e.message, false);
      busy = false;
    }
  }

  const err = (key) => submitted && problems[key];
</script>

<nav class="mb-3">
  <a class="link link-hover text-sm" href={list}>← Alle utlegg</a>
</nav>
<h1 class="text-2xl font-bold mb-4">Nytt krav</h1>

<form class="card border border-base-300 bg-base-100 max-w-xl" novalidate onsubmit={submit}>
  <div class="card-body grid gap-4">
    <fieldset class="fieldset">
      <legend class="fieldset-legend">Hva vil du ha dekket?</legend>
      <div class="flex flex-wrap gap-2">
        {#each [["utlegg", "Et utlegg med kvittering"], ["kjoring", "Kjøring med egen bil"]] as [key, label] (key)}
          <label class="label cursor-pointer gap-2 border border-base-300 rounded-box px-3 py-2 {kind === key ? 'border-primary' : ''}">
            <input type="radio" class="radio radio-sm" name="kind" value={key} bind:group={kind} />
            <span class="text-sm text-base-content">{label}</span>
          </label>
        {/each}
      </div>
    </fieldset>

    {#if kind === "utlegg"}
      <div class="fieldset">
        <label class="fieldset-legend" for="ny-file">Kvittering</label>
        <!-- No `capture`: a phone then offers both the camera and saved photos/files. -->
        <input
          id="ny-file"
          type="file"
          class="file-input w-full {err('file') ? 'file-input-error' : ''}"
          accept="image/*,application/pdf"
          onchange={pick}
          aria-invalid={!!err("file")}
          aria-describedby="ny-file-hjelp"
        />
        <span id="ny-file-hjelp" class="label whitespace-normal {err('file') ? 'text-error' : ''}">
          {err("file") || "Bilde eller PDF. Kvitteringen kan ikke endres etter innsending og følger kravet inn i regnskapet."}
        </span>
      </div>
    {/if}

    <div class="grid gap-3 sm:grid-cols-2">
      <div class="fieldset">
        <label class="fieldset-legend" for="ny-date">{kind === "utlegg" ? "Dato på kvitteringen" : "Kjøredato"}</label>
        <input id="ny-date" type="date" class="input w-full" max={today()} bind:value={date} aria-invalid={!!err("date")} />
        {#if err("date")}<span class="label text-error">{err("date")}</span>{/if}
      </div>
      {#if kind === "utlegg"}
        <div class="fieldset">
          <label class="fieldset-legend" for="ny-amount">Beløp (kr)</label>
          <input
            id="ny-amount"
            class="input w-full {err('amount') ? 'input-error' : ''}"
            inputmode="decimal"
            autocomplete="off"
            bind:value={amount}
            aria-invalid={!!err("amount")}
            aria-describedby="ny-amount-hjelp"
          />
          <span id="ny-amount-hjelp" class="label whitespace-normal {err('amount') ? 'text-error' : ''}">
            {err("amount") || "Hele beløpet, med mva"}
          </span>
        </div>
      {:else}
        <div class="fieldset">
          <label class="fieldset-legend" for="ny-km">Kilometer</label>
          <input
            id="ny-km"
            class="input w-full {err('km') ? 'input-error' : ''}"
            inputmode="numeric"
            autocomplete="off"
            bind:value={km}
            aria-invalid={!!err("km")}
            aria-describedby="ny-km-hjelp"
          />
          <span id="ny-km-hjelp" class="label whitespace-normal {err('km') ? 'text-error' : ''}">
            {err("km") || "Tur-retur teller begge veier"}
          </span>
        </div>
      {/if}
    </div>

    <div class="fieldset">
      <label class="fieldset-legend" for="ny-description">{kind === "utlegg" ? "Hva gjaldt det?" : "Strekning og formål"}</label>
      <input
        id="ny-description"
        class="input w-full {err('description') ? 'input-error' : ''}"
        placeholder={kind === "utlegg" ? "F.eks. Taxi til kundemøte hos Nordvik AS" : "F.eks. Oslo–Drammen t/r, kundemøte"}
        bind:value={description}
        aria-invalid={!!err("description")}
      />
      {#if err("description")}<span class="label text-error">{err("description")}</span>{/if}
    </div>

    {#if kind === "kjoring"}
      <div class="alert alert-soft text-sm" role="status" aria-live="polite">
        {#if rate?.error}
          <span>{rate.error}</span>
        {:else if !rate}
          <span>Henter satsen for kjøredatoen …</span>
        {:else if estimate}
          <span>
            {kmValue} km × {kr(rate.sats_ore_per_km)} kr = <strong>{kr(estimate.amount)} kr</strong>.
            {#if estimate.taxable > 0}
              Av dette er {kr(estimate.taxable)} kr over statens trekkfrie sats ({kr(rate.trekkfri_ore_per_km)} kr/km) og
              skal lønnsinnberettes.
            {/if}
          </span>
        {:else}
          <span>Statens sats på kjøredatoen er {kr(rate.sats_ore_per_km)} kr per km.</span>
        {/if}
      </div>
    {/if}

    <div class="flex flex-wrap gap-2">
      <button class="btn btn-primary" type="submit" disabled={busy}>
        {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
        Send inn
      </button>
      <a class="btn btn-ghost" href={list}>Avbryt</a>
    </div>
  </div>
</form>
