<script>
  // Årsavslutning (#84, docs/arsavslutning.md): resultatdisponering og
  // skattekostnad som ETT ordinært bilag. Hører hjemme ved siden av
  // periodelåsen fordi avslutningen låser året selv — den er halvparten
  // av den samme handlingen.
  //
  // Two steps, like the asset disposal: "Se over" posts and rolls back
  // on the server and shows the voucher lines; the confirm carries the
  // result that was shown and is refused if the ledger moved since.
  import { tick } from "svelte";
  import { api, post } from "../../lib/api.js";
  import { formatDate, kr, today } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import YearPicker from "../../components/YearPicker.svelte";
  import { kanIkkeAvslutte, lesSkatt } from "./periode.js";

  // laastTil: the current lock, from the card above. onavsluttet: the
  // closing locks the year, so that card must reload.
  let { companyId, laastTil, erAdmin, onavsluttet } = $props();

  const iDag = today();
  let rader = $state(null); // null = loading, { error } = failed
  let ar = $state(Number(iDag.slice(0, 4)) - 1);
  let resultat = $state(null); // { ar, ore } | { ar, error }
  let skatt = $state("");
  let feil = $state("");
  let utkast = $state(null); // the preview's response
  let lagrer = $state(false);

  let kanAvslutte = $derived(
    harRett(companyId, "BILAG_BOKFOR") && harRett(companyId, "PERIODE_LAAS"),
  );
  let hindring = $derived(
    rader && !rader.error ? kanIkkeAvslutte(ar, iDag, rader, laastTil, erAdmin) : null,
  );

  async function last() {
    const id = companyId;
    try {
      const svar = (await api("/companies/" + id + "/arsavslutning")).arsavslutninger;
      if (id === companyId) rader = svar;
    } catch (error) {
      if (id === companyId) rader = { error: error.message };
    }
  }

  async function hentResultat() {
    const y = ar;
    try {
      const svar = await api("/companies/" + companyId + "/arsavslutning/" + y + "/forslag");
      if (y === ar) resultat = { ar: y, ore: svar.resultat_for_skatt_ore };
    } catch (error) {
      if (y === ar) resultat = { ar: y, error: error.message };
    }
  }

  $effect(() => {
    void companyId;
    rader = null;
    last();
  });

  // A new year starts over: the numbers on screen always belong to the
  // year the button names.
  $effect(() => {
    void ar;
    resultat = null;
    utkast = null;
    feil = "";
    if (kanAvslutte && rader && !rader.error && !hindring) hentResultat();
  });

  function endreSkatt(verdi) {
    skatt = verdi;
    utkast = null;
    feil = "";
  }

  async function seOver(event) {
    event.preventDefault();
    const lest = lesSkatt(skatt);
    if (lest.feil) {
      feil = lest.feil;
      document.getElementById("arsavslutning-skatt")?.focus();
      return;
    }
    lagrer = true;
    try {
      utkast = await post("/companies/" + companyId + "/arsavslutning?preview=true", {
        ar,
        skattekostnad_ore: lest.ore,
      });
      await tick();
      document.getElementById("arsavslutning-utkast")?.focus();
    } catch (error) {
      feil = error.message;
    } finally {
      lagrer = false;
    }
  }

  async function avslutt() {
    if (lagrer) return;
    lagrer = true;
    try {
      const svar = await post("/companies/" + companyId + "/arsavslutning", {
        ar: utkast.ar,
        skattekostnad_ore: utkast.skattekostnad_ore,
        forventet_resultat_ore: utkast.resultat_for_skatt_ore,
      });
      toast(
        `${svar.ar} er avsluttet med bilag ${svar.bilag}. ${kr(svar.disponert_ore)} kr er overført til egenkapitalen.`,
        true,
      );
      utkast = null;
      skatt = "";
      await last();
      await onavsluttet?.();
      await tick();
      document.getElementById("arsavslutning-historikk")?.focus();
    } catch (error) {
      utkast = null;
      feil = error.message;
    } finally {
      lagrer = false;
    }
  }
</script>

<Card title="Årsavslutning">
  <p class="text-sm opacity-70 mb-3">
    Overfører årets resultat til opptjent egenkapital og avsetter skattekostnaden, som ett bilag
    datert 31.12 (rskl. §6-1, §6-2). Avslutningen låser året, og kan ikke gjøres om; en rettelse er
    et reverserende bilag.
  </p>

  {#if !rader}
    <span class="loading loading-spinner loading-sm" aria-label="Laster årsavslutningene"></span>
  {:else if rader.error}
    <LoadError error={rader.error} retry={last} />
  {:else}
    {#if kanAvslutte}
      <div class="flex flex-wrap items-center gap-3 mb-2">
        <YearPicker bind:year={ar} />
        {#if !hindring}
          <p class="text-sm" aria-live="polite">
            {#if !resultat}
              <span class="loading loading-spinner loading-xs" aria-label="Henter resultatet"></span>
            {:else if resultat.error}
              <span class="text-error">{resultat.error}</span>
            {:else}
              Resultat før skatt {resultat.ar}: <strong>{kr(resultat.ore)} kr</strong>
            {/if}
          </p>
        {/if}
      </div>

      {#if hindring}
        <p class="text-sm opacity-80 mb-2">{hindring}</p>
      {:else if resultat && !resultat.error}
        <form class="mb-2" onsubmit={seOver} novalidate>
          <fieldset class="fieldset py-0">
            <legend class="fieldset-legend">Skattekostnad for {ar} (kr)</legend>
            <input
              id="arsavslutning-skatt"
              class="input input-sm w-44"
              inputmode="decimal"
              placeholder="0,00"
              aria-invalid={!!feil}
              aria-describedby="arsavslutning-skatt-hjelp"
              value={skatt}
              oninput={(e) => endreSkatt(e.currentTarget.value)}
            />
            <p id="arsavslutning-skatt-hjelp" class="label whitespace-normal">
              Oppgis av deg. Skattemessig resultat er ikke regnskapsmessig resultat, og systemet
              dikter ikke opp en skattemelding. 0 er et gyldig svar.
            </p>
          </fieldset>
          {#if feil}<p class="text-error text-sm mt-1" role="alert">{feil}</p>{/if}
          {#if !utkast}
            <button class="btn btn-sm btn-primary mt-2" disabled={lagrer}>
              {#if lagrer}<span class="loading loading-spinner loading-xs"></span>{/if}
              Se over avslutningen
            </button>
          {/if}
        </form>

        {#if utkast}
          <section
            id="arsavslutning-utkast"
            tabindex="-1"
            aria-label={"Avslutning av " + utkast.ar}
            class="border border-base-300 rounded-box p-3 mb-2 focus:outline-none"
          >
            <p class="font-medium mb-1">Bilag datert 31.12.{utkast.ar}</p>
            <ul class="text-sm mb-2">
              {#each utkast.linjer as l, i (i)}
                <li class="flex gap-3">
                  <span class="tabular-nums w-12">{l.konto}</span>
                  <span class="flex-1 min-w-0">{l.tekst}</span>
                  <span class="tabular-nums text-right whitespace-nowrap">{kr(l.belop_ore)}</span>
                </li>
              {/each}
            </ul>
            <p class="text-sm mb-1">
              Overføres til egenkapital: <strong>{kr(utkast.disponert_ore)} kr</strong>
            </p>
            <p class="text-sm opacity-70 mb-2">
              {#if utkast.relast_til}
                Perioden er låst til og med {formatDate(utkast.relast_til)}. Låsen flyttes tilbake
                til 30.12.{utkast.ar} mens bilaget føres, og settes tilbake til
                {formatDate(utkast.relast_til)}. Begge deler står i historikken.
              {:else}
                Perioden låses til og med 31.12.{utkast.ar}.
              {/if}
            </p>
            <div class="flex flex-wrap gap-2">
              <button class="btn btn-sm btn-primary" disabled={lagrer} onclick={avslutt}>
                {#if lagrer}<span class="loading loading-spinner loading-xs"></span>{/if}
                Avslutt {utkast.ar} og lås året
              </button>
              <button class="btn btn-sm btn-ghost" disabled={lagrer} onclick={() => (utkast = null)}>
                Avbryt
              </button>
            </div>
          </section>
        {/if}
      {/if}
    {/if}

    <h3 id="arsavslutning-historikk" tabindex="-1" class="font-semibold mt-4 mb-1 focus:outline-none">
      Avsluttede år
    </h3>
    {#if rader.length}
      <ul class="divide-y divide-base-200 text-sm">
        {#each rader as a (a.ar)}
          <li class="py-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
            <span class="font-medium">{a.ar}</span>
            <span class="opacity-70">bilag {a.bilag}</span>
            <span class="ml-auto tabular-nums whitespace-nowrap">
              {kr(a.disponert_ore)} kr til egenkapital
            </span>
            <span class="basis-full text-xs opacity-60 tabular-nums">
              Før skatt {kr(a.resultat_for_skatt_ore)} kr, skatt {kr(a.skattekostnad_ore)} kr, avsluttet
              av {a.created_by}
            </span>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="text-sm opacity-70">Ingen år er avsluttet ennå.</p>
    {/if}
  {/if}
</Card>
