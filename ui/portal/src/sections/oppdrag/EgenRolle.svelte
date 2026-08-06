<script>
  // En egendefinert rolle: rettighetene kan endres, rollen kan
  // deaktiveres — men aldri slettes, for da ville sporet av hvem som
  // hadde hva forsvinne.
  import { untrack } from "svelte";
  import { post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Rettighetsvelger from "./Rettighetsvelger.svelte";

  let { companyId, rolle, vokabular, onDone } = $props();

  // Med vilje et øyeblikksbilde: avkrysningene er et utkast man
  // redigerer, ikke serverens liste. Den er lik igjen etter Lagre.
  // svelte-ignore state_referenced_locally
  let valgte = $state(untrack(() => [...rolle.rettigheter]));

  async function lagre() {
    try {
      await send("PUT", "/companies/" + companyId + "/roles/" + rolle.id, {
        rettigheter: valgte,
      });
      toast("Rettighetene er lagret", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function deaktiver() {
    if (
      !(await bekreft("Deaktivere rollen? De som har den mister tilgangen med én gang.", {
        tittel: "Deaktiver rolle",
        ok: "Deaktiver",
        farlig: true,
      }))
    ) {
      return;
    }
    try {
      await post("/companies/" + companyId + "/roles/" + rolle.id + "/deactivate", {});
      toast("Rollen er deaktivert", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function aktiver() {
    try {
      await post("/companies/" + companyId + "/roles/" + rolle.id + "/restore", {});
      toast("Rollen er aktiv igjen", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<details
  class="collapse collapse-arrow border border-base-300 mb-2 {rolle.aktiv
    ? ''
    : 'opacity-50'}"
>
  <summary class="collapse-title min-h-0 py-2 pl-3 pr-8 font-semibold">
    {rolle.navn}
    <span class="opacity-70 text-sm font-normal">
      — {rolle.rettigheter.length} rettigheter, {rolle.i_bruk} med rollen{rolle.aktiv
        ? ""
        : ", deaktivert"}
    </span>
  </summary>
  <div class="collapse-content pl-3">
    <Rettighetsvelger {vokabular} bind:valgte />
    <div class="flex gap-2 mt-2">
      <button class="btn btn-xs" onclick={lagre}>Lagre</button>
      {#if rolle.aktiv}
        <button class="btn btn-xs btn-outline" onclick={deaktiver}>Deaktiver</button>
      {:else}
        <button class="btn btn-xs btn-outline" onclick={aktiver}>Aktiver igjen</button>
      {/if}
    </div>
  </div>
</details>
