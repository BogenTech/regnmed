<script>
  // Firmaopplysningene som trykkes på salgsdokumentet. Adressen er
  // LOVPÅLAGT (bokføringsforskriften §5-1-2) og utstedelsen nekter uten
  // den, så kortet sier fra i stedet for å la brukeren oppdage det
  // først når en faktura avvises.
  //
  // Registreringsstatusen er DATERT og lagres (#81): den avgjør «MVA» og
  // «Foretaksregisteret» på dokumentet, og utledes aldri av hva
  // dokumentet tilfeldigvis inneholder. En endring blir en ny rad med
  // egen fra-dato, så eldre dokumenter beholder statusen de ble utstedt
  // under.
  import { untrack } from "svelte";
  import { post, send } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import { KILDE } from "../admin/admin.js";

  let { companyId, settings, onDone = () => {} } = $props();

  let kanEndre = $derived(harRett(companyId, "SELSKAP_ADMIN"));

  let address = $state(untrack(() => settings.address || ""));
  let email = $state(untrack(() => settings.email || ""));
  let bankAccount = $state(untrack(() => settings.bank_account || ""));
  let orgform = $state(untrack(() => settings.orgform || ""));
  // KID hører til bankavtalen, ikke til fakturanummeret (#104): uten
  // avtale trykkes ingen KID, for betaleren kan ikke bruke den.
  let kidAvtale = $state(untrack(() => !!settings.kid_avtale));
  // Whose diett rates travel claims are paid with (docs/utlegg.md
  // «Reise og diett»); a submitted claim keeps the choice it went in with.
  let reiseSatser = $state(untrack(() => settings.reise_satser || "trekkfri"));

  let mva = $state(untrack(() => !!settings.mva_registrert));
  let freg = $state(untrack(() => !!settings.foretaksregistrert));
  let fraDato = $state("");
  let historikk = $derived(settings.registrering_historikk || []);
  // En 'migrert' rad er den gamle utledningen, ikke en observasjon —
  // den skal bekreftes mot Enhetsregisteret.
  let ubekreftet = $derived(historikk.length > 0 && historikk[0].kilde === "migrert");

  let lagrer = $state(false);
  let melding = $state(null); // { ok, tekst } next to the button
  let lagrerReg = $state(false);
  let meldingReg = $state(null);

  const ORGFORMER = ["", "AS", "ASA", "ENK", "ANS", "DA", "NUF"];

  async function save(event) {
    event.preventDefault();
    if (lagrer) return;
    lagrer = true;
    melding = null;
    try {
      await send("PUT", "/companies/" + companyId + "/settings", {
        address,
        bank_account: bankAccount,
        orgform,
        email,
        kid_avtale: kidAvtale,
        reise_satser: reiseSatser,
      });
      melding = { ok: true, tekst: "Lagret." };
      toast("Firmaopplysninger lagret", true);
      onDone();
    } catch (error) {
      melding = { ok: false, tekst: error.message };
    } finally {
      lagrer = false;
    }
  }

  async function lagreRegistrering(event) {
    event.preventDefault();
    if (lagrerReg) return;
    lagrerReg = true;
    meldingReg = null;
    try {
      const svar = await post("/companies/" + companyId + "/settings/registrering", {
        mva_registrert: mva,
        foretaksregistrert: freg,
        valid_from: fraDato || null,
      });
      meldingReg = { ok: true, tekst: "Lagret fra " + formatDate(svar.valid_from) + "." };
      fraDato = "";
      onDone();
    } catch (error) {
      meldingReg = { ok: false, tekst: error.message };
    } finally {
      lagrerReg = false;
    }
  }
</script>

<Card title="Firmaopplysninger på salgsdokumentene">
  <p class="text-sm opacity-70 mb-2">
    Adresse, kontonummer og selskapsform trykkes på faktura-PDF-en.
  </p>
  {#if !address.trim()}
    <div class="alert alert-warning alert-soft mb-2 text-sm">
      Adressen er påkrevd på salgsdokumentet (bokføringsforskriften §5-1-2). Fakturaer kan ikke
      utstedes før den er fylt ut.
    </div>
  {/if}
  <form class="grid gap-1 max-w-md" onsubmit={save}>
    <fieldset class="fieldset py-0" disabled={!kanEndre}>
      <legend class="fieldset-legend">Adresse</legend>
      <input class="input input-sm w-full" autocomplete="street-address" bind:value={address} />
    </fieldset>
    <fieldset class="fieldset py-0" disabled={!kanEndre}>
      <legend class="fieldset-legend">E-post (svaradresse på utsendelser)</legend>
      <input class="input input-sm w-full" type="email" autocomplete="email" bind:value={email} />
    </fieldset>
    <div class="flex flex-wrap gap-x-2">
      <fieldset class="fieldset py-0 flex-1 min-w-44" disabled={!kanEndre}>
        <legend class="fieldset-legend">Kontonummer</legend>
        <input
          class="input input-sm w-full"
          inputmode="numeric"
          placeholder="1234.56.78903"
          bind:value={bankAccount}
        />
      </fieldset>
      <fieldset class="fieldset py-0" disabled={!kanEndre}>
        <legend class="fieldset-legend">Selskapsform</legend>
        <select class="select select-sm" bind:value={orgform}>
          {#each ORGFORMER as f (f)}
            <option value={f}>{f || "Velg"}</option>
          {/each}
        </select>
      </fieldset>
    </div>
    <label class="flex items-start gap-2 text-sm mt-2 cursor-pointer">
      <input
        type="checkbox"
        class="checkbox checkbox-sm mt-0.5"
        disabled={!kanEndre}
        bind:checked={kidAvtale}
      />
      <span>
        Vi har KID-/OCR-avtale med banken
        <span class="block opacity-60 text-xs">Uten avtale trykkes ingen KID på faktura og purring.</span>
      </span>
    </label>
    <fieldset class="fieldset py-0" disabled={!kanEndre}>
      <legend class="fieldset-legend">Diett på reiser betales etter</legend>
      <select class="select select-sm w-full" bind:value={reiseSatser}>
        <option value="trekkfri">Skattefrie satser, uten skattetrekk</option>
        <option value="statens">Statens satser (det over skattefri sats er trekkpliktig lønn)</option>
      </select>
    </fieldset>
    {#if kanEndre}
      <div class="flex flex-wrap items-center gap-3 mt-2">
        <button class="btn btn-sm btn-primary" disabled={lagrer}>
          {#if lagrer}<span class="loading loading-spinner loading-xs"></span>{/if}
          Lagre
        </button>
        <span role="status" class="text-sm {melding?.ok ? 'text-success' : 'text-error'}">
          {melding?.tekst ?? ""}
        </span>
      </div>
    {/if}
  </form>
</Card>

<Card title="Registrering i offentlige registre">
  <p class="text-sm opacity-70 mb-2">
    Avgjør påtegningene «MVA» og «Foretaksregisteret» på salgsdokumentet (§5-1-2). Statusen
    gjelder fra en dato: dokumenter datert før den beholder sin egen. En registrering kan ikke
    endres; en ny status får en senere fra-dato.
  </p>
  {#if ubekreftet}
    <div class="alert alert-warning alert-soft mb-2 text-sm">
      Dagens verdier er utledet av tidligere logikk, ikke hentet fra Enhetsregisteret. Bekreft
      dem, for feil påtegning gjør salgsdokumentet mangelfullt.
    </div>
  {/if}
  {#if kanEndre}
    <form class="grid gap-1 max-w-md" onsubmit={lagreRegistrering}>
      <label class="flex items-center gap-2 text-sm cursor-pointer">
        <input type="checkbox" class="checkbox checkbox-sm" bind:checked={mva} />
        Registrert i Merverdiavgiftsregisteret
      </label>
      <label class="flex items-center gap-2 text-sm cursor-pointer">
        <input type="checkbox" class="checkbox checkbox-sm" bind:checked={freg} />
        Registrert i Foretaksregisteret
      </label>
      <fieldset class="fieldset py-0">
        <legend class="fieldset-legend">Gjelder fra (tomt betyr i dag)</legend>
        <input type="date" class="input input-sm w-44" bind:value={fraDato} />
      </fieldset>
      <div class="flex flex-wrap items-center gap-3 mt-2">
        <button class="btn btn-sm" disabled={lagrerReg}>
          {#if lagrerReg}<span class="loading loading-spinner loading-xs"></span>{/if}
          Lagre registrering
        </button>
        <span role="status" class="text-sm {meldingReg?.ok ? 'text-success' : 'text-error'}">
          {meldingReg?.tekst ?? ""}
        </span>
      </div>
    </form>
  {/if}
  {#if historikk.length}
    <h3 class="font-semibold text-sm mt-4 mb-1">Historikk</h3>
    <ul class="divide-y divide-base-200 text-sm">
      {#each historikk as h (h.valid_from)}
        <li class="py-1.5 flex flex-wrap gap-x-3 gap-y-0.5">
          <span class="font-medium tabular-nums">Fra {formatDate(h.valid_from)}</span>
          <span>Mva: {h.mva_registrert ? "ja" : "nei"}</span>
          <span>Foretaksregisteret: {h.foretaksregistrert ? "ja" : "nei"}</span>
          <span class="opacity-60 ml-auto">{KILDE[h.kilde] ?? h.kilde}</span>
        </li>
      {/each}
    </ul>
  {/if}
</Card>
