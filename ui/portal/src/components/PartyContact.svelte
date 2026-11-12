<script>
  // A party's contact info with visible labels (the fields used to be
  // labelled only by placeholders, which vanish once filled in). Shared
  // by the customer page and the reskontro party page.
  import { untrack } from "svelte";
  import { send } from "../lib/api.js";
  import { toast } from "../lib/toast.svelte.js";

  let { companyId, party, onSaved = () => {} } = $props();

  let address = $state(untrack(() => party.address || ""));
  let email = $state(untrack(() => party.email || ""));
  let bankAccount = $state(untrack(() => party.bank_account || ""));
  let busy = $state(false);

  async function save(event) {
    event.preventDefault();
    busy = true;
    try {
      await send("PUT", "/companies/" + companyId + "/parties/" + party.party_id + "/contact", {
        address,
        email,
        bank_account: bankAccount,
      });
      toast("Kontaktinfo er lagret", true);
      onSaved();
    } catch (error) {
      toast(error.message, false);
    }
    busy = false;
  }
</script>

<form class="grid gap-3" onsubmit={save}>
  <label class="fieldset">
    <span class="fieldset-legend">Adresse</span>
    <input class="input w-full" autocomplete="street-address" bind:value={address} />
    <span class="label whitespace-normal">Står på fakturaen</span>
  </label>
  <label class="fieldset">
    <span class="fieldset-legend">E-post</span>
    <input class="input w-full" type="email" autocomplete="email" bind:value={email} />
    <span class="label whitespace-normal">Fakturaer og purringer sendes hit</span>
  </label>
  <label class="fieldset">
    <span class="fieldset-legend">Kontonummer</span>
    <input class="input w-full" inputmode="numeric" bind:value={bankAccount} />
    <span class="label whitespace-normal">11 siffer, brukes når dere betaler til parten</span>
  </label>
  <div>
    <button class="btn btn-sm" disabled={busy}>Lagre kontaktinfo</button>
  </div>
</form>
