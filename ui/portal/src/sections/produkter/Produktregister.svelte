<script>
  // Produktregisteret (#39): nummeret er permanent, og produkter
  // deaktiveres — de slettes aldri, fordi utstedte dokumenter har
  // kopiert verdiene sine.
  import { post, send } from "../../lib/api.js";
  import { kr, parseKr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { sporsmal } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import VatSelect from "../../components/VatSelect.svelte";

  let { companyId, products, onDone } = $props();

  let nummer = $state("");
  let navn = $state("");
  let pris = $state("");
  let vat = $state("3");
  let konto = $state("3000");
  let lager = $state(false);

  async function opprett() {
    try {
      await post("/companies/" + companyId + "/products", {
        nummer: nummer.trim(),
        navn: navn.trim(),
        salgspris_ore: parseKr(pris),
        vat_code: vat,
        konto: konto.trim() || null,
        lagerfort: lager,
      });
      toast("Produkt opprettet", true);
      nummer = "";
      navn = "";
      pris = "";
      vat = "3";
      konto = "3000";
      lager = false;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function nyPris(p) {
    const svar = await sporsmal("Ny salgspris (kr):", { ok: "Lagre" });
    if (!svar) return;
    try {
      await send("PUT", "/companies/" + companyId + "/products/" + encodeURIComponent(p.nummer), {
        salgspris_ore: parseKr(svar),
      });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function toggle(p) {
    try {
      await send("PUT", "/companies/" + companyId + "/products/" + encodeURIComponent(p.nummer), {
        aktiv: !p.aktiv,
      });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Produktregister">
  <p class="text-sm opacity-70 mb-2">
    Linjer på faktura og tilbud kopierer produktverdiene ved utstedelse — endringer her rører aldri
    utstedte dokumenter. Nummer er permanent; produkter deaktiveres, slettes aldri.
  </p>
  <div class="flex flex-wrap gap-2 items-end">
    <input class="input input-sm w-24" placeholder="Nummer" bind:value={nummer} />
    <input class="input input-sm" placeholder="Navn" bind:value={navn} />
    <input class="input input-sm w-28" placeholder="Pris (kr)" bind:value={pris} />
    <VatSelect cls="select select-sm" bind:value={vat} />
    <input
      class="input input-sm w-20"
      title="Inntektskonto"
      bind:value={konto}
    />
    <label class="label cursor-pointer gap-1">
      <span class="text-sm">Lager</span>
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={lager} />
    </label>
    <button class="btn btn-sm" onclick={opprett}>Nytt produkt</button>
  </div>
  {#if products.length}
    <table class="table table-sm mt-3">
      <thead>
        <tr>
          <th>Nr</th><th>Navn</th><th class="text-right">Pris</th>
          <th>Mva</th><th>Konto</th><th>Lager</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each products as p (p.nummer)}
          <tr class={p.aktiv ? "" : "opacity-50"}>
            <td class="font-mono">{p.nummer}</td>
            <td>{p.navn}</td>
            <td class="text-right">{kr(p.salgspris_ore)}</td>
            <td>{p.vat_code || "–"}</td>
            <td>{p.konto}</td>
            <td>{p.lagerfort ? "✓" : ""}</td>
            <td>
              <button class="btn btn-xs btn-ghost" onclick={() => nyPris(p)}>Pris</button>
              <button class="btn btn-xs btn-ghost" onclick={() => toggle(p)}>
                {p.aktiv ? "Deaktiver" : "Aktiver"}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
