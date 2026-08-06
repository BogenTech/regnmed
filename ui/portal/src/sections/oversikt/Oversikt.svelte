<script>
  import { api } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Nokkeltall from "./Nokkeltall.svelte";
  import ImportKort from "./ImportKort.svelte";
  import InviterKort from "./InviterKort.svelte";
  import Valutakurser from "./Valutakurser.svelte";
  import Forankring from "./Forankring.svelte";

  let { companyId } = $props();

  let data = $state(null);

  async function load(id) {
    const year = new Date().getFullYear();
    const termin = Math.floor(new Date().getMonth() / 2) + 1;
    // Abonnement og firmaopplysninger flyttet til Administrasjon-
    // konsollen — oversikten henter bare det den selv viser.
    const [open, mva, lock, vouchers, overdue, rates, tall, anchors] = await Promise.all([
      api("/companies/" + id + "/invoices?open=true"),
      api("/companies/" + id + "/reports/mva?year=" + year + "&termin=" + termin).catch(
        () => null,
      ),
      api("/companies/" + id + "/period-lock"),
      api("/companies/" + id + "/vouchers"),
      api("/companies/" + id + "/invoices/overdue").catch(() => null),
      api("/companies/" + id + "/currency/rates").catch(() => ({ rates: [] })),
      api("/companies/" + id + "/reports/nokkeltall").catch(() => null),
      api("/companies/" + id + "/anchors").catch(() => ({ anchors: [] })),
    ]);
    data = {
      termin,
      open: open.invoices,
      mva,
      lock,
      vouchers: vouchers.vouchers,
      overdue,
      rates: rates.rates,
      tall,
      anchors: anchors.anchors,
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });

  let openSum = $derived(data ? data.open.reduce((sum, i) => sum + i.remaining_ore, 0) : 0);
  let overdueSum = $derived(
    data?.overdue ? data.overdue.invoices.reduce((sum, i) => sum + i.remaining_ore, 0) : 0,
  );
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <!-- Stack on narrow screens instead of scrolling sideways. -->
  <div class="stats stats-vertical lg:stats-horizontal border border-base-300 bg-base-100 w-full mb-6">
    <div class="stat">
      <div class="stat-title">Utestående fakturaer</div>
      <div class="stat-value text-2xl">{kr(openSum)}</div>
      <div class="stat-desc">{data.open.length} åpne</div>
    </div>
    <div class="stat">
      <div class="stat-title">Forfalt</div>
      <div class="stat-value text-2xl {overdueSum > 0 ? 'text-error' : ''}">{kr(overdueSum)}</div>
      <div class="stat-desc">
        {#if data.overdue?.invoices.length}
          <a class="link" href={"#/c/" + companyId + "/faktura"}>
            {data.overdue.invoices.length} til oppfølging
          </a>
        {:else}
          ingenting forfalt
        {/if}
      </div>
    </div>
    <div class="stat">
      <div class="stat-title">Mva {data.termin}. termin</div>
      <div class="stat-value text-2xl">{data.mva ? kr(data.mva.netto_ore) : "–"}</div>
      <div class="stat-desc">{data.mva && data.mva.netto_ore >= 0 ? "å betale" : "til gode"}</div>
    </div>
    <div class="stat">
      <div class="stat-title">Periode låst t.o.m.</div>
      <div class="stat-value text-2xl">{data.lock.locked_through || "åpen"}</div>
    </div>
  </div>

  {#if data.tall}
    <Nokkeltall tall={data.tall} />
  {/if}

  {#if data.vouchers.length === 0}
    <ImportKort {companyId} onDone={reload} />
  {/if}

  <InviterKort {companyId} />

  <Card title="Siste bilag">
    <table class="table table-sm">
      <thead><tr><th>Bilag</th><th>Dato</th><th>Tekst</th></tr></thead>
      <tbody>
        {#each data.vouchers.slice(0, 8) as v}
          <tr><td>{v.voucher}</td><td>{v.date}</td><td>{v.description}</td></tr>
        {/each}
      </tbody>
    </table>
  </Card>

  <Valutakurser {companyId} rates={data.rates} onDone={reload} />

  <Forankring {companyId} anchors={data.anchors} />
{/if}
