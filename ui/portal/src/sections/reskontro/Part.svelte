<script>
  // Partssiden: postene med åpent-filter, kontaktinfo, og manuell
  // matching av åpne poster. Matchen tar alltid den positive resten mot
  // den negative (entry_a mot entry_b), men HVA de to sidene er snur med
  // parts-typen: kundens faktura er debet og innbetalingen kredit,
  // leverandørens faktura er kredit og utbetalingen debet. Derfor er
  // etikettene styrt av `kind` — en «Faktura»-merkelapp over
  // utbetalingen ville fått brukeren til å matche feil vei.
  import { untrack } from "svelte";
  import { api, post, send } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, party, partyId, tilbake = "reskontro" } = $props();

  // Parts-typen kommer fra listen; mangler den (direkte lenke), er
  // kunde det trygge valget — samme etiketter som før.
  let erLeverandor = $derived(party?.kind === "leverandor");
  let debetTekst = $derived(erLeverandor ? "Utbetaling" : "Faktura");
  let kreditTekst = $derived(erLeverandor ? "Leverandørfaktura" : "Innbetaling");

  let items = $state(null);
  let kunApne = $state(false);
  let address = $state(untrack(() => party?.address || ""));
  let email = $state(untrack(() => party?.email || ""));
  let bankAccount = $state(untrack(() => party?.bank_account || ""));

  let kanMatche = $derived(harRett(companyId, "RESKONTRO_SKRIV"));

  function last() {
    items = null;
    api(
      "/companies/" + companyId + "/parties/" + partyId + "/items" + (kunApne ? "?open=true" : ""),
    )
      .then((svar) => (items = svar.items))
      .catch((error) => toast(error.message, false));
  }

  $effect(() => {
    kunApne;
    last();
  });

  let debetSide = $derived((items || []).filter((i) => i.remaining_ore > 0));
  let kreditSide = $derived((items || []).filter((i) => i.remaining_ore < 0));

  let entryA = $state("");
  let entryB = $state("");

  // Foreslått beløp = det minste av de to restene, som er det største
  // matchen kan være. Brukeren kan skrive mindre (delbetaling).
  let foreslatt = $derived.by(() => {
    const a = debetSide.find((i) => i.entry_id === entryA);
    const b = kreditSide.find((i) => i.entry_id === entryB);
    if (!a || !b) return 0;
    return Math.min(a.remaining_ore, -b.remaining_ore);
  });
  let belop = $state("");

  async function match(event) {
    event.preventDefault();
    const ore = belop.trim() ? Math.round(Number(belop.replace(",", ".")) * 100) : foreslatt;
    if (!(ore > 0)) {
      toast("Beløpet må være positivt", false);
      return;
    }
    try {
      await post("/companies/" + companyId + "/reskontro/matches", {
        entry_a: entryA,
        entry_b: entryB,
        amount_ore: ore,
      });
      toast("Postene er matchet", true);
      entryA = "";
      entryB = "";
      belop = "";
      last();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function save() {
    try {
      await send("PUT", "/companies/" + companyId + "/parties/" + partyId + "/contact", {
        address,
        email,
        bank_account: bankAccount,
      });
      toast("Kontaktinfo lagret", true);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title={party?.name || ""}>
  <a href={"#/c/" + companyId + "/" + tilbake} class="btn btn-ghost btn-xs w-fit">← tilbake</a>
  <label class="label cursor-pointer w-fit gap-2">
    <input type="checkbox" class="checkbox checkbox-sm" bind:checked={kunApne} />
    <span class="label-text">Bare åpne poster</span>
  </label>
  {#if items}
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Bilag</th><th>Dato</th><th>Tekst</th>
          <th class="text-right">Beløp</th><th class="text-right">Åpent</th>
        </tr>
      </thead>
      <tbody>
        {#each items as i (i.entry_id)}
          <tr>
            <td>{i.voucher}</td>
            <td>{i.date}</td>
            <td>{i.description || ""}</td>
            <td class="text-right">{kr(i.amount_ore)}</td>
            <td class="text-right">{kr(i.remaining_ore)}</td>
          </tr>
        {:else}
          <tr>
            <td colspan="5" class="opacity-70">
              {kunApne ? "Ingen åpne poster." : "Ingen poster."}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <span class="loading loading-spinner loading-sm"></span>
  {/if}
</Card>

{#if kanMatche && debetSide.length && kreditSide.length}
  <Card title="Match åpne poster">
    <form class="grid gap-2 max-w-2xl" onsubmit={match}>
      <label class="form-control">
        <span class="label-text">{debetTekst}</span>
        <select class="select select-sm" required bind:value={entryA}>
          <option value="" disabled>Velg post</option>
          {#each debetSide as i (i.entry_id)}
            <option value={i.entry_id}>
              {i.voucher} · {i.date} · {kr(i.remaining_ore)} åpent
            </option>
          {/each}
        </select>
      </label>
      <label class="form-control">
        <span class="label-text">{kreditTekst}</span>
        <select class="select select-sm" required bind:value={entryB}>
          <option value="" disabled>Velg post</option>
          {#each kreditSide as i (i.entry_id)}
            <option value={i.entry_id}>
              {i.voucher} · {i.date} · {kr(i.remaining_ore)} åpent
            </option>
          {/each}
        </select>
      </label>
      <label class="form-control">
        <span class="label-text">Beløp (tomt = {kr(foreslatt)})</span>
        <input class="input input-sm w-40" placeholder={(foreslatt / 100).toFixed(2)} bind:value={belop} />
      </label>
      <button class="btn btn-sm btn-primary w-fit">Match</button>
    </form>
  </Card>
{/if}

{#if party}
  <Card title="Kontaktinfo">
    <div class="grid gap-2 max-w-md">
      <input
        class="input input-sm"
        placeholder="Adresse (på fakturaen)"
        bind:value={address}
      />
      <input
        class="input input-sm"
        placeholder="E-post (for utsendelse)"
        bind:value={email}
      />
      <input
        class="input input-sm"
        placeholder="Kontonummer (for remittering)"
        bind:value={bankAccount}
      />
      <button class="btn btn-sm" onclick={save}>Lagre</button>
    </div>
  </Card>
{/if}
