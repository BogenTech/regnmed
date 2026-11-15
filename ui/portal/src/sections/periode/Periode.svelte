<script>
  // Periodelåsing (#4/M1): låsen er innsettings-bar historikk, aldri en
  // redigering — hver lås er en ny rad. Gjenåpning er en admin-handling
  // og logges; portalen viser bare sporet, serveren håndhever.
  //
  // The next step is offered as one button (lock through the end of last
  // month); any other date is a labelled field. Moving the lock back is a
  // reopening and is asked about first.
  import { api, send } from "../../lib/api.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { formatDate, formatTime, today } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import Arsavslutning from "./Arsavslutning.svelte";
  import { laseForslag } from "./periode.js";

  let { companyId } = $props();

  let lock = $state(null); // null = loading, { error } = failed
  let annenDato = $state("");
  let lagrer = $state(false);

  let kanLase = $derived(harRett(companyId, "PERIODE_LAAS"));
  let erAdmin = $derived(harRett(companyId, "SELSKAP_ADMIN"));
  const iDag = today();
  let forslag = $derived(lock && !lock.error ? laseForslag(lock.locked_through, iDag) : null);

  async function load() {
    const id = companyId;
    try {
      const svar = await api("/companies/" + id + "/period-lock");
      if (id === companyId) lock = svar;
    } catch (error) {
      if (id === companyId) lock = { error: error.message };
    }
  }

  $effect(() => {
    void companyId;
    lock = null;
    load();
  });

  async function las(dato) {
    if (lagrer || !dato) return;
    const naa = lock.locked_through;
    const gjenapner = naa && dato < naa;
    const ok = gjenapner
      ? await bekreft(
          `Låsen flyttes tilbake fra ${formatDate(naa)} til ${formatDate(dato)}. Bilag datert etter ${formatDate(dato)} kan føres igjen, og gjenåpningen står i historikken for alltid.`,
          { tittel: "Gjenåpne perioden?", ok: "Gjenåpne", farlig: true },
        )
      : await bekreft(
          `Bilag datert til og med ${formatDate(dato)} kan ikke lenger føres eller endres. Rettelser føres i åpen periode. Bare en administrator kan åpne perioden igjen.`,
          { tittel: `Lås til og med ${formatDate(dato)}?`, ok: "Lås perioden" },
        );
    if (!ok) return;
    lagrer = true;
    try {
      await send("PUT", "/companies/" + companyId + "/period-lock", { locked_through: dato });
      toast(
        gjenapner
          ? "Perioden er gjenåpnet fra " + formatDate(dato)
          : "Låst til og med " + formatDate(dato),
        true,
      );
      annenDato = "";
      await load();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }
</script>

<Card title="Periodelåsing">
  {#if !lock}
    <span class="loading loading-spinner loading-sm" aria-label="Laster periodelåsen"></span>
  {:else if lock.error}
    <LoadError error={lock.error} retry={load} />
  {:else}
    <p class="mb-1">
      {#if lock.locked_through}
        Låst til og med <strong>{formatDate(lock.locked_through)}</strong>.
      {:else}
        Ingen periode er låst ennå.
      {/if}
    </p>
    <p class="text-sm opacity-70 mb-3">
      Bilag datert i en låst periode avvises; rettelser føres i åpen periode. Lås hver måned når
      den er ferdig ført, og senest når mva-terminen er levert.
    </p>

    {#if kanLase}
      {#if forslag}
        <div class="mb-3">
          <button class="btn btn-primary btn-sm" disabled={lagrer} onclick={() => las(forslag)}>
            {#if lagrer}<span class="loading loading-spinner loading-xs"></span>{/if}
            Lås til og med {formatDate(forslag)}
          </button>
        </div>
      {/if}
      <form
        class="flex flex-wrap items-end gap-2"
        onsubmit={(e) => {
          e.preventDefault();
          las(annenDato);
        }}
      >
        <fieldset class="fieldset py-0">
          <legend class="fieldset-legend">{forslag ? "Annen dato" : "Lås til og med"}</legend>
          <input type="date" class="input input-sm" required max={iDag} bind:value={annenDato} />
        </fieldset>
        <button class="btn btn-sm" disabled={lagrer || !annenDato}>
          {annenDato && lock.locked_through && annenDato < lock.locked_through
            ? "Gjenåpne"
            : "Lås"}
        </button>
      </form>
      {#if lock.locked_through}
        <p class="text-xs opacity-70 mt-1">
          {erAdmin
            ? "En dato før dagens lås gjenåpner perioden. Det logges."
            : "Bare en administrator kan gjenåpne en låst periode."}
        </p>
      {/if}
    {/if}

    <h3 class="font-semibold mt-4 mb-1">Historikk</h3>
    {#if lock.history.length}
      <ul class="divide-y divide-base-200 text-sm">
        {#each lock.history as h, i (i)}
          <li class="py-1.5 flex flex-wrap gap-x-3 gap-y-0.5 items-baseline">
            <span class="font-medium tabular-nums">{formatDate(h.locked_through)}</span>
            {#if h.gjenapnet}<span class="badge badge-warning badge-soft badge-sm">gjenåpnet</span>{/if}
            <span class="opacity-70 min-w-0 break-words">{h.set_by}</span>
            <span class="opacity-60 text-xs ml-auto tabular-nums">{formatTime(h.at)}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="text-sm opacity-70">Ingen periode er låst ennå.</p>
    {/if}
  {/if}
</Card>

<!--
  Årsavslutningen SETTER periodelåsen (docs/arsavslutning.md), så kortet
  over må lastes på nytt — ellers står det «ingen lås» rett over kortet
  som nettopp låste året, og operatøren får to motstridende svar på
  samme spørsmål.
-->
<Arsavslutning
  {companyId}
  laastTil={lock && !lock.error ? lock.locked_through : null}
  {erAdmin}
  onavsluttet={load}
/>
