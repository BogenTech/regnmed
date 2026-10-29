<script>
  // Samtalen med en agent (docs/agent.md §8.5).
  //
  // Dette er en VISNING av `agent_oppgave_hendelse`, ikke en ny
  // datamodell: tråden fantes fra #103, fordi `sporsmal` med vilje ikke
  // er terminal. Derfor finnes det heller ingen «slett melding» her —
  // sporet er innsettings-bart, og et svar kan ikke trekkes tilbake.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Agentansikt from "../../components/Agentansikt.svelte";

  let { companyId, oppgave, agent = null, onDone } = $props();

  let trad = $state(null);
  let svar = $state("");
  let sender = $state(false);

  async function last() {
    try {
      trad = await api("/companies/" + companyId + "/agent/oppgaver/" + oppgave.oppgave_id);
    } catch (error) {
      toast(error.message, false);
    }
  }

  $effect(() => {
    trad = null;
    svar = "";
    last();
  });

  // Et menneskes svar er `pagar` med notat: statusen sier at arbeidet
  // fortsetter, notatet er det som ble sagt. Det gjør også oppgaven
  // hentbar igjen for workeren, som hopper over `sporsmal`.
  async function send() {
    const tekst = svar.trim();
    if (!tekst) return;
    sender = true;
    try {
      await post("/companies/" + companyId + "/agent/oppgaver/" + oppgave.oppgave_id + "/svar", {
        status: "pagar",
        notat: tekst,
      });
      svar = "";
      await last();
      onDone?.();
    } catch (error) {
      toast(error.message, false);
    } finally {
      sender = false;
    }
  }

  function tid(t) {
    return (t || "").slice(0, 16).replace("T", " ");
  }

  // Hvem som sa det avgjøres av navnet i sporet, som er det eneste
  // sporet faktisk vet.
  const fraAgenten = (h) => !!oppgave.agent && h.av === oppgave.agent;

  // Boblene er håndrullet av klasser bundelen alt har, ikke daisyUIs
  // chat-komponent: den koster ~2,3 KB i dist for ren utsmykking, og
  // budsjettet (frugality.md) hadde 7 KB igjen. Samme avveining som
  // ikonene — vi skriver markupen, temaet eier fargene.
  const BOBLE = {
    sporsmal: "bg-warning text-warning-content",
    avvist: "bg-error text-error-content",
    menneske: "bg-primary text-primary-content",
    agent: "bg-base-200",
  };
  function boble(h) {
    if (h.status === "sporsmal") return BOBLE.sporsmal;
    if (h.status === "avvist") return BOBLE.avvist;
    return fraAgenten(h) ? BOBLE.agent : BOBLE.menneske;
  }
</script>

<div class="border-base-200 rounded-box border p-3">
  <div class="flex items-center gap-2 mb-2">
    {#if agent}
      <Agentansikt navn={agent.navn} farge={agent.avatar_farge} ikon={agent.avatar_ikon} storrelse="w-8" />
    {/if}
    <div class="text-sm">
      <strong>{oppgave.agent || "enhver agent"}</strong>
      <div class="text-xs opacity-60">{oppgave.melding}</div>
    </div>
  </div>

  {#if !trad}
    <span class="loading loading-spinner loading-sm"></span>
  {:else}
    <div class="max-h-80 overflow-y-auto pr-1 flex flex-col gap-2">
      <div class="flex flex-col items-end">
        <span class="text-xs opacity-60">
          {oppgave.opprettet_av} · {tid(oppgave.created_at)}
        </span>
        <div class="rounded-box max-w-[85%] px-3 py-2 text-sm {BOBLE.menneske}">
          {oppgave.melding}
        </div>
      </div>
      {#each trad.historikk as h, i (i)}
        {#if h.notat}
          <div class="flex flex-col {fraAgenten(h) ? 'items-start' : 'items-end'}">
            <span class="text-xs opacity-60">{h.av} · {tid(h.tidspunkt)}</span>
            <div class="rounded-box max-w-[85%] px-3 py-2 text-sm {boble(h)}">{h.notat}</div>
          </div>
        {/if}
      {/each}
    </div>

    {#if oppgave.avsluttet}
      <p class="text-xs opacity-60 mt-2">
        Oppgaven er avsluttet. En ny bestilling er en ny oppgave — sporet står som det er.
      </p>
    {:else}
      <div class="flex gap-2 items-end mt-3">
        <textarea
          class="textarea textarea-sm flex-1"
          rows="2"
          placeholder={oppgave.status === "sporsmal"
            ? "Agenten venter på svar fra deg"
            : "Skriv til agenten"}
          bind:value={svar}
        ></textarea>
        <button class="btn btn-sm" onclick={send} disabled={sender || !svar.trim()}>Send</button>
      </div>
    {/if}
  {/if}
</div>
