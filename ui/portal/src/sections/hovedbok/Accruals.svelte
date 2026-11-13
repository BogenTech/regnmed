<script>
  // Accruals (#87, docs/periodisering.md): a prepaid cost or unearned
  // income spread over the months it belongs to — one voucher per month,
  // dated month end, posted automatically on the 1st. The amount is NET:
  // the VAT was settled on the source voucher and is never spread
  // (mval. § 15-9).
  import { api, post } from "../../lib/api.js";
  import { kr, oreOrNull } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/periodiseringer");
  const thisMonth = () => new Date().toISOString().slice(0, 7);

  let plans = $state(null);
  let chart = $state({ kontoer: [], standard: [] });
  let form = $state(null);
  let busy = $state(false);
  let submitted = $state(false);
  let canPost = $derived(harRett(companyId, "BILAG_BOKFOR"));

  function load() {
    api(base)
      .then((r) => (plans = r.periodiseringer))
      .catch((e) => toast(e.message, false));
  }
  $effect(() => {
    load();
    api("/companies/" + companyId + "/accounts")
      .then((a) => (chart = { kontoer: a.kontoer || [], standard: a.standard || [] }))
      .catch(() => {});
  });

  function start() {
    const y = new Date().getFullYear();
    form = { text: "", income: "", balance: "1700", amount: "", from: thisMonth(), to: y + "-12" };
    submitted = false;
  }

  let amountOre = $derived(form ? oreOrNull(form.amount) : null);
  let months = $derived.by(() => {
    if (!form?.from || !form?.to) return 0;
    const [fy, fm] = form.from.split("-").map(Number);
    const [ty, tm] = form.to.split("-").map(Number);
    return (ty - fy) * 12 + (tm - fm) + 1;
  });
  let problem = $derived(
    !form
      ? null
      : !form.text.trim()
        ? "Skriv hva som periodiseres"
        : !form.income.trim()
          ? "Velg resultatkonto"
          : !form.balance.trim()
            ? "Velg balansekonto"
            : !(amountOre > 0)
              ? "Skriv beløpet uten mva, for eksempel 12 000"
              : months < 1
                ? "Til-måneden må være etter fra-måneden"
                : null,
  );

  async function save(event) {
    event.preventDefault();
    submitted = true;
    if (problem || busy) return;
    busy = true;
    const [fy, fm] = form.from.split("-").map(Number);
    const [ty, tm] = form.to.split("-").map(Number);
    try {
      await post(base, {
        beskrivelse: form.text.trim(),
        resultatkonto: form.income.trim(),
        balansekonto: form.balance.trim(),
        total_ore: amountOre,
        fra_ar: fy,
        fra_maned: fm,
        til_ar: ty,
        til_maned: tm,
      });
      toast("Periodiseringen er opprettet", true);
      form = null;
      load();
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }

  async function run(p) {
    try {
      const r = await post(base + "/" + p.id + "/kjor", {});
      const ok = r.kjort.filter((k) => k.bilag).length;
      // A failed month is NOT "nothing was due": say what went wrong.
      const failed = r.kjort.filter((k) => k.feil);
      if (failed.length) {
        toast((ok ? ok + " måned(er) bokført. " : "") + failed.length + " feilet: " + failed[0].feil, false);
      } else {
        toast(ok ? ok + " måned(er) bokført" : "Ingen måneder er forfalt ennå", true);
      }
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  async function stop(p) {
    if (!(await bekreft("Måneder som alt er bokført står. Resten bokføres ikke.", { tittel: "Stopp «" + p.beskrivelse + "»?", ok: "Stopp", farlig: true })))
      return;
    try {
      await post(base + "/" + p.id + "/stopp", {});
      load();
    } catch (e) {
      toast(e.message, false);
    }
  }

  const monthName = (iso) =>
    new Date(iso.slice(0, 7) + "-01T12:00").toLocaleDateString("nb-NO", { month: "long", year: "numeric" });
</script>

<section class="card border border-base-300 bg-base-100 max-w-4xl" aria-labelledby="per-tittel">
  <div class="card-body gap-3">
    <div class="flex flex-wrap items-center gap-3">
      <h2 id="per-tittel" class="card-title mr-auto">Periodisering</h2>
      {#if canPost && !form}<button class="btn btn-primary btn-sm" onclick={start}>Ny periodisering</button>{/if}
    </div>
    <p class="text-sm opacity-70">
      Fordeler en forskuddsbetalt kostnad eller en uopptjent inntekt over månedene den hører hjemme i (rskl. § 4-1). Ett bilag
      per måned, datert siste dag i måneden, bokført automatisk den 1. <strong>Beløpet er uten mva</strong>: avgiften ble gjort
      opp på kildebilaget.
    </p>

    {#if form}
      <form class="grid gap-3 sm:grid-cols-2 border border-base-300 rounded-box p-4" novalidate onsubmit={save}>
        <div class="fieldset py-0 sm:col-span-2">
          <label class="fieldset-legend pt-0" for="per-tekst">Hva periodiseres?</label>
          <input id="per-tekst" class="input w-full" placeholder="F.eks. Forsikring 2027" bind:value={form.text} />
        </div>
        <div class="fieldset py-0">
          <label class="fieldset-legend pt-0" for="per-belop">Beløp uten mva (kr)</label>
          <input id="per-belop" class="input w-full" inputmode="decimal" bind:value={form.amount} />
          {#if amountOre > 0 && months > 0}
            <span class="label">{kr(Math.round(amountOre / months))} kr per måned i {months} måneder</span>
          {/if}
        </div>
        <div class="grid grid-cols-2 gap-2">
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for="per-fra">Fra måned</label>
            <input id="per-fra" type="month" class="input w-full" bind:value={form.from} />
          </div>
          <div class="fieldset py-0">
            <label class="fieldset-legend pt-0" for="per-til">Til måned</label>
            <input id="per-til" type="month" class="input w-full" bind:value={form.to} />
          </div>
        </div>
        <div class="fieldset py-0">
          <label class="fieldset-legend pt-0" for="per-resultat">Resultatkonto</label>
          <KontoVelger id="per-resultat" kontoer={chart.kontoer} standard={chart.standard} bind:value={form.income} cls="input w-full" />
          <span class="label">Kostnaden eller inntekten, f.eks. 6300</span>
        </div>
        <div class="fieldset py-0">
          <label class="fieldset-legend pt-0" for="per-balanse">Balansekonto</label>
          <KontoVelger id="per-balanse" kontoer={chart.kontoer} standard={chart.standard} bind:value={form.balance} cls="input w-full" />
          <span class="label">1700 forskuddsbetalt, 2900 uopptjent</span>
        </div>
        {#if submitted && problem}<p class="text-error text-sm sm:col-span-2" role="alert">{problem}</p>{/if}
        <div class="flex gap-2 sm:col-span-2">
          <button class="btn btn-primary" type="submit" disabled={busy}>Opprett</button>
          <button class="btn btn-ghost" type="button" onclick={() => (form = null)}>Avbryt</button>
        </div>
      </form>
    {/if}

    {#if plans === null}
      <span class="loading loading-spinner"></span>
    {:else if plans.length === 0}
      <p class="opacity-70">Ingen periodiseringer ennå.</p>
    {:else}
      <ul class="grid gap-2">
        {#each plans as p (p.id)}
          <li class="border border-base-300 rounded-box p-3 flex flex-wrap items-center gap-x-4 gap-y-1">
            <div class="mr-auto min-w-0">
              <span class="font-medium">{p.beskrivelse}</span>
              {#if p.stoppet_dato}<span class="badge badge-ghost badge-sm ml-1">stoppet</span>{/if}
              <span class="block text-sm opacity-70">
                {monthName(p.fra_maned)}–{monthName(p.til_maned)} · {p.resultatkonto} mot {p.balansekonto}
              </span>
            </div>
            <span class="tabular-nums text-sm">{kr(p.fort_ore)} av {kr(p.total_ore)} kr ført ({p.forte_maneder} mnd)</span>
            {#if canPost && !p.stoppet_dato}
              <span class="flex gap-1">
                <button class="btn btn-sm" onclick={() => run(p)}>Bokfør forfalte måneder</button>
                <button class="btn btn-ghost btn-sm" onclick={() => stop(p)}>Stopp</button>
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>
