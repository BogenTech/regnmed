<script>
  // Hvem som kommer til i selskapet. Invitasjonen går til en
  // E-POSTADRESSE — personen finnes ikke hos oss før første innlogging.
  // Endringer her virker straks, og kan gjelde meg selv, så /me hentes
  // på nytt etterpå.
  //
  // Members are rows, not a table: the actions have to be reachable on a
  // phone. One request at a time — `opptatt` disables every action.
  import { post, send } from "../../lib/api.js";
  import { formatTime } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { loadMe, me } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";
  import { rolleNavn, rolleendringAdvarsel } from "../brukere/brukere.js";

  let { companyId, tilgang, invitasjoner, roller, onDone } = $props();

  // Rollene som kan tildeles: de innebygde (unntatt 'revisor', som
  // følger av et oppdrag) pluss selskapets egne, aktive.
  let rollevalg = $derived([
    ...(roller?.innebygde || [])
      .filter((r) => r.navn !== "revisor")
      .map((r) => ({ navn: r.navn, beskrivelse: r.beskrivelse })),
    ...(roller?.egne || [])
      .filter((r) => r.aktiv)
      .map((r) => ({ navn: r.navn, beskrivelse: "Egendefinert rolle." })),
  ]);
  const beskrivelse = (navn) => rollevalg.find((r) => r.navn === navn)?.beskrivelse || "";

  let epost = $state("");
  let rolle = $state("ansatt");
  let opptatt = $state(false);

  async function utfor(arbeid, melding, egen = false) {
    opptatt = true;
    try {
      await arbeid();
      if (melding) toast(melding, true);
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
      const svar = await post("/companies/" + companyId + "/invitations", {
        epost: epost.trim(),
        rolle,
      });
      // Invitasjonen gjelder uansett — e-posten er varselet om den, ikke
      // selve tilgangen. Gikk den ikke ut, skal admin få vite det og
      // kunne si fra i en annen kanal (#66).
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
      () => send("PUT", "/companies/" + companyId + "/access/" + m.person_id, { rolle: ny }),
      "Rollen er endret",
      egen,
    );
  }

  async function fjern(m) {
    const egen = m.person_id === me.personId;
    const melding = egen
      ? "Fjerne din egen tilgang? Du kommer ikke inn i selskapet igjen før en administrator gir deg tilgang."
      : "Fjerne tilgangen for " + m.navn + "? Den slutter å virke med én gang.";
    if (!(await bekreft(melding, { tittel: "Fjern tilgang", ok: "Fjern", farlig: true }))) return;
    await utfor(
      () => send("DELETE", "/companies/" + companyId + "/access/" + m.person_id),
      "Tilgangen er fjernet",
      egen,
    );
  }

  const gjenopprett = (m) =>
    utfor(
      () => post("/companies/" + companyId + "/access/" + m.person_id + "/restore", {}),
      "Tilgangen er gitt igjen",
    );

  const sendPaaNytt = (i) =>
    utfor(
      () => post("/companies/" + companyId + "/invitations/" + i.id + "/resend", {}),
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
      () => send("DELETE", "/companies/" + companyId + "/invitations/" + i.id),
      "Invitasjonen er tilbakekalt",
    );
  }
</script>

<Card title="Tilgang">
  <p class="text-sm opacity-70 mb-3">
    Inviter med e-postadressen personen bruker for å logge inn. Tilgangen blir til når
    vedkommende logger inn neste gang. E-posten er bare et varsel og har ingen lenke som kan
    misbrukes om den videresendes. Den som har tilgang gjennom et oppdrag, styres av oppdraget.
  </p>
  <ul class="divide-y divide-base-200 mb-4" aria-label="Medlemmer">
    {#each tilgang.medlemmer as m (m.person_id + m.via)}
      <li class="py-2 flex flex-wrap items-center gap-x-3 gap-y-1 {m.aktiv ? '' : 'opacity-60'}">
        <div class="min-w-0 flex-1 basis-48">
          <div class="font-medium">
            {m.navn}
            {#if m.person_id === me.personId}<span class="badge badge-ghost badge-sm ml-1">deg</span>{/if}
            {#if !m.aktiv}<span class="badge badge-ghost badge-sm ml-1">uten tilgang</span>{/if}
          </div>
          {#if m.epost}<div class="text-sm opacity-70 break-all">{m.epost}</div>{/if}
          <div class="text-xs opacity-60">
            {#if m.kan_endres}
              {rolleNavn(m.rolle)}{beskrivelse(m.rolle) ? ": " + beskrivelse(m.rolle) : ""}
            {:else}
              {rolleNavn(m.rolle)} gjennom oppdrag for {m.via}. Endres ved å endre oppdraget.
            {/if}
          </div>
        </div>
        {#if m.kan_endres}
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
                {#if !rollevalg.some((r) => r.navn === m.rolle)}
                  <!-- A deactivated custom role is still this member's role. -->
                  <option value={m.rolle} disabled>{rolleNavn(m.rolle)} (deaktivert)</option>
                {/if}
                {#each rollevalg as r (r.navn)}
                  <option value={r.navn}>{rolleNavn(r.navn)}</option>
                {/each}
              </select>
              <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={() => fjern(m)}>
                Fjern
              </button>
            {/if}
          </div>
        {/if}
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
        placeholder="navn@firma.no"
        bind:value={epost}
      />
    </label>
    <label class="fieldset py-0">
      <span class="fieldset-legend">Rolle</span>
      <select class="select select-sm w-40" bind:value={rolle}>
        {#each rollevalg as r (r.navn)}
          <option value={r.navn}>{rolleNavn(r.navn)}</option>
        {/each}
      </select>
    </label>
    <button class="btn btn-sm btn-primary" disabled={opptatt}>
      {#if opptatt}<span class="loading loading-spinner loading-xs"></span>{/if}
      Inviter
    </button>
  </form>
  {#if beskrivelse(rolle)}<p class="text-xs opacity-60 mb-3">{beskrivelse(rolle)}</p>{/if}

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
