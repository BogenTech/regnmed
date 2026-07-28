<script>
  import { untrack } from "svelte";
  import { send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, settings } = $props();

  let address = $state(untrack(() => settings.address || ""));
  let email = $state(untrack(() => settings.email || ""));
  let bankAccount = $state(untrack(() => settings.bank_account || ""));
  let orgform = $state(untrack(() => settings.orgform || ""));

  const ORGFORMER = ["", "AS", "ASA", "ENK", "ANS", "DA"];

  async function save() {
    try {
      await send("PUT", "/companies/" + companyId + "/settings", {
        address,
        bank_account: bankAccount,
        orgform,
        email,
      });
      toast("Firmaopplysninger lagret", true);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Firmaopplysninger — på salgsdokumentene">
  <p class="text-sm opacity-70 mb-2">
    Adresse, kontonummer og selskapsform trykkes på faktura-PDF-en («Foretaksregisteret» påføres
    for AS/ASA).
  </p>
  <div class="grid gap-2 max-w-md">
    <input class="input input-sm input-bordered" placeholder="Adresse" bind:value={address} />
    <input
      class="input input-sm input-bordered"
      placeholder="E-post (svaradresse på utsendelser)"
      bind:value={email}
    />
    <div class="flex gap-2">
      <input
        class="input input-sm input-bordered flex-1"
        placeholder="Kontonummer"
        bind:value={bankAccount}
      />
      <select class="select select-sm select-bordered" bind:value={orgform}>
        {#each ORGFORMER as f}
          <option value={f}>{f || "(selskapsform)"}</option>
        {/each}
      </select>
    </div>
    <button class="btn btn-sm" onclick={save}>Lagre</button>
  </div>
</Card>
