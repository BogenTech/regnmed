<script>
  // Hvem som kommer til i selskapet. Invitasjonen går til en
  // E-POSTADRESSE — personen finnes ikke hos oss før første innlogging.
  // Endringer her virker straks, og kan gjelde meg selv, så /me hentes
  // på nytt etterpå.
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { loadMe } from "../../lib/me.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, tilgang, invitasjoner, roller, onDone } = $props();

  // Rollene som kan tildeles: de innebygde (unntatt 'revisor', som
  // følger av et oppdrag) pluss selskapets egne, aktive.
  let rollevalg = $derived(
    ["ansatt", "les", "bokforing", "admin"].concat(
      roller ? roller.egne.filter((e) => e.aktiv).map((e) => e.navn) : [],
    ),
  );

  let epost = $state("");
  let rolle = $state("ansatt");

  async function inviter() {
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
          ? "Invitasjonen er sendt"
          : "Invitasjonen er registrert, men e-posten gikk ikke ut — si fra til vedkommende selv",
        svar.epost_sendt,
      );
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endreRolle(m, ny) {
    try {
      await send("PUT", "/companies/" + companyId + "/access/" + m.person_id, { rolle: ny });
      toast("Rollen er endret", true);
      await loadMe(true);
      onDone();
    } catch (error) {
      toast(error.message, false);
      onDone(); // sett listen tilbake til det serveren faktisk mener
    }
  }

  async function fjern(m) {
    if (!confirm("Fjerne tilgangen? Den slutter å virke med én gang.")) return;
    try {
      await send("DELETE", "/companies/" + companyId + "/access/" + m.person_id);
      toast("Tilgangen er fjernet", true);
      await loadMe(true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function gjenopprett(m) {
    try {
      await post("/companies/" + companyId + "/access/" + m.person_id + "/restore", {});
      toast("Tilgangen er gjenopprettet", true);
      await loadMe(true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function sendPaaNytt(i) {
    try {
      await post("/companies/" + companyId + "/invitations/" + i.id + "/resend", {});
      toast("Invitasjonen er sendt på nytt", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function tilbakekall(i) {
    try {
      await send("DELETE", "/companies/" + companyId + "/invitations/" + i.id);
      toast("Invitasjonen er tilbakekalt", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Tilgang">
  <p class="text-sm opacity-70 mb-2">
    Hvem som kommer til i dette selskapet. Inviter med e-postadressen personen bruker for å logge
    inn — det går en e-post til adressen, og tilgangen blir til når hun logger inn neste gang.
    E-posten inneholder ingen hemmelig lenke: den kan ikke videresendes til noen som helst nytte.
    Den som har fått tilgang gjennom et oppdrag styres av oppdraget, ikke herfra.
  </p>
  <table class="table table-sm mb-3">
    <thead>
      <tr><th>Navn</th><th>E-post</th><th>Rolle</th><th>Via</th><th></th></tr>
    </thead>
    <tbody>
      {#each tilgang.medlemmer as m (m.person_id)}
        <tr class={m.aktiv ? "" : "opacity-50"}>
          <td>{m.navn}</td>
          <td>{m.epost || ""}</td>
          <td><span class="badge badge-ghost badge-sm">{m.rolle}</span></td>
          <td><span class="badge badge-ghost badge-sm">{m.via}</span></td>
          <td>
            {#if !m.kan_endres}
              <!-- Tilgang gjennom et oppdrag styres av engasjementet. Si det,
                   ikke tilby en knapp som ikke virker. -->
              <span class="opacity-60 text-xs">via oppdrag</span>
            {:else if !m.aktiv}
              <button class="btn btn-xs btn-outline" onclick={() => gjenopprett(m)}>
                Gi tilgang igjen
              </button>
            {:else}
              <select
                class="select select-xs w-28 mr-1"
                value={m.rolle}
                onchange={(e) => endreRolle(m, e.currentTarget.value)}
              >
                {#each rollevalg as r}
                  <option value={r}>{r}</option>
                {/each}
              </select>
              <button class="btn btn-xs btn-outline" onclick={() => fjern(m)}>Fjern</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  <div class="flex gap-2 items-center flex-wrap mb-3">
    <input
      class="input input-sm w-64"
      placeholder="e-postadresse"
      bind:value={epost}
    />
    <select class="select select-sm" bind:value={rolle}>
      {#each rollevalg as r}
        <option value={r}>{r}</option>
      {/each}
    </select>
    <button class="btn btn-sm" onclick={inviter}>Inviter</button>
  </div>
  {#if invitasjoner.length}
    <h3 class="font-semibold text-sm mb-1">Venter på innlogging</h3>
    <table class="table table-sm">
      <thead>
        <tr><th>E-post</th><th>Rolle</th><th>Invitert av</th><th>Sendt</th><th></th></tr>
      </thead>
      <tbody>
        {#each invitasjoner as i (i.id)}
          <tr>
            <td>{i.epost}</td>
            <td>{i.rolle}</td>
            <td>{i.invitert_av}</td>
            <td class="text-xs opacity-70">
              {i.sist_sendt ? new Date(i.sist_sendt).toLocaleString("no") : "ikke sendt"}
            </td>
            <td class="whitespace-nowrap">
              <button class="btn btn-xs btn-outline" onclick={() => sendPaaNytt(i)}>
                Send på nytt
              </button>
              <button class="btn btn-xs btn-outline" onclick={() => tilbakekall(i)}>
                Tilbakekall
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</Card>
