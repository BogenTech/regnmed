<script>
  // Byråets egne folk (#78). Samme disiplin som selskapets Tilgang-kort:
  // invitasjonen går til en E-POSTADRESSE og blir til medlemskap ved
  // innlogging. Forskjellen er rekkevidden — et byråmedlem når ALLE
  // byråets klienter gjennom oppdragene, så dette er porteføljetilgang.
  //
  // Members are rows so the actions fit a phone; one request at a time.
  import { post, send } from "../../lib/api.js";
  import { formatTime } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { loadMe, me } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";
  import { rolleNavn, rolleendringAdvarsel } from "../brukere/brukere.js";

  let { firmId, medlemmer, invitasjoner, onDone } = $props();

  const ROLLER = [
    { navn: "ansatt", beskrivelse: "Arbeider med alle byråets klienter. Avgjør ikke oppdrag og styrer ikke medlemmer." },
    { navn: "admin", beskrivelse: "Som ansatt, og godtar eller avslår oppdrag og styrer byråets medlemmer." },
  ];
  const beskrivelse = (navn) => ROLLER.find((r) => r.navn === navn)?.beskrivelse || "";

  let epost = $state("");
  let rolle = $state("ansatt");
  let opptatt = $state(false);

  async function utfor(arbeid, melding, egen = false) {
    opptatt = true;
    try {
      await arbeid();
      toast(melding, true);
      if (egen) await loadMe(true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      opptatt = false;
      onDone();
    }
  }

  async function inviter(event) {
    event.preventDefault();
    if (opptatt) return;
    opptatt = true;
    try {
      const svar = await post("/firms/" + firmId + "/invitations", { epost: epost.trim(), rolle });
      toast(
        svar.epost_sendt
          ? "Invitasjonen er sendt til " + epost.trim()
          : "Invitasjonen er registrert, men e-posten gikk ikke ut. Si fra til vedkommende selv.",
        svar.epost_sendt,
      );
      epost = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      opptatt = false;
    }
  }

  async function endreRolle(m, select) {
    const ny = select.value;
    const egen = m.person_id === me.personId;
    const advarsel = rolleendringAdvarsel(m, ny, egen);
    if (advarsel && !(await bekreft(advarsel, { tittel: "Endre rolle", ok: "Endre", farlig: true }))) {
      select.value = m.rolle;
      return;
    }
    await utfor(
      () => send("PUT", "/firms/" + firmId + "/access/" + m.person_id, { rolle: ny }),
      "Rollen er endret",
      egen,
    );
  }

  async function fjern(m) {
    const egen = m.person_id === me.personId;
    const melding = egen
      ? "Fjerne din egen tilgang til byrået? Du mister tilgangen til alle byråets klienter med én gang."
      : "Fjerne " + m.navn + " fra byrået? Tilgangen til alle byråets klienter slutter med én gang.";
    if (!(await bekreft(melding, { tittel: "Fjern tilgang", ok: "Fjern", farlig: true }))) return;
    await utfor(
      () => send("DELETE", "/firms/" + firmId + "/access/" + m.person_id),
      "Tilgangen er fjernet",
      egen,
    );
  }

  const gjenopprett = (m) =>
    utfor(
      () => post("/firms/" + firmId + "/access/" + m.person_id + "/restore", {}),
      "Tilgangen er gitt igjen",
    );

  const sendPaaNytt = (i) =>
    utfor(
      () => post("/firms/" + firmId + "/invitations/" + i.id + "/resend", {}),
      "Invitasjonen er sendt på nytt",
    );

  async function tilbakekall(i) {
    if (
      !(await bekreft("Tilbakekalle invitasjonen til " + i.epost + "? Den kan ikke lenger brukes.", {
        tittel: "Tilbakekall invitasjon",
        ok: "Tilbakekall",
        farlig: true,
      }))
    )
      return;
    await utfor(
      () => send("DELETE", "/firms/" + firmId + "/invitations/" + i.id),
      "Invitasjonen er tilbakekalt",
    );
  }
</script>

<Card title="Medlemmer">
  <p class="text-sm opacity-70 mb-3">
    Et aktivt medlem har tilgang til alle byråets klienter gjennom oppdragene. Inviter med
    e-postadressen personen logger inn med; tilgangen blir til ved neste innlogging.
  </p>
  <ul class="divide-y divide-base-200 mb-4" aria-label="Medlemmer">
    {#each medlemmer as m (m.person_id)}
      <li class="py-2 flex flex-wrap items-center gap-x-3 gap-y-1 {m.aktiv ? '' : 'opacity-60'}">
        <div class="min-w-0 flex-1 basis-48">
          <div class="font-medium">
            {m.navn}
            {#if m.person_id === me.personId}<span class="badge badge-ghost badge-sm ml-1">deg</span>{/if}
            {#if !m.aktiv}<span class="badge badge-ghost badge-sm ml-1">uten tilgang</span>{/if}
          </div>
          {#if m.epost}<div class="text-sm opacity-70 break-all">{m.epost}</div>{/if}
          <div class="text-xs opacity-60">{rolleNavn(m.rolle)}: {beskrivelse(m.rolle)}</div>
        </div>
        <div class="flex flex-wrap gap-2 items-center ml-auto">
          {#if !m.aktiv}
            <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={() => gjenopprett(m)}>
              Gi tilgang igjen
            </button>
          {:else}
            <select
              class="select select-sm w-40"
              aria-label={"Rolle for " + m.navn}
              value={m.rolle}
              disabled={opptatt}
              onchange={(e) => endreRolle(m, e.currentTarget)}
            >
              {#each ROLLER as r (r.navn)}
                <option value={r.navn}>{rolleNavn(r.navn)}</option>
              {/each}
            </select>
            <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={() => fjern(m)}>Fjern</button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  <form class="flex flex-wrap gap-2 items-end mb-1" onsubmit={inviter}>
    <label class="fieldset py-0 grow basis-56">
      <span class="fieldset-legend">E-postadresse</span>
      <input
        class="input input-sm w-full"
        type="email"
        autocomplete="off"
        required
        placeholder="navn@byra.no"
        bind:value={epost}
      />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Rolle</span>
      <select class="select select-sm w-40" bind:value={rolle}>
        {#each ROLLER as r (r.navn)}
          <option value={r.navn}>{rolleNavn(r.navn)}</option>
        {/each}
      </select>
    </label>
    <button class="btn btn-sm btn-primary" disabled={opptatt}>
      {#if opptatt}<span class="loading loading-spinner loading-xs"></span>{/if}
      Inviter
    </button>
  </form>
  <p class="text-xs opacity-60 mb-3">{beskrivelse(rolle)}</p>

  {#if invitasjoner.length}
    <h3 class="font-semibold text-sm mt-3 mb-1">Venter på innlogging</h3>
    <ul class="divide-y divide-base-200" aria-label="Åpne invitasjoner">
      {#each invitasjoner as i (i.id)}
        <li class="py-2 flex flex-wrap items-center gap-x-3 gap-y-1">
          <div class="min-w-0 flex-1 basis-48">
            <div class="break-all">{i.epost}</div>
            <div class="text-xs opacity-60">
              {rolleNavn(i.rolle)}, invitert av {i.invitert_av}.
              {i.sist_sendt ? "Sendt " + formatTime(i.sist_sendt) + "." : "E-posten er ikke sendt."}
            </div>
          </div>
          <div class="flex gap-2 ml-auto">
            <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={() => sendPaaNytt(i)}>
              Send på nytt
            </button>
            <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={() => tilbakekall(i)}>
              Tilbakekall
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</Card>
