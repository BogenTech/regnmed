<script>
  // «Inviter folkene dine» (#79) — the step registration never had. The
  // machinery exists (invitations to an address, redeemed at login;
  // roles as sets of rights), but it lives under Oppdrag → Tilgang where
  // a fresh admin never looks. This card guides the FIRST invitations
  // and disappears once the company has a second direct member. Role
  // descriptions come from the server (/roles → Rolle::beskrivelse) —
  // one copy, next to the bundles that make them true.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId } = $props();

  // Typical people, mapped to the role that fits them. The external
  // regnskapsfører is deliberately NOT an invitation — access through an
  // oppdrag follows the engagement and can be ended the same day.
  const PROFILER = [
    { navn: "Lønnsmottaker", rolle: "ansatt" },
    { navn: "Økonomiansvarlig", rolle: "bokforing" },
    { navn: "Medeier / daglig leder", rolle: "admin" },
    { navn: "Ekstern regnskapsfører", rolle: null },
  ];

  let synlig = $state(false);
  let roller = $state([]);
  let invitasjoner = $state([]);
  let profil = $state(PROFILER[0]);
  let epost = $state("");

  async function load(id) {
    try {
      const [access, invitations, roles] = await Promise.all([
        api("/companies/" + id + "/access"),
        api("/companies/" + id + "/invitations"),
        api("/companies/" + id + "/roles"),
      ]);
      const direkte = access.medlemmer.filter((m) => m.kan_endres && m.aktiv);
      synlig = direkte.length === 1;
      invitasjoner = invitations.invitasjoner;
      roller = roles.innebygde;
    } catch (e) {
      // Not an admin (404) — the card is not for this viewer.
      synlig = false;
    }
  }

  $effect(() => {
    synlig = false;
    load(companyId);
  });

  const beskrivelse = $derived(
    profil.rolle ? roller.find((r) => r.navn === profil.rolle)?.beskrivelse || "" : "",
  );

  async function inviter() {
    try {
      const svar = await post("/companies/" + companyId + "/invitations", {
        epost: epost.trim(),
        rolle: profil.rolle,
      });
      toast(
        svar.epost_sendt
          ? "Invitasjonen er sendt"
          : "Invitasjonen er registrert, men e-posten gikk ikke ut — si fra til vedkommende selv",
        svar.epost_sendt,
      );
      epost = "";
      load(companyId);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if synlig}
  <Card title="Inviter folkene dine">
    <p class="text-sm opacity-70 mb-3">
      Selskapet har bare deg. Inviter med e-postadressen personen logger inn med — tilgangen blir
      til ved neste innlogging, og e-posten inneholder ingen hemmelig lenke.
    </p>
    <div class="flex flex-wrap gap-2 mb-3">
      {#each PROFILER as p (p.navn)}
        <button
          class={"btn btn-sm " + (profil.navn === p.navn ? "btn-primary" : "btn-outline")}
          onclick={() => (profil = p)}
        >
          {p.navn}
        </button>
      {/each}
    </div>

    {#if profil.rolle === null}
      <div class="alert alert-info text-sm">
        <span>
          En ekstern regnskapsfører inviteres ikke som medlem — gi byrået et <b>oppdrag</b> i
          stedet. Da følger tilgangen oppdraget: den finnes så lenge avtalen består, og kan
          avsluttes samme dag.
        </span>
        <a class="btn btn-sm" href={"#/c/" + companyId + "/oppdrag"}>Finn byrå</a>
      </div>
    {:else}
      <p class="text-sm mb-3">
        <span class="badge badge-ghost mr-1">{profil.rolle}</span>
        {beskrivelse}
      </p>
      <div class="flex gap-2 items-center flex-wrap">
        <input
          class="input input-sm input-bordered w-64"
          placeholder="e-postadresse"
          bind:value={epost}
          onkeydown={(e) => e.key === "Enter" && inviter()}
        />
        <button class="btn btn-sm btn-primary" onclick={inviter}>Inviter</button>
      </div>
    {/if}

    {#if invitasjoner.length}
      <h3 class="font-semibold text-sm mt-4 mb-1">Venter på innlogging</h3>
      <table class="table table-sm">
        <thead><tr><th>E-post</th><th>Rolle</th><th>E-post sendt</th></tr></thead>
        <tbody>
          {#each invitasjoner as i (i.id)}
            <tr>
              <td>{i.epost}</td>
              <td>{i.rolle}</td>
              <td class="text-xs opacity-70">
                {i.sist_sendt ? new Date(i.sist_sendt).toLocaleString("no") : "ikke sendt"}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="text-xs opacity-60 mt-1">
        Innløses automatisk når adressen logger inn. Full tilgangsstyring:
        <a class="link" href={"#/c/" + companyId + "/oppdrag"}>Oppdrag → Tilgang</a>.
      </p>
    {/if}
  </Card>
{/if}
