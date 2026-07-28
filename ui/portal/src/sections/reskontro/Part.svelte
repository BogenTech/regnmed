<script>
  import { untrack } from "svelte";
  import { api, send } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, party, partyId } = $props();

  let items = $state(null);
  let address = $state(untrack(() => party?.address || ""));
  let email = $state(untrack(() => party?.email || ""));
  let bankAccount = $state(untrack(() => party?.bank_account || ""));

  $effect(() => {
    api("/companies/" + companyId + "/parties/" + partyId + "/items")
      .then((svar) => (items = svar.items))
      .catch((error) => toast(error.message, false));
  });

  async function save() {
    try {
      await send("PUT", "/companies/" + companyId + "/parties/" + partyId + "/contact", {
        address,
        email,
        bank_account: bankAccount,
      });
      toast("Kontaktinfo lagret", true);
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title={party?.name || ""}>
  <a href={"#/c/" + companyId + "/reskontro"} class="btn btn-ghost btn-xs w-fit">tilbake</a>
  {#if items}
    <table class="table table-sm">
      <thead>
        <tr>
          <th>Bilag</th><th>Dato</th><th>Tekst</th>
          <th class="text-right">Beløp</th><th class="text-right">Åpent</th>
        </tr>
      </thead>
      <tbody>
        {#each items as i}
          <tr>
            <td>{i.voucher}</td>
            <td>{i.date}</td>
            <td>{i.description || ""}</td>
            <td class="text-right">{kr(i.amount_ore)}</td>
            <td class="text-right">{kr(i.remaining_ore)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <span class="loading loading-spinner loading-sm"></span>
  {/if}
</Card>

{#if party}
  <Card title="Kontaktinfo">
    <div class="grid gap-2 max-w-md">
      <input
        class="input input-sm input-bordered"
        placeholder="Adresse (på fakturaen)"
        bind:value={address}
      />
      <input
        class="input input-sm input-bordered"
        placeholder="E-post (for utsendelse)"
        bind:value={email}
      />
      <input
        class="input input-sm input-bordered"
        placeholder="Kontonummer (for remittering)"
        bind:value={bankAccount}
      />
      <button class="btn btn-sm" onclick={save}>Lagre</button>
    </div>
  </Card>
{/if}
