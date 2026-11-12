<script>
  // Oversikten svarer på «hva må jeg gjøre i dag?» før «hvordan går
  // det?»: snarveier, så det som venter, så tallene. Verktøy og
  // innstillinger bor i sine egne seksjoner (valutakurser i Bank,
  // forankringen under Rapporter → Revisjon).
  import { api } from "../../lib/api.js";
  import { kr, krWhole, formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Ikon from "../../components/Ikon.svelte";
  import Nokkeltall from "./Nokkeltall.svelte";
  import ImportKort from "./ImportKort.svelte";
  import InviterKort from "./InviterKort.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const c = "/companies/" + id;
    // Every fetch but the first two degrades to null: a viewer without
    // a right still gets an overview, just with fewer lines in it.
    const [open, vouchers, recent, overdue, tall, inbox, recon, anchors] = await Promise.all([
      api(c + "/invoices?open=true"),
      api(c + "/vouchers"),
      api(c + "/vouchers?limit=8&lines=true").catch(() => null),
      api(c + "/invoices/overdue").catch(() => null),
      api(c + "/reports/nokkeltall").catch(() => null),
      api(c + "/inbox").catch(() => null),
      api(c + "/bank/reconciliation?account=1920").catch(() => null),
      api(c + "/anchors").catch(() => ({ anchors: [] })),
    ]);
    data = {
      open: open.invoices,
      vouchers: vouchers.vouchers,
      recent: recent?.vouchers ?? vouchers.vouchers.slice(0, 8),
      overdue: overdue?.invoices ?? [],
      tall,
      inbox: inbox ? inbox.documents.filter((d) => d.status === "ny") : [],
      unmatched: recon?.unmatched_bank ?? [],
      anchor: anchors.anchors[0],
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });

  const sum = (rows) => rows.reduce((s, r) => s + r.remaining_ore, 0);
  const flertall = (n, en, flere) => n + " " + (n === 1 ? en : flere);
  const dagerTil = (iso) =>
    Math.round((Date.parse(iso + "T00:00") - new Date().setHours(0, 0, 0, 0)) / 864e5);
  // A voucher's size is its debit side; the lines always balance.
  const bilagSum = (v) => (v.lines || []).reduce((s, l) => s + Math.max(l.amount_ore, 0), 0);

  const href = (seksjon) => "#/c/" + companyId + "/" + seksjon;
  let openSum = $derived(data ? sum(data.open) : 0);
  let overdueSum = $derived(data ? sum(data.overdue) : 0);

  // What is waiting on the user, most pressing first. Each line names
  // the work and links straight to where it is done.
  let oppfolging = $derived.by(() => {
    if (!data) return [];
    const linjer = [];
    if (data.overdue.length) {
      linjer.push({
        seksjon: "faktura",
        ikon: "faktura",
        tekst: flertall(data.overdue.length, "forfalt faktura", "forfalte fakturaer"),
        detalj: krWhole(overdueSum) + " kr ubetalt",
        haster: true,
      });
    }
    const frist = data.tall?.frister?.[0];
    if (frist) {
      const dager = dagerTil(frist.frist);
      linjer.push({
        seksjon: "mva",
        ikon: "mva",
        tekst: "Lever mva-melding for " + frist.label,
        detalj:
          "frist " + formatDate(frist.frist) + (dager === 0 ? ", i dag" : ", om " + flertall(dager, "dag", "dager")),
        haster: dager <= 7,
      });
    }
    if (data.inbox.length) {
      linjer.push({
        seksjon: "bilag",
        ikon: "bilag",
        tekst: flertall(data.inbox.length, "dokument", "dokumenter") + " i innboksen",
        detalj: "venter på bokføring",
      });
    }
    if (data.unmatched.length) {
      linjer.push({
        seksjon: "bank",
        ikon: "bank",
        tekst: flertall(data.unmatched.length, "banktransaksjon", "banktransaksjoner") + " uten match",
        detalj: "avstem mot hovedboken",
      });
    }
    return linjer;
  });
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <div class="flex flex-wrap gap-2 mb-6">
    <a class="btn btn-primary btn-sm" href={href("faktura")}><Ikon navn="faktura" />Ny faktura</a>
    <a class="btn btn-sm" href={href("bilag")}><Ikon navn="bilag" />Last opp bilag</a>
    <a class="btn btn-sm" href={href("timer")}><Ikon navn="timer" />Registrer timer</a>
  </div>

  <!-- Import stays available while the ledger holds nothing but imported
       history: systems like Conta export one SAF-T file per fiscal year,
       so the years arrive one upload at a time. The server enforces the
       rule; this condition only mirrors it. -->
  {#if data.vouchers.length === 0 || data.vouchers.every((v) => v.journal === "IMP")}
    <ImportKort {companyId} onDone={reload} harHistorikk={data.vouchers.length > 0} />
  {/if}

  <Card title="Til oppfølging">
    {#if oppfolging.length}
      <ul class="-mx-2">
        {#each oppfolging as l (l.seksjon)}
          <li>
            <a class="flex items-center gap-3 rounded-box px-2 py-2 hover:bg-base-200" href={href(l.seksjon)}>
              <span class={l.haster ? "text-error" : "opacity-60"}><Ikon navn={l.ikon} /></span>
              <span class="flex-1 min-w-0">
                <span class="font-medium {l.haster ? 'text-error' : ''}">{l.tekst}</span>
                <span class="block text-sm opacity-70">{l.detalj}</span>
              </span>
            </a>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="text-sm opacity-70">Ingenting venter på deg nå.</p>
    {/if}
  </Card>

  <!-- Stack on narrow screens instead of scrolling sideways. -->
  <div class="stats stats-vertical md:stats-horizontal border border-base-300 bg-base-100 w-full mb-6">
    <a class="stat hover:bg-base-200" href={href("faktura")}>
      <div class="stat-title">Utestående fra kunder</div>
      <div class="stat-value text-2xl">{krWhole(openSum)}</div>
      <div class="stat-desc">
        {#if overdueSum}
          <span class="text-error">{krWhole(overdueSum)} forfalt</span>
          av {flertall(data.open.length, "faktura", "fakturaer")}
        {:else}
          {flertall(data.open.length, "åpen faktura", "åpne fakturaer")}, ingen forfalt
        {/if}
      </div>
    </a>
    {#if data.tall}
      <a class="stat hover:bg-base-200" href={href("rapporter/resultat")}>
        <div class="stat-title">Resultat hittil i {data.tall.year}</div>
        <div class="stat-value text-2xl {data.tall.resultat_hittil_ore < 0 ? 'text-error' : ''}">
          {krWhole(data.tall.resultat_hittil_ore)}
        </div>
        <div class="stat-desc">Samme dato i fjor: {krWhole(data.tall.resultat_fjor_ore)}</div>
      </a>
      <div
        class="stat"
        title={"Bank " + kr(data.tall.likviditet.bank_ore) +
          " + kunder " + kr(data.tall.likviditet.kundefordringer_ore) +
          " − leverandører " + kr(data.tall.likviditet.leverandorgjeld_ore) +
          " − mva " + kr(Math.max(data.tall.likviditet.mva_netto_ore, 0))}
      >
        <div class="stat-title">Disponibelt</div>
        <div class="stat-value text-2xl {data.tall.likviditet.disponibelt_ore < 0 ? 'text-error' : ''}">
          {krWhole(data.tall.likviditet.disponibelt_ore)}
        </div>
        <div class="stat-desc">bank og kunder, minus leverandører og mva</div>
      </div>
    {/if}
  </div>

  {#if data.tall?.maaneder.some((v) => v !== 0)}
    <Nokkeltall tall={data.tall} />
  {/if}

  <Card title="Siste bilag">
    {#if data.recent.length}
      <table class="table table-sm">
        <thead>
          <tr><th class="hidden sm:table-cell">Bilag</th><th>Dato</th><th>Tekst</th><th class="text-right">Beløp</th></tr>
        </thead>
        <tbody>
          {#each data.recent as v (v.voucher_id)}
            <tr>
              <td class="hidden sm:table-cell whitespace-nowrap">{v.voucher}</td>
              <td class="whitespace-nowrap">{formatDate(v.date)}</td>
              <td>{v.description}</td>
              <td class="text-right tabular-nums whitespace-nowrap">{v.lines ? kr(bilagSum(v)) : ""}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <a class="link text-sm" href={href("hovedbok")}>Se alle bilag i hovedboken</a>
    {:else}
      <p class="text-sm opacity-70">
        Ingen bilag ennå. Lag en <a class="link" href={href("faktura")}>faktura</a> eller
        <a class="link" href={href("bilag")}>last opp et bilag</a>, så dukker de opp her.
      </p>
    {/if}
  </Card>

  <InviterKort {companyId} />

  <p class="text-sm opacity-70">
    {#if data.anchor}
      Hovedboken ble sist forankret {formatDate(data.anchor.created_at)}{data.anchor.witnesses.length
        ? " og bevitnet eksternt"
        : ""}.
    {:else}
      Hovedboken er ikke forankret ennå. Det skjer automatisk hver natt.
    {/if}
    <a class="link" href={href("rapporter/revisjon")}>Kontroller at ingenting er endret</a>
  </p>
{/if}
