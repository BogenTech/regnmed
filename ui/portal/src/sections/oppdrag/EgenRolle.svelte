<script>
  // En egendefinert rolle: rettighetene kan endres, rollen kan
  // deaktiveres — men aldri slettes, for da ville sporet av hvem som
  // hadde hva forsvinne.
  import { untrack } from "svelte";
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Rettighetsvelger from "./Rettighetsvelger.svelte";
  import { sammeSett } from "../brukere/brukere.js";

  let { companyId, rolle, vokabular, onDone } = $props();

  // Med vilje et øyeblikksbilde: avkrysningene er et utkast man
  // redigerer, ikke serverens liste. The parent keys this component on
  // the saved rights, so a save starts a fresh draft.
  // svelte-ignore state_referenced_locally
  let valgte = $state(untrack(() => [...rolle.rettigheter]));
  let opptatt = $state(false);
  let endret = $derived(!sammeSett(valgte, rolle.rettigheter));

  async function utfor(arbeid, melding) {
    opptatt = true;
    try {
      await arbeid();
      toast(melding, true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    } finally {
      opptatt = false;
    }
  }

  const lagre = () =>
    utfor(
      () => send("PUT", "/companies/" + companyId + "/roles/" + rolle.id, { rettigheter: valgte }),
      "Rettighetene for «" + rolle.navn + "» er lagret",
    );

  async function deaktiver() {
    const hvem = rolle.i_bruk
      ? rolle.i_bruk + (rolle.i_bruk === 1 ? " person" : " personer") + " med rollen mister tilgangen med én gang."
      : "Ingen har rollen nå.";
    if (
      !(await bekreft("Deaktivere rollen «" + rolle.navn + "»? " + hvem, {
        tittel: "Deaktiver rolle",
        ok: "Deaktiver",
        farlig: true,
      }))
    )
      return;
    await utfor(
      () => post("/companies/" + companyId + "/roles/" + rolle.id + "/deactivate", {}),
      "Rollen er deaktivert",
    );
  }

  const aktiver = () =>
    utfor(
      () => post("/companies/" + companyId + "/roles/" + rolle.id + "/restore", {}),
      "Rollen er aktiv igjen",
    );
</script>

<details class="collapse collapse-arrow border border-base-300 mb-2 {rolle.aktiv ? '' : 'opacity-60'}">
  <summary class="collapse-title min-h-0 py-2 pl-3 pr-8 font-semibold">
    {rolle.navn}
    <span class="opacity-70 text-sm font-normal">
      ({rolle.rettigheter.length} rettigheter, {rolle.i_bruk} med rollen{rolle.aktiv ? "" : ", deaktivert"})
    </span>
  </summary>
  <div class="collapse-content pl-3">
    <Rettighetsvelger {vokabular} bind:valgte />
    <div class="flex flex-wrap gap-2 mt-2 items-center">
      <button class="btn btn-sm btn-primary" disabled={!endret || opptatt} onclick={lagre}>Lagre</button>
      {#if endret}
        <button class="btn btn-sm btn-ghost" disabled={opptatt} onclick={() => (valgte = [...rolle.rettigheter])}>
          Angre
        </button>
      {/if}
      {#if rolle.aktiv}
        <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={deaktiver}>Deaktiver</button>
      {:else}
        <button class="btn btn-sm btn-outline" disabled={opptatt} onclick={aktiver}>Aktiver igjen</button>
      {/if}
      {#if endret}<span class="text-xs opacity-70" role="status">Ulagrede endringer</span>{/if}
    </div>
  </div>
</details>
