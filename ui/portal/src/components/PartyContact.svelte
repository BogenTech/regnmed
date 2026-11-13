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
  let buyerReference = $state(untrack(() => party.buyer_reference || ""));
  let busy = $state(false);

  // What each field is FOR depends on the party: the address and e-mail
  // carry our invoices to a customer; the account number is where we pay
  // a supplier.
  const supplier = untrack(() => party.kind === "leverandor");
  const help = supplier
    ? {
        address: "Leverandørens adresse",
        email: "For kontakt om fakturaer",
        account: "Dit dere betaler. Uten det kan leverandøren ikke betales fra betalingslisten.",
      }
    : {
        address: "Står på fakturaen",
        email: "Fakturaer og purringer sendes hit",
        account: "11 siffer, bare hvis dere skal betale penger tilbake",
      };

  async function save(event) {
    event.preventDefault();
    busy = true;
    try {
      await send("PUT", "/companies/" + companyId + "/parties/" + party.party_id + "/contact", {
        address,
        email,
        bank_account: bankAccount,
        // Only a customer has a standing reference on our invoices.
        ...(supplier ? {} : { buyer_reference: buyerReference }),
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
    <span class="label whitespace-normal">{help.address}</span>
  </label>
  <label class="fieldset">
    <span class="fieldset-legend">E-post</span>
    <input class="input w-full" type="email" autocomplete="email" bind:value={email} />
    <span class="label whitespace-normal">{help.email}</span>
  </label>
  {#if !supplier}
    <label class="fieldset">
      <span class="fieldset-legend">Deres referanse</span>
      <input class="input w-full" bind:value={buyerReference} />
      <span class="label whitespace-normal">
        Kundens faste referanse på fakturaene, f.eks. bestillingsnummer eller kontaktperson. Offentlige kunder krever den.
      </span>
    </label>
  {/if}
  <label class="fieldset">
    <span class="fieldset-legend">Kontonummer</span>
    <input class="input w-full" inputmode="numeric" bind:value={bankAccount} />
    <span class="label whitespace-normal">{help.account}</span>
  </label>
  <div>
    <button class="btn btn-sm" disabled={busy}>Lagre kontaktinfo</button>
  </div>
</form>
