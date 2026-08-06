<script>
  // Hovedbok (docs/hovedbok.md): the book itself — every bilag with its
  // lines — plus manual bilagsføring, with the kontoplan as a deep-
  // linkable sub-tab (…/hovedbok/kontoplan): it is the book's index,
  // not a section of its own, but it deserves an address. A four-digit
  // `extra` is an account drill-down (kontospesifikasjon filtered
  // server-side). Standard accounts live in the catalog and join the
  // company's kontoplan the first time they are used; custom accounts
  // are added explicitly with a name.
  import { api, post, send } from "../../lib/api.js";
  import { kr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { sporsmal, bekreft } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import DimSelect from "../../components/DimSelect.svelte";
  import Paginering from "../../components/Paginering.svelte";
  import Ikon from "../../components/Ikon.svelte";

  let { companyId, extra } = $props();

  let erKonto = $derived(!!extra && /^\d{4}$/.test(extra));
  let fane = $derived(extra === "kontoplan" ? "kontoplan" : "posteringer");

  let data = $state(null); // { kontoer, standard }
  let dims = $state([]);
  let sok = $state("");

  async function load(id) {
    data = await api("/companies/" + id + "/accounts");
    const d = await api("/companies/" + id + "/dimensions").catch(() => null);
    dims = d?.dimensions || [];
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  // ---- Nytt bilag ----
  function tomLinje() {
    return { account: "", amount: "", vat: "", avdeling: "", prosjekt: "" };
  }
  let date = $state(today());
  let description = $state("");
  let lines = $state([tomLinje(), tomLinje()]);

  let sum = $derived(
    lines.reduce((acc, l) => {
      const n = Number(l.amount.trim().replace(/\s/g, "").replace("−", "-").replace(",", "."));
      return acc + (Number.isFinite(n) ? Math.round(n * 100) : 0);
    }, 0),
  );

  async function bokfor() {
    const brukte = lines.filter((l) => l.account.trim() && l.amount.trim());
    if (brukte.length < 2) {
      toast("Et bilag trenger minst to linjer", false);
      return;
    }
    try {
      // A standard account used for the first time joins the kontoplan
      // now — that is the "available to the company afterwards" rule.
      const egne = new Set(data.kontoer.map((k) => k.number));
      for (const l of brukte) {
        const nr = l.account.trim();
        if (egne.has(nr)) continue;
        if (data.standard.some((s) => s.number === nr)) {
          await post("/companies/" + companyId + "/accounts", { number: nr });
          egne.add(nr);
        } else {
          toast("Konto " + nr + " finnes ikke — legg den til i kontoplanen med navn først", false);
          return;
        }
      }
      const posted = await post("/companies/" + companyId + "/vouchers", {
        journal_code: "GL",
        date,
        description: description || "Manuelt bilag",
        lines: brukte.map((l) => ({
          account: l.account.trim(),
          amount_ore: Math.round(
            Number(l.amount.trim().replace(/\s/g, "").replace("−", "-").replace(",", ".")) * 100,
          ),
          vat_code: l.vat.trim() || null,
          avdeling: l.avdeling || null,
          prosjekt: l.prosjekt || null,
        })),
      });
      toast("Bokført som bilag " + posted.voucher, true);
      date = today();
      description = "";
      lines = [tomLinje(), tomLinje()];
      reload();
      lastPosteringer();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // ---- Kontoplan (the index): search, active-filter, paging ----
  const KONTOER_PER_SIDE = 25;
  let visDeaktiverte = $state(false);
  let kSide = $state(1);

  let treffEgne = $derived.by(() => {
    if (!data) return [];
    const q = sok.trim().toLowerCase();
    return data.kontoer.filter(
      (k) =>
        (visDeaktiverte || k.active) &&
        (!q || k.number.startsWith(q) || k.name.toLowerCase().includes(q)),
    );
  });
  let kontoSide = $derived(
    treffEgne.slice((kSide - 1) * KONTOER_PER_SIDE, kSide * KONTOER_PER_SIDE),
  );
  $effect(() => {
    void sok;
    void visDeaktiverte;
    kSide = 1;
  });

  let treffStandard = $derived.by(() => {
    if (!data) return [];
    const q = sok.trim().toLowerCase();
    if (!q) return [];
    const egne = new Set(data.kontoer.map((k) => k.number));
    return data.standard
      .filter(
        (s) => !egne.has(s.number) && (s.number.startsWith(q) || s.name.toLowerCase().includes(q)),
      )
      .slice(0, 15);
  });

  let egenNummer = $state("");
  let egenNavn = $state("");

  async function leggTil(number, name) {
    try {
      const r = await post("/companies/" + companyId + "/accounts", {
        number,
        name: name || null,
      });
      toast("Konto " + r.number + " " + r.name + " lagt til", true);
      egenNummer = "";
      egenNavn = "";
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endreNavn(k) {
    const navn = await sporsmal("Nytt navn for konto " + k.number + ":", {
      standard: k.name,
      ok: "Lagre",
    });
    if (!navn) return;
    try {
      await send("PUT", "/companies/" + companyId + "/accounts/" + k.number, { name: navn });
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function toggleAktiv(k) {
    if (
      k.active &&
      !(await bekreft(
        "Deaktivere konto " + k.number + "? Nye posteringer avvises; historikken består.",
        { tittel: "Deaktiver konto", ok: "Deaktiver", farlig: true },
      ))
    ) {
      return;
    }
    try {
      await send("PUT", "/companies/" + companyId + "/accounts/" + k.number, {
        active: !k.active,
      });
      reload();
    } catch (error) {
      toast(error.message, false);
    }
  }

  // ---- Posteringene — the actual hovedbok. Vouchers newest-first
  // with their lines, paged and filtered SERVER-SIDE (GET /vouchers
  // with from/to/sok/limit/offset/lines) — the fetch is one page, so
  // the view scales with the ledger. The filter reads the whole bilag:
  // number, date, text, and every line's account number/name.
  const BILAG_PER_SIDE = 20;
  let year = $state(new Date().getFullYear());
  let posteringer = $state(null); // { total, vouchers }
  let pSok = $state("");
  let pSokAktiv = $state("");
  let pSide = $state(1);
  let fetchId = 0;
  let debounceTimer;

  // Debounce the filter: one request per pause in typing, not one per
  // keystroke. The stale-response guard (fetchId) makes overlapping
  // answers harmless — only the newest lands.
  $effect(() => {
    const v = pSok;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => (pSokAktiv = v), 300);
  });
  $effect(() => {
    void pSokAktiv;
    void year;
    pSide = 1;
  });

  async function lastPosteringer() {
    const id = ++fetchId;
    posteringer = null;
    try {
      const svar = await api(
        "/companies/" + companyId + "/vouchers?lines=true&from=" + year + "-01-01&to=" + year +
          "-12-31&limit=" + BILAG_PER_SIDE + "&offset=" + (pSide - 1) * BILAG_PER_SIDE +
          (pSokAktiv.trim() ? "&sok=" + encodeURIComponent(pSokAktiv.trim()) : ""),
      );
      if (id === fetchId) posteringer = svar;
    } catch (error) {
      toast(error.message, false);
    }
  }

  $effect(() => {
    if (extra) return;
    void year;
    void pSide;
    void pSokAktiv;
    lastPosteringer();
  });

  // ---- Drill-down (extra = account number) ----
  let drill = $state(null);
  $effect(() => {
    if (!erKonto) {
      drill = null;
      return;
    }
    drill = null;
    api(
      "/companies/" + companyId + "/reports/kontospesifikasjon?from=" + year +
        "-01-01&to=" + year + "-12-31&account=" + extra,
    )
      .then((svar) => (drill = svar))
      .catch((error) => toast(error.message, false));
  });
  let drillKonto = $derived(data?.kontoer.find((k) => k.number === extra));
</script>

{#if erKonto}
  <Card title={"Konto " + extra + (drillKonto ? " — " + drillKonto.name : "")}>
    <div class="flex items-center gap-3 mb-2">
      <a class="btn btn-xs btn-ghost" href={"#/c/" + companyId + "/hovedbok/kontoplan"}>
        ← Kontoplan
      </a>
      <div class="join">
        <button class="btn btn-xs join-item" onclick={() => (year -= 1)}>«</button>
        <span class="btn btn-xs join-item pointer-events-none">{year}</span>
        <button class="btn btn-xs join-item" onclick={() => (year += 1)}>»</button>
      </div>
      {#if drillKonto}
        <span class="text-sm opacity-70">Saldo totalt: {kr(drillKonto.saldo_ore)}</span>
      {/if}
    </div>
    {#if !drill}
      <span class="loading loading-spinner loading-sm"></span>
    {:else if drill.posts.length === 0}
      <p class="opacity-70 text-sm">Ingen posteringer på kontoen i {year}.</p>
    {:else}
      <table class="table table-sm">
        <thead>
          <tr>
            <th>Bilag</th><th>Dato</th><th>Tekst</th>
            <th class="text-right">Beløp</th><th class="text-right">Saldo</th>
          </tr>
        </thead>
        <tbody>
          {#each drill.posts as p}
            <tr>
              <td>{p.bilag}</td>
              <td>{p.date}</td>
              <td>
                {p.description}
                {#if p.party_no}<span class="opacity-60">({p.party_no})</span>{/if}
                {#if p.avdeling}<span class="badge badge-ghost badge-xs">{p.avdeling}</span>{/if}
                {#if p.prosjekt}<span class="badge badge-ghost badge-xs">{p.prosjekt}</span>{/if}
              </td>
              <td class="text-right">{kr(p.amount_ore)}</td>
              <td class="text-right">{kr(p.saldo_ore)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </Card>
{:else if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <div role="tablist" class="tabs tabs-box tabs-sm mb-4">
    <a
      role="tab"
      href={"#/c/" + companyId + "/hovedbok"}
      class="tab gap-1.5 {fane === 'posteringer' ? 'tab-active' : ''}"
      aria-selected={fane === "posteringer"}
    >
      <Ikon navn="hovedbok" />
      Posteringer
    </a>
    <a
      role="tab"
      href={"#/c/" + companyId + "/hovedbok/kontoplan"}
      class="tab gap-1.5 {fane === 'kontoplan' ? 'tab-active' : ''}"
      aria-selected={fane === "kontoplan"}
    >
      <Ikon navn="rapporter" />
      Kontoplan
    </a>
  </div>

  {#if fane === "kontoplan"}
    <Card title="Kontoplan">
      <div class="flex gap-3 items-center flex-wrap mb-2">
        <input
          class="input input-sm w-full max-w-sm"
          placeholder="Søk på nummer eller navn (også i standardkontoplanen)"
          bind:value={sok}
        />
        <label class="label cursor-pointer gap-2 text-sm">
          <input type="checkbox" class="checkbox checkbox-sm" bind:checked={visDeaktiverte} />
          Vis deaktiverte
        </label>
      </div>
      {#if treffEgne.length}
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Nr</th><th>Navn</th>
              <th class="text-right">Saldo</th><th class="text-right">Posteringer</th>
              <th></th><th></th>
            </tr>
          </thead>
          <tbody>
            {#each kontoSide as k (k.number)}
              <tr class={k.active ? "" : "opacity-50"}>
                <td class="font-mono">
                  <a class="link" href={"#/c/" + companyId + "/hovedbok/" + k.number}>{k.number}</a>
                </td>
                <td>
                  {k.name}
                  {#if k.reskontro_kind}
                    <span class="badge badge-ghost badge-xs">{k.reskontro_kind}</span>
                  {/if}
                  {#if !k.active}<span class="badge badge-ghost badge-xs">deaktivert</span>{/if}
                </td>
                <td class="text-right">{kr(k.saldo_ore)}</td>
                <td class="text-right">{k.posteringer}</td>
                <td>
                  <button class="btn btn-xs btn-ghost" onclick={() => endreNavn(k)}>Navn</button>
                </td>
                <td>
                  <button class="btn btn-xs btn-ghost" onclick={() => toggleAktiv(k)}>
                    {k.active ? "Deaktiver" : "Aktiver"}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <div class="mt-2">
          <Paginering bind:side={kSide} antall={treffEgne.length} perSide={KONTOER_PER_SIDE} />
        </div>
      {:else}
        <p class="opacity-70 text-sm mb-2">Ingen kontoer i selskapet matcher søket.</p>
      {/if}

      {#if treffStandard.length}
        <p class="text-sm font-semibold mt-3 mb-1">Fra standardkontoplanen</p>
        <table class="table table-sm">
          <tbody>
            {#each treffStandard as s (s.number)}
              <tr>
                <td class="font-mono w-16">{s.number}</td>
                <td>{s.name}</td>
                <td class="text-right">
                  <button class="btn btn-xs btn-outline" onclick={() => leggTil(s.number, null)}>
                    + Legg til
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      <p class="text-sm font-semibold mt-3 mb-1">Egendefinert konto</p>
      <div class="flex gap-2 flex-wrap items-center">
        <input class="input input-sm w-24" placeholder="Nummer" bind:value={egenNummer} />
        <input
          class="input input-sm w-64"
          placeholder="Navn (standardnavn brukes om tomt)"
          bind:value={egenNavn}
        />
        <button
          class="btn btn-sm"
          disabled={!/^\d{4}$/.test(egenNummer.trim())}
          onclick={() => leggTil(egenNummer.trim(), egenNavn.trim() || null)}
        >
          Legg til konto
        </button>
      </div>
    </Card>
  {:else}
    <Card title="Nytt bilag">
      <p class="text-sm opacity-70 mb-2">
        Manuell postering rett i hovedboken — debet positivt, kredit negativt, summen må gå i null.
        Har bilaget et dokument, hører det hjemme i
        <a class="link" href={"#/c/" + companyId + "/bilag"}>innboksen</a> (vedlegget følger da
        bilaget).
      </p>
      <div class="flex gap-2 mb-2">
        <input type="date" class="input input-sm" bind:value={date} />
        <input class="input input-sm flex-1" placeholder="Tekst" bind:value={description} />
      </div>
      {#each lines as line}
        <div class="flex gap-2 mb-1 flex-wrap">
          <KontoVelger
            kontoer={data.kontoer}
            standard={data.standard}
            bind:value={line.account}
          />
          <input
            class="input input-sm w-32"
            placeholder="Beløp (−125,50)"
            bind:value={line.amount}
          />
          <input class="input input-sm w-16" placeholder="Mva" bind:value={line.vat} />
          <DimSelect {dims} kind="avdeling" cls="select select-sm w-28" bind:value={line.avdeling} />
          <DimSelect {dims} kind="prosjekt" cls="select select-sm w-28" bind:value={line.prosjekt} />
        </div>
      {/each}
      <div class="flex items-center gap-3 mt-1">
        <button class="btn btn-xs btn-ghost" onclick={() => lines.push(tomLinje())}>+ linje</button>
        <span class="text-sm {sum === 0 ? 'opacity-70' : 'text-error'}">
          Differanse: {kr(sum)}
        </span>
        <button class="btn btn-sm btn-primary" disabled={sum !== 0} onclick={bokfor}>Bokfør</button>
      </div>
    </Card>

    <Card title="Posteringer">
      <div class="flex items-center gap-3 mb-2 flex-wrap">
        <div class="join">
          <button class="btn btn-xs join-item" onclick={() => (year -= 1)}>«</button>
          <span class="btn btn-xs join-item pointer-events-none">{year}</span>
          <button class="btn btn-xs join-item" onclick={() => (year += 1)}>»</button>
        </div>
        <input
          class="input input-sm w-full max-w-xs"
          placeholder="Filtrer: bilag, dato, tekst, konto …"
          bind:value={pSok}
        />
        {#if posteringer}
          <span class="text-sm opacity-70">
            {posteringer.total} bilag{pSokAktiv.trim() ? " (filtrert)" : ""} — nyeste øverst
          </span>
        {/if}
      </div>
      {#if !posteringer}
        <span class="loading loading-spinner loading-sm"></span>
      {:else if !posteringer.vouchers.length}
        <p class="opacity-70 text-sm">
          {pSokAktiv.trim() ? "Ingen bilag matcher filteret." : "Ingen bilag i " + year + " ennå — det første kan føres over."}
        </p>
      {:else}
        {#each posteringer.vouchers as v (v.voucher)}
          <div class="mb-3">
            <span class="font-semibold">{v.journal}-{v.voucher}</span>
            <span class="opacity-70 text-sm">{v.date} — {v.description}</span>
            <table class="table table-sm">
              <tbody>
                {#each v.lines as l}
                  <tr>
                    <td>
                      <a
                        class="link font-mono"
                        href={"#/c/" + companyId + "/hovedbok/" + l.account}
                      >
                        {l.account}
                      </a>
                      {l.account_name}
                      {#if l.party_no}<span class="opacity-60">({l.party_no})</span>{/if}
                      {#if l.avdeling}<span class="badge badge-ghost badge-xs">{l.avdeling}</span>{/if}
                      {#if l.prosjekt}<span class="badge badge-ghost badge-xs">{l.prosjekt}</span>{/if}
                    </td>
                    <td>{l.vat_code ? "mva " + l.vat_code : ""}</td>
                    <td class="text-right">{kr(l.amount_ore)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/each}
        <Paginering bind:side={pSide} antall={posteringer.total} perSide={BILAG_PER_SIDE} />
      {/if}
    </Card>
  {/if}
{/if}
