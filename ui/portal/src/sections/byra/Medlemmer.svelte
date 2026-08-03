<script>
  // Byråets egne folk (#78). Samme disiplin som selskapets Tilgang-kort:
  // invitasjonen går til en E-POSTADRESSE og blir til medlemskap ved
  // innlogging. Forskjellen er rekkevidden — et byråmedlem når ALLE
  // byråets klienter gjennom oppdragene, så dette er porteføljetilgang.
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { firmId, medlemmer, invitasjoner, onDone } = $props();

  const ROLLER = ["ansatt", "admin"];

  let epost = $state("");
  let rolle = $state("ansatt");

  async function inviter() {
    try {
      const svar = await post("/firms/" + firmId + "/invitations", {
        epost: epost.trim(),
        rolle,
      });
      toast(
        svar.epost_sendt
          ? "Invitasjonen er sendt"
          : "Invitasjonen er registrert, men e-posten gikk ikke ut — si fra til vedkommende selv",
        svar.epost_sendt,
      );
      epost = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endreRolle(m, ny) {
    try {
      await send("PUT", "/firms/" + firmId + "/access/" + m.person_id, { rolle: ny });
      toast("Rollen er endret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
      onDone(); // sett listen tilbake til det serveren faktisk mener
    }
  }

  async function fjern(m) {
    if (!confirm("Fjerne tilgangen? Den slutter å virke med én gang — også hos klientene.")) return;
    try {
      await send("DELETE", "/firms/" + firmId + "/access/" + m.person_id);
      toast("Tilgangen er fjernet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function gjenopprett(m) {
    try {
      await post("/firms/" + firmId + "/access/" + m.person_id + "/restore", {});
      toast("Tilgangen er gjenopprettet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function sendPaaNytt(i) {
    try {
      await post("/firms/" + firmId + "/invitations/" + i.id + "/resend", {});
      toast("Invitasjonen er sendt på nytt", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function tilbakekall(i) {
    try {
      await send("DELETE", "/firms/" + firmId + "/invitations/" + i.id);
      toast("Invitasjonen er tilbakekalt", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Medlemmer">
  <p class="text-sm opacity-70 mb-2">
    Byråets folk. Et aktivt medlem har tilgang til alle byråets klienter gjennom oppdragene —
    admin styrer i tillegg byrået selv (medlemmer og oppdragsavgjørelser). Inviter med
    e-postadressen personen logger inn med; tilgangen blir til ved neste innlogging.
  </p>
  <table class="table table-sm mb-3">
    <thead>
      <tr><th>Navn</th><th>E-post</th><th>Rolle</th><th></th></tr>
    </thead>
    <tbody>
      {#each medlemmer as m (m.person_id)}
        <tr class={m.aktiv ? "" : "opacity-50"}>
          <td>{m.navn}</td>
          <td>{m.epost || ""}</td>
          <td>{m.rolle}</td>
          <td>
            {#if !m.aktiv}
              <button class="btn btn-xs btn-outline" onclick={() => gjenopprett(m)}>
                Gi tilgang igjen
              </button>
            {:else}
              <select
                class="select select-xs w-28 mr-1"
                value={m.rolle}
                onchange={(e) => endreRolle(m, e.currentTarget.value)}
              >
                {#each ROLLER as r}
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
      {#each ROLLER as r}
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
