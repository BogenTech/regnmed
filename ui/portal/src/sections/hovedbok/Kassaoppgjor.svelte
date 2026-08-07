<script>
  // Kassaoppgjør (#89, docs/kontantsalg.md): dagens Z-rapport som ett
  // bilag med mva-splitt, og kassadifferansen som SITT EGET bilag.
  // regnmed er ikke et kassasystem — dette bokfører oppgjøret fra ett.
  import { api, post } from "../../lib/api.js";
  import { kr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import KontoVelger from "../../components/KontoVelger.svelte";

  let { companyId, onDone } = $props();

  let dato = $state(today());
  let zNummer = $state("");
  let salg = $state([{ konto: "3000", vat_code: "3", belop: "" }]);
  let betaling = $state([{ konto: "1900", belop: "" }]);
  let kontantkonto = $state("1900");
  let opptalt = $state("");
  let fil = $state(null);
  let lagrer = $state(false);

  const ore = (v) => Math.round(parseFloat(String(v).replace(",", ".") || "0") * 100);
  let salgSum = $derived(salg.reduce((s, l) => s + ore(l.belop), 0));
  let betalingSum = $derived(betaling.reduce((s, b) => s + ore(b.belop), 0));
  // Vist før innsending: en Z-rapport som ikke går opp avvises av
  // serveren, og da er det bedre å se det med en gang enn å få en feil.
  let iBalanse = $derived(salgSum === betalingSum);

  async function bokfor() {
    lagrer = true;
    try {
      const kropp = {
        dato,
        z_nummer: zNummer,
        salg: salg
          .filter((l) => ore(l.belop))
          .map((l) => ({
            konto: l.konto,
            vat_code: l.vat_code || null,
            brutto_ore: ore(l.belop),
          })),
        betaling: betaling
          .filter((b) => ore(b.belop))
          .map((b) => ({ konto: b.konto, belop_ore: ore(b.belop) })),
        kontantkonto: opptalt === "" ? null : kontantkonto,
        opptalt_kontant_ore: opptalt === "" ? null : ore(opptalt),
      };
      let svar;
      if (fil) {
        svar = await api(
          "/companies/" + companyId + "/kassaoppgjor/rapport?filename=" +
            encodeURIComponent(fil.name),
          {
            method: "POST",
            headers: {
              "content-type": fil.type || "application/octet-stream",
              "x-dagsoppgjor": JSON.stringify(kropp),
            },
            body: fil,
          },
        );
      } else {
        svar = await post("/companies/" + companyId + "/kassaoppgjor", kropp);
      }
      toast(
        svar.differansebilag
          ? "Bokført som bilag " + svar.bilag + "; kassadifferanse " +
              kr(svar.differanse_ore) + " som bilag " + svar.differansebilag
          : "Bokført som bilag " + svar.bilag,
        true,
      );
      zNummer = "";
      salg = [{ konto: "3000", vat_code: "3", belop: "" }];
      betaling = [{ konto: "1900", belop: "" }];
      opptalt = "";
      fil = null;
      onDone?.();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }
</script>

<p class="text-sm opacity-70 mb-2">
  Dagsoppgjøret fra kassasystemet som ett bilag, med salget splittet per mva-sats
  (bokføringsforskriften §5-3 og §5-4). Z-nummeret knytter bilaget til kassasystemets egen
  nummererte rapport. <strong>regnmed er ikke et kassasystem</strong> — dette bokfører oppgjøret
  fra ett.
</p>

<div class="flex gap-2 mb-2 flex-wrap">
  <input type="date" class="input input-sm" bind:value={dato} />
  <input class="input input-sm w-40" placeholder="Z-nummer" bind:value={zNummer} />
</div>

<p class="font-semibold text-sm mt-2">Salg (brutto per sats)</p>
{#each salg as linje, i (i)}
  <div class="flex gap-2 mb-1 flex-wrap">
    <KontoVelger {companyId} bind:value={linje.konto} placeholder="Salgskonto" />
    <input class="input input-sm w-24" placeholder="Mva-kode" bind:value={linje.vat_code} />
    <input class="input input-sm w-32" placeholder="Brutto" bind:value={linje.belop} />
  </div>
{/each}
<button class="btn btn-xs mb-2" onclick={() => (salg = [...salg, { konto: "", vat_code: "3", belop: "" }])}>
  + sats
</button>

<p class="font-semibold text-sm mt-2">Betalingsmidler</p>
{#each betaling as b, i (i)}
  <div class="flex gap-2 mb-1 flex-wrap">
    <KontoVelger {companyId} bind:value={b.konto} placeholder="Konto (1900 / 1571 / 1920)" />
    <input class="input input-sm w-32" placeholder="Beløp" bind:value={b.belop} />
  </div>
{/each}
<button
  class="btn btn-xs mb-2"
  onclick={() => (betaling = [...betaling, { konto: "", belop: "" }])}
>
  + betalingsmiddel
</button>

<p class={"text-sm mb-2 " + (iBalanse ? "opacity-70" : "text-error")}>
  Salg {kr(salgSum)} mot betalingsmidler {kr(betalingSum)}
  {#if !iBalanse}
    — Z-rapporten går ikke opp, og det er ikke en kassadifferanse
  {/if}
</p>

<p class="font-semibold text-sm mt-2">Opptalt kasse (valgfritt)</p>
<div class="flex gap-2 mb-2 flex-wrap">
  <KontoVelger {companyId} bind:value={kontantkonto} placeholder="Kontantkonto" />
  <input class="input input-sm w-32" placeholder="Opptalt" bind:value={opptalt} />
</div>
<p class="text-xs opacity-70 mb-2">
  Uoppgitt = kassen ble ikke talt. Da bokføres ingen differanse — vi antar aldri at den stemte.
</p>

<input
  type="file"
  class="file-input file-input-sm w-full mb-2"
  onchange={(e) => (fil = e.currentTarget.files[0] || null)}
/>

<button class="btn btn-sm btn-primary" disabled={lagrer || !iBalanse} onclick={bokfor}>
  Bokfør dagsoppgjør
</button>
