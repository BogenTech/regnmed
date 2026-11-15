<script>
  // One form for a new product and for editing one in place under its
  // row (`produkt` given). An edit sends only what changed, in ONE call;
  // deactivating is its own button because it is a different decision.
  // Price and account are checked here first, so an empty price never
  // becomes a 0 kr product.
  import { onMount, untrack } from "svelte";
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import VatSelect from "../../components/VatSelect.svelte";
  import KontoVelger from "../../components/KontoVelger.svelte";
  import { lesKonto, lesPris } from "./produkt.js";

  let { companyId, produkt = null, kontoer, onSaved, onClose = null } = $props();

  const ny = !untrack(() => produkt);
  const tom = () => ({ nummer: "", navn: "", pris: "", vat: "3", konto: "3000", lager: false });
  const fra = (p) => ({
    nummer: p.nummer,
    navn: p.navn,
    pris: String(p.salgspris_ore / 100).replace(".", ","),
    vat: p.vat_code ?? "",
    konto: p.konto,
    lager: p.lagerfort,
  });
  let felt = $state(untrack(() => (produkt ? fra(produkt) : tom())));
  let lagrer = $state(false);
  const id = (navn) => "produkt-" + (ny ? "ny" : felt.nummer) + "-" + navn;
  // The first field: the number for a new product, the name in an edit.
  const forste = () => document.getElementById(id(ny ? "nummer" : "navn"))?.focus();

  onMount(() => {
    if (!ny) forste();
  });

  const sti = () =>
    "/companies/" + companyId + "/products" + (ny ? "" : "/" + encodeURIComponent(produkt.nummer));

  async function lagre(event) {
    event.preventDefault();
    if (lagrer) return;
    const pris = lesPris(felt.pris);
    if (pris.feil) return toast(pris.feil, false);
    const konto = lesKonto(felt.konto);
    if (konto.feil) return toast(konto.feil, false);
    const verdier = {
      navn: felt.navn.trim(),
      salgspris_ore: pris.ore,
      vat_code: felt.vat || null,
      konto: konto.konto,
      lagerfort: felt.lager,
    };
    let body = verdier;
    if (!ny) {
      const gammel = {
        navn: produkt.navn,
        salgspris_ore: produkt.salgspris_ore,
        vat_code: produkt.vat_code ?? null,
        konto: produkt.konto,
        lagerfort: produkt.lagerfort,
      };
      body = Object.fromEntries(Object.entries(verdier).filter(([k, v]) => v !== gammel[k]));
      if (!Object.keys(body).length) return onClose?.();
    }
    lagrer = true;
    try {
      if (ny) await post(sti(), { nummer: felt.nummer.trim(), ...body });
      else await send("PUT", sti(), body);
      toast("Produkt " + (ny ? felt.nummer.trim() + " er opprettet" : produkt.nummer + " er lagret"), true);
      await onSaved();
      if (ny) {
        felt = tom();
        forste();
      } else onClose?.();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }

  async function aktiv() {
    try {
      await send("PUT", sti(), { aktiv: !produkt.aktiv });
      toast("Produkt " + produkt.nummer + (produkt.aktiv ? " er deaktivert" : " er aktivert igjen"), true);
      await onSaved();
      onClose?.();
    } catch (error) {
      toast(error.message, false);
    }
  }

  function tast(event) {
    if (!ny && event.key === "Escape") {
      event.preventDefault();
      onClose?.();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class={ny ? "mt-1 text-sm" : "mt-2 p-3 rounded-box bg-base-200 text-sm"} onkeydown={tast}>
  <form class="flex flex-wrap gap-x-3 gap-y-1 items-end" onsubmit={lagre}>
    {#if ny}
      <label class="fieldset py-0">
        <span class="fieldset-legend">Nummer</span>
        <input
          id={id("nummer")}
          class="input input-sm w-24"
          required
          title="Nummeret kan ikke endres senere"
          bind:value={felt.nummer}
        />
      </label>
    {/if}
    <label class="fieldset py-0 flex-1 min-w-40">
      <span class="fieldset-legend">Navn</span>
      <input id={id("navn")} class="input input-sm w-full" required bind:value={felt.navn} />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Pris eks. mva (kr)</span>
      <input class="input input-sm w-28" inputmode="decimal" required bind:value={felt.pris} />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Mva</span>
      <VatSelect cls="select select-sm w-36" uten bind:value={felt.vat} />
    </label>
    <label class="fieldset py-0" for={id("konto")}>
      <span class="fieldset-legend">Inntektskonto</span>
      <KontoVelger
        id={id("konto")}
        kontoer={kontoer.kontoer}
        standard={kontoer.standard}
        bind:value={felt.konto}
      />
    </label>
    <label class="label cursor-pointer gap-2 py-2">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={felt.lager} />
      <span>Lagerført</span>
    </label>
    <div class="flex gap-2 py-1">
      <button class="btn btn-sm btn-primary" disabled={lagrer}>
        {ny ? "Opprett produkt" : "Lagre"}
      </button>
      {#if !ny}
        <button type="button" class="btn btn-sm btn-ghost" onclick={() => onClose?.()}>Avbryt</button>
      {/if}
    </div>
  </form>
  {#if !ny}
    <div class="mt-2">
      <button type="button" class="btn btn-xs btn-outline" onclick={aktiv}>
        {produkt.aktiv ? "Deaktiver produktet" : "Aktiver produktet igjen"}
      </button>
      {#if produkt.aktiv}
        <span class="text-xs opacity-60 ml-1">
          Et deaktivert produkt kan ikke velges på nye fakturaer, og kan aktiveres igjen.
        </span>
      {/if}
    </div>
  {/if}
</div>
