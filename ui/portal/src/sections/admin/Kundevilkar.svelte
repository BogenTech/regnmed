<script>
  // Kundevilkårene (#185, docs/kundevilkar.md §1.4): innsending til
  // myndighetene på selskapets vegne krever at en administrator har godtatt
  // akkurat denne teksten. Versjonen er tekstens hash — serveren nekter en
  // versjon som ikke er den gjeldende.
  import { api, post } from "../../lib/api.js";
  import { formatDate, formatTime } from "../../lib/format.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";

  let { companyId } = $props();

  let status = $state(null); // null = loading, { error } = failed
  let vilkar = $state(null); // { versjon, tekst } når åpnet
  let register = $state(null); // §7.1: kall til myndighetene, hentet ved behov
  let godtar = $state(false);

  let erAdmin = $derived(harRett(companyId, "SELSKAP_ADMIN"));

  const SKJEMA = {
    "a-melding": "A-melding",
    aksjonaerregisteroppgave: "Aksjonærregisteroppgave",
    "altinn-hendelser": "Varsling fra Altinn",
    skattekort: "Skattekort",
  };
  const HENDELSE = {
    receipt: "kvittering",
    failed: "feilet",
    feedback: "svar",
    outcome: "svar",
    answer: "svar",
  };

  async function hent() {
    const id = companyId;
    try {
      const svar = await api("/companies/" + id + "/terms");
      if (id === companyId) status = svar;
    } catch (error) {
      if (id === companyId) status = { error: error.message };
    }
  }

  $effect(() => {
    void companyId;
    status = null;
    hent();
  });

  async function les() {
    try {
      vilkar = await api("/terms");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function visRegister() {
    try {
      register = await api("/companies/" + companyId + "/authority-calls");
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function godta() {
    if (
      godtar ||
      !(await bekreft(
        "Godta kundevilkårene på vegne av selskapet? Du bekrefter at du har rett til å binde selskapet.",
        { tittel: "Godta kundevilkår", ok: "Godta" },
      ))
    ) {
      return;
    }
    godtar = true;
    try {
      status = await post("/companies/" + companyId + "/terms/accept", {
        versjon: vilkar.versjon,
      });
      toast("Kundevilkårene er godtatt", true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      godtar = false;
    }
  }
</script>

<Card title="Kundevilkår">
  {#if !status}
    <span class="loading loading-spinner loading-sm" aria-label="Laster kundevilkårene"></span>
  {:else if status.error}
    <LoadError error={status.error} retry={hent} />
  {:else}
    {#if status.godtatt}
      <p class="text-sm mb-2">
        Godtatt av <b>{status.godtatt_av}</b>
        {formatTime(status.godtatt_tidspunkt)}.
      </p>
    {:else}
      <p class="text-sm mb-2">
        Før regnmed kan sende a-melding, aksjonærregisteroppgave og andre oppgaver til
        myndighetene på selskapets vegne, må en administrator godta kundevilkårene.
      </p>
    {/if}
    {#if vilkar}
      <!-- Focusable so the scrolling text can be read with the keyboard. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre
        class="whitespace-pre-wrap text-xs border border-base-300 rounded p-3 max-h-96 overflow-y-auto mb-2"
        tabindex="0"
        aria-label="Kundevilkårene">{vilkar.tekst}</pre>
      <p class="text-xs opacity-60 mb-2">Versjon {vilkar.versjon.slice(0, 12)}</p>
      {#if !status.godtatt}
        {#if erAdmin}
          <button class="btn btn-sm btn-primary" disabled={godtar} onclick={godta}>
            {#if godtar}<span class="loading loading-spinner loading-xs"></span>{/if}
            Godta på vegne av selskapet
          </button>
        {:else}
          <p class="text-sm opacity-70">Bare en administrator kan godta vilkårene.</p>
        {/if}
      {/if}
    {:else}
      <button class="btn btn-sm btn-outline" onclick={les}>Les kundevilkårene</button>
    {/if}
    {#if status.historikk.some((h) => h.versjon !== status.versjon)}
      <p class="text-xs opacity-60 mt-3 mb-1">Tidligere godtatte versjoner</p>
      <ul class="text-xs">
        {#each status.historikk.filter((h) => h.versjon !== status.versjon) as h (h.versjon)}
          <li>
            <a class="link" href={"/vilkar/" + h.versjon} target="_blank">{h.versjon.slice(0, 12)}</a>,
            {h.godtatt_av}, {formatDate(h.godtatt_tidspunkt)}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="mt-3">
      {#if !register}
        <button class="btn btn-xs btn-ghost" onclick={visRegister}>
          Vis innsendinger til myndighetene
        </button>
      {:else if register.length === 0}
        <p class="text-xs opacity-60">Ingen innsendinger til myndighetene ennå.</p>
      {:else}
        <h3 class="font-semibold text-sm mb-1">Innsendinger til myndighetene</h3>
        <ul class="divide-y divide-base-200 text-sm">
          {#each register as r, i (i)}
            <li class="py-1.5 flex flex-wrap gap-x-3 gap-y-0.5">
              <span class="font-medium">{SKJEMA[r.skjema] ?? r.skjema}</span>
              <span>{r.gjelder}</span>
              <span class="ml-auto opacity-70">
                {r.hendelser.map((h) => HENDELSE[h.hendelse] ?? h.hendelse).join(", ") || "venter"}
              </span>
              <span class="basis-full text-xs opacity-60 tabular-nums">
                Bestilt {formatTime(r.bestilt_tidspunkt)} av {r.bestilt_av}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</Card>
