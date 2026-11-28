<script>
  // Er leverandøren selvstendig næringsdrivende uten fast forretningssted?
  // Det er kundens vurdering — regnmed gjetter aldri (docs/selvstendig.md).
  // Bare en leverandør med «ja» kommer med i oppgaven til Skatteetaten.
  import { api, post } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId, party } = $props();

  const url = $derived("/companies/" + companyId + "/parties/" + party.party_id + "/self-employed");
  let data = $state(null);
  let valg = $state("");
  let fnr = $state("");
  let busy = $state(false);
  let kanSkrive = $derived(harRett(companyId, "RESKONTRO_SKRIV"));

  function vis(svar) {
    data = svar;
    valg = svar.gjeldende ? (svar.gjeldende.selvstendig ? "ja" : "nei") : "";
    fnr = "";
  }
  $effect(() => {
    api(url)
      .then(vis)
      .catch((e) => toast(e.message, false));
  });

  // Fødselsnummer trengs bare når leverandøren ikke har orgnr og ikke
  // allerede har ett lagret.
  let trengerFnr = $derived(valg === "ja" && !party.orgnr && !data?.gjeldende?.har_fodselsnummer);
  let kanLagre = $derived(valg !== "");

  async function lagre(event) {
    event.preventDefault();
    busy = true;
    try {
      const body = { selvstendig: valg === "ja" };
      if (valg === "ja" && !party.orgnr && fnr.trim()) body.fodselsnummer = fnr.trim();
      vis(await post(url, body));
      toast("Vurderingen er lagret", true);
    } catch (e) {
      toast(e.message, false);
    }
    busy = false;
  }
</script>

<section class="card border border-base-300 bg-base-100 max-w-xl mb-6" aria-labelledby="selvstendig-tittel">
  <div class="card-body">
    <h2 id="selvstendig-tittel" class="card-title">Selvstendig næringsdrivende</h2>
    <p class="text-sm opacity-70">
      Driver leverandøren selvstendig næringsvirksomhet <b>uten fast forretningssted</b>? Da skal det dere er
      fakturert i året rapporteres til Skatteetaten innen 15. februar. Er dere i tvil, skal det rapporteres.
      Aksjeselskaper og ansvarlige selskaper rapporteres ikke.
      <a class="link" href={"#/c/" + companyId + "/leverandorer/betalinger"}>Se oppgaven</a>
    </p>
    {#if !data}
      <span class="loading loading-spinner loading-sm" aria-label="Laster vurderingen"></span>
    {:else}
      {#if kanSkrive}
        <form class="grid gap-2" onsubmit={lagre}>
          <fieldset class="flex flex-wrap gap-4">
            <legend class="sr-only">Vurdering</legend>
            <label class="label"><input type="radio" class="radio radio-sm" bind:group={valg} value="ja" /> Ja, rapporteres</label>
            <label class="label"><input type="radio" class="radio radio-sm" bind:group={valg} value="nei" /> Nei</label>
          </fieldset>
          {#if valg === "ja" && !party.orgnr}
            <label class="fieldset py-0">
              <span class="fieldset-legend">
                Fødselsnummer eller D-nummer{data.gjeldende?.har_fodselsnummer ? " (lagret, fyll inn bare for å endre)" : ""}
              </span>
              <input class="input input-sm w-48 font-mono" inputmode="numeric" maxlength="11" autocomplete="off" bind:value={fnr} />
              <span class="text-xs opacity-70">Leverandøren har ikke organisasjonsnummer, så Skatteetaten må ha fødselsnummeret.</span>
            </label>
          {/if}
          {#if trengerFnr && !fnr.trim()}
            <p class="text-sm text-warning">Uten fødselsnummer kan oppgaven ikke leveres.</p>
          {/if}
          <div><button class="btn btn-primary btn-sm" disabled={!kanLagre || busy}>Lagre vurderingen</button></div>
        </form>
      {:else}
        <p class="text-sm">
          {data.gjeldende ? (data.gjeldende.selvstendig ? "Ja, rapporteres" : "Nei") : "Ikke vurdert"}
        </p>
      {/if}
      {#if data.historikk.length}
        <details class="text-sm mt-2">
          <summary class="cursor-pointer opacity-70">Historikk</summary>
          <ul class="mt-1">
            {#each data.historikk as h, i (i)}
              <li>
                {formatDate(h.vurdert.slice(0, 10))}: {h.selvstendig ? "ja" : "nei"}{h.fodselsdato
                  ? ", født " + formatDate(h.fodselsdato)
                  : ""} — {h.vurdert_av}
              </li>
            {/each}
          </ul>
        </details>
      {/if}
    {/if}
  </div>
</section>
