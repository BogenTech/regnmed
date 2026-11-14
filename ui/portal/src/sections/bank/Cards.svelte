<script>
  // The company's cards (docs/utlegg.md «Firmakort»). Each card has its
  // own ledger account — a debit card the bank account it draws on, a
  // credit card an account of its own — so its statement is imported and
  // reconciled under Avstemming like any bank's, and a purchase's receipt
  // is booked against that account instead of owed to the employee. Only
  // the last four digits are stored, never the card number.
  import { api, post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId } = $props();

  const base = $derived("/companies/" + companyId + "/cards");
  // The register is changed by whoever reconciles the bank.
  const canManage = harRett(companyId, "BANK_AVSTEM");

  let cards = $state(null);
  let people = $state([]);
  let chart = $state({ kontoer: [], standard: [] });
  let name = $state("");
  let last4 = $state("");
  let holder = $state("");
  let account = $state("");
  let busy = $state(false);

  function load() {
    api(base)
      .then((r) => (cards = r.cards))
      .catch((err) => (cards = { error: err.message }));
  }
  $effect(() => {
    load();
    api("/companies/" + companyId + "/expenses/claimants").then((r) => (people = r.people)).catch(() => {});
    api("/companies/" + companyId + "/accounts")
      .then((a) => (chart = { kontoer: a.kontoer || [], standard: a.standard || [] }))
      .catch(() => {});
  });

  async function create(event) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    try {
      const nr = account.trim();
      // A catalog account becomes the company's the first time it is used.
      if (!chart.kontoer.some((k) => k.number === nr) && chart.standard.some((s) => s.number === nr)) {
        await post("/companies/" + companyId + "/accounts", { number: nr });
      }
      await post(base, { name, last4: last4.trim(), holder_person_id: holder || null, konto: nr });
      toast("Kortet er registrert", true);
      name = last4 = holder = account = "";
      event.target.reset();
      load();
    } catch (err) {
      toast(err.message, false);
    }
    busy = false;
  }

  async function update(c, changes) {
    try {
      await send("PUT", base + "/" + c.card_id, {
        name: c.name,
        holder_person_id: c.holder_person_id,
        active: c.active,
        ...changes,
      });
      load();
    } catch (err) {
      toast(err.message, false);
    }
  }
</script>

<section class="card border border-base-300 bg-base-100 max-w-4xl" aria-labelledby="kort-tittel">
  <div class="card-body grid gap-4">
    <h2 id="kort-tittel" class="card-title">Firmakort</h2>
    <p class="text-sm opacity-70">
      Kjøp med firmakort er firmaets penger: kvitteringen bokføres mot kortets konto, og ingenting utbetales til den
      ansatte. Importer kortets kontoutskrift under Avstemming på kortets konto — kjøp uten kvittering vises da for
      kortholderen under Utlegg.
    </p>

    {#if cards === null}
      <span class="loading loading-spinner"></span>
    {:else if cards.error}
      <div class="alert alert-error" role="alert"><span>{cards.error}</span></div>
    {:else if cards.length}
      <div class="overflow-x-auto">
        <table class="table table-sm">
          <caption class="sr-only">Registrerte firmakort</caption>
          <thead>
            <tr><th scope="col">Kort</th><th scope="col">Kortholder</th><th scope="col">Konto</th><th scope="col">I bruk</th></tr>
          </thead>
          <tbody>
            {#each cards as c (c.card_id)}
              <tr class={c.active ? "" : "opacity-60"}>
                <td>{c.label}</td>
                <td>
                  <select
                    disabled={!canManage}
                    class="select select-sm"
                    aria-label={"Kortholder for " + c.label}
                    value={c.holder_person_id || ""}
                    onchange={(ev) => update(c, { holder_person_id: ev.target.value || null })}
                  >
                    <option value="">Delt kort</option>
                    {#each people as p (p.person_id)}<option value={p.person_id}>{p.name}</option>{/each}
                    {#if c.holder_person_id && !people.some((p) => p.person_id === c.holder_person_id)}
                      <option value={c.holder_person_id}>{c.holder}</option>
                    {/if}
                  </select>
                </td>
                <td><span class="font-mono">{c.account}</span> <span class="opacity-70">{c.account_name}</span></td>
                <td>
                  <input
                    type="checkbox"
                    disabled={!canManage}
                    class="toggle toggle-sm"
                    aria-label={(c.active ? "Ta ut av bruk: " : "Ta i bruk: ") + c.label}
                    checked={c.active}
                    onchange={(ev) => update(c, { active: ev.target.checked })}
                  />
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="opacity-70">Ingen firmakort er registrert.</p>
    {/if}

    {#if canManage}
    <form class="grid gap-3 sm:grid-cols-2 lg:grid-cols-[1fr_6rem_1fr_10rem_auto] items-end" onsubmit={create}>
      <label class="fieldset">
        <span class="fieldset-legend">Navn</span>
        <input class="input w-full" placeholder="F.eks. Visa bedrift" bind:value={name} required />
      </label>
      <label class="fieldset">
        <span class="fieldset-legend">Siste fire</span>
        <input class="input w-full" inputmode="numeric" maxlength="4" pattern={"[0-9]{4}"} placeholder="1234" bind:value={last4} required />
      </label>
      <label class="fieldset">
        <span class="fieldset-legend">Kortholder</span>
        <select class="select w-full" bind:value={holder}>
          <option value="">Delt kort (bare de som godkjenner utlegg)</option>
          {#each people as p (p.person_id)}<option value={p.person_id}>{p.name}</option>{/each}
        </select>
      </label>
      <div class="fieldset">
        <label class="fieldset-legend" for="kort-konto">Konto</label>
        <KontoVelger id="kort-konto" kontoer={chart.kontoer} standard={chart.standard} bind:value={account} cls="input w-full" />
      </div>
      <button class="btn btn-primary" type="submit" disabled={busy || !name.trim() || !/^\d{4}$/.test(last4.trim()) || !account.trim()}>
        Registrer kort
      </button>
    </form>
    <p class="text-xs opacity-70">
      Ett kort per konto, så kontoutskriften kan avstemmes per kort. Et debetkort kan stå på bankkontoen det trekker
      fra; et kredittkort trenger en egen konto (for eksempel en underkonto i 2390-serien).
    </p>
    {/if}
  </div>
</section>
