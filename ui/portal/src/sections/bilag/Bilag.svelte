<script>
  import { api } from "../../lib/api.js";
  import { company } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import AnsattKvittering from "./AnsattKvittering.svelte";
  import Attestering from "./Attestering.svelte";
  import Innboks from "./Innboks.svelte";
  import EpostInn from "./EpostInn.svelte";
  import BilagListe from "./BilagListe.svelte";

  let { companyId } = $props();

  // En ansatt kan LASTE OPP en kvittering, men ikke lese innboksen,
  // bilagslisten eller attesteringskøen. Da får hun sin egen lille
  // side i stedet for den vanlige, som ville feilet på første kall.
  let bareEgne = $derived(company(companyId)?.access === "ansatt");

  let data = $state(null);

  async function load(id) {
    const [vouchers, inbox, dims, policy, members, mailSettings, mailLog] = await Promise.all([
      api("/companies/" + id + "/vouchers"),
      api("/companies/" + id + "/inbox"),
      api("/companies/" + id + "/dimensions").catch(() => ({ dimensions: [] })),
      api("/companies/" + id + "/attestering/policy").catch(() => ({ policy: null })),
      api("/companies/" + id + "/members").catch(() => ({ members: [] })),
      api("/companies/" + id + "/inbox/settings").catch(() => null),
      api("/companies/" + id + "/inbox/mail").catch(() => ({ mail: [] })),
    ]);
    data = {
      vouchers: vouchers.vouchers,
      inbox: inbox.documents,
      dims: dims.dimensions,
      policy: policy.policy,
      members: members.members,
      mailSettings,
      mailLog: mailLog.mail,
    };
  }

  function reload() {
    load(companyId).catch((error) => toast(error.message, false));
  }

  $effect(() => {
    if (bareEgne) return;
    data = null;
    load(companyId).catch((error) => toast(error.message, false));
  });
</script>

{#if bareEgne}
  <AnsattKvittering {companyId} />
{:else if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  <Attestering
    {companyId}
    inbox={data.inbox}
    policy={data.policy}
    members={data.members}
    onDone={reload}
  />
  <Innboks {companyId} inbox={data.inbox} dims={data.dims} policy={data.policy} onDone={reload} />
  {#if data.mailSettings}
    <EpostInn
      {companyId}
      settings={data.mailSettings}
      mailLog={data.mailLog}
      onDone={reload}
    />
  {/if}
  <BilagListe {companyId} vouchers={data.vouchers} />
{/if}
