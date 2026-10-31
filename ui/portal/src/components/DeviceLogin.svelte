<script>
  // Logg inn på en skjerm som ikke kan ta et langt passord: en bilskjerm,
  // en lånt PC. Koden vises her, telefonen godkjenner den.
  import { startDevice, pollDevice } from "../lib/auth.svelte.js";

  let { onavbryt } = $props();

  let data = $state(null);
  let feil = $state("");
  let gjenstar = $state(0);

  async function start() {
    feil = "";
    data = null;
    try {
      data = await startDevice();
      gjenstar = data.expires_in ?? 300;
    } catch (e) {
      feil = e.message;
    }
  }

  start();

  // Én klokke for både nedtelling og spørring, så de ikke kan komme i
  // utakt. Spørringen følger intervallet utstederen oppgir.
  $effect(() => {
    if (!data) return;
    const intervall = data.interval ?? 5;
    let tikk = 0;
    const klokke = setInterval(async () => {
      gjenstar -= 1;
      if (gjenstar <= 0) {
        clearInterval(klokke);
        feil = "Koden gikk ut. Hent en ny.";
        data = null;
        return;
      }
      if (++tikk % intervall !== 0) return;
      const svar = await pollDevice();
      if (svar === null) {
        clearInterval(klokke);
        feil = "Innloggingen ble avbrutt. Hent en ny kode.";
        data = null;
      }
      // true trenger ingen opprydding: appen bytter visning av seg selv.
    }, 1000);
    return () => clearInterval(klokke);
  });

  const minutter = $derived(
    `${Math.floor(gjenstar / 60)}:${String(gjenstar % 60).padStart(2, "0")}`,
  );
</script>

{#if feil}
  <div class="alert alert-warning text-sm">{feil}</div>
  <button class="btn btn-primary w-full mt-3" onclick={start}>Hent ny kode</button>
{:else if !data}
  <span class="loading loading-spinner loading-lg my-6"></span>
{:else}
  <p class="text-sm opacity-70 text-center">
    Skann koden med telefonen din, eller gå til
    <span class="font-mono">{data.verification_uri}</span> og skriv inn koden.
  </p>

  {#if data.qr}
    <!-- SVG-en kommer fra vårt eget API, ikke fra siden. -->
    <div class="w-48 h-48 my-3">{@html data.qr}</div>
  {/if}

  <div class="font-mono text-2xl tracking-widest">{data.user_code}</div>
  <p class="text-xs opacity-60 mt-1">Varer i {minutter}</p>
  <p class="text-xs opacity-60 mt-3 text-center">
    Denne skjermen blir logget ut etter en time, og lagrer ingen innlogging.
  </p>
{/if}

<button class="btn btn-ghost btn-sm w-full mt-2" onclick={onavbryt}>Tilbake</button>
