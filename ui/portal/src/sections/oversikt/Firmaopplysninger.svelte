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
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, settings, onDone = () => {} } = $props();

  let address = $state(untrack(() => settings.address || ""));
  let email = $state(untrack(() => settings.email || ""));
  let bankAccount = $state(untrack(() => settings.bank_account || ""));
  let orgform = $state(untrack(() => settings.orgform || ""));

  let mva = $state(untrack(() => !!settings.mva_registrert));
  let freg = $state(untrack(() => !!settings.foretaksregistrert));
  let fraDato = $state("");
  const historikk = settings.registrering_historikk || [];
  // En 'migrert' rad er den gamle utledningen, ikke en observasjon —
  // den skal bekreftes mot Enhetsregisteret.
  let ubekreftet = $derived(historikk.length > 0 && historikk[0].kilde === "migrert");

  const ORGFORMER = ["", "AS", "ASA", "ENK", "ANS", "DA", "NUF"];

  async function save() {
    try {
      await send("PUT", "/companies/" + companyId + "/settings", {
        address,
        bank_account: bankAccount,
        orgform,
        email,
      });
      toast("Firmaopplysninger lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function lagreRegistrering() {
    try {
      const svar = await post("/companies/" + companyId + "/settings/registrering", {
        mva_registrert: mva,
        foretaksregistrert: freg,
        valid_from: fraDato || null,
      });
      toast("Registreringsstatus lagret fra " + svar.valid_from, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Firmaopplysninger — på salgsdokumentene">
  <p class="text-sm opacity-70 mb-2">
    Adresse, kontonummer og selskapsform trykkes på faktura-PDF-en.
  </p>
  {#if !address.trim()}
    <div class="alert alert-warning alert-soft mb-2 text-sm">
      Adressen er påkrevd på salgsdokumentet (bokføringsforskriften §5-1-2) — fakturaer kan
      ikke utstedes før den er fylt ut.
    </div>
  {/if}
  <div class="grid gap-2 max-w-md">
    <input class="input input-sm" placeholder="Adresse" bind:value={address} />
    <input
      class="input input-sm"
      placeholder="E-post (svaradresse på utsendelser)"
      bind:value={email}
    />
    <div class="flex gap-2">
      <input class="input input-sm flex-1" placeholder="Kontonummer" bind:value={bankAccount} />
      <select class="select select-sm" bind:value={orgform}>
        {#each ORGFORMER as f}
          <option value={f}>{f || "(selskapsform)"}</option>
        {/each}
      </select>
    </div>
    <button class="btn btn-sm" onclick={save}>Lagre</button>
  </div>
</Card>

<Card title="Registrering i offentlige registre">
  <p class="text-sm opacity-70 mb-2">
    Avgjør påtegningene «MVA» og «Foretaksregisteret» på salgsdokumentet (§5-1-2). Statusen
    gjelder fra en dato: dokumenter datert før den beholder sin egen.
  </p>
  {#if ubekreftet}
    <div class="alert alert-warning alert-soft mb-2 text-sm">
      Dagens verdier er utledet av tidligere logikk, ikke hentet fra Enhetsregisteret. Bekreft
      dem — feil påtegning gjør salgsdokumentet mangelfullt.
    </div>
  {/if}
  <div class="grid gap-2 max-w-md">
    <label class="label cursor-pointer justify-start gap-2">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={mva} />
      <span class="label-text">Registrert i Merverdiavgiftsregisteret</span>
    </label>
    <label class="label cursor-pointer justify-start gap-2">
      <input type="checkbox" class="checkbox checkbox-sm" bind:checked={freg} />
      <span class="label-text">Registrert i Foretaksregisteret</span>
    </label>
    <label class="fieldset">
      <span class="fieldset-legend">Gjelder fra (tomt = i dag)</span>
      <input type="date" class="input input-sm" bind:value={fraDato} />
    </label>
    <button class="btn btn-sm" onclick={lagreRegistrering}>Lagre registrering</button>
  </div>
  {#if historikk.length}
    <table class="table table-sm mt-3">
      <thead>
        <tr><th>Fra</th><th>Mva</th><th>Foretaksreg.</th><th>Kilde</th></tr>
      </thead>
      <tbody>
        {#each historikk as h (h.valid_from)}
          <tr>
            <td>{h.valid_from}</td>
            <td>{h.mva_registrert ? "ja" : "nei"}</td>
            <td>{h.foretaksregistrert ? "ja" : "nei"}</td>
            <td>{h.kilde}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
