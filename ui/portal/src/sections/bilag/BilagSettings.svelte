<script>
  // Innstillinger for Bilag: attesteringspolicyen (#47) og e-post-inn
  // (#35). Flyttet ut av arbeidsflaten, der de sto foran innboksen.
  import { api } from "../../lib/api.js";
  import AttestPolicy from "./AttestPolicy.svelte";
  import EpostInn from "./EpostInn.svelte";

  let { companyId } = $props();

  let data = $state(null);
  function load() {
    const c = "/companies/" + companyId;
    Promise.all([
      api(c + "/attestering/policy").catch(() => ({ policy: null })),
      api(c + "/members").catch(() => ({ members: [] })),
      api(c + "/inbox/settings").catch(() => null),
      api(c + "/inbox/mail").catch(() => ({ mail: [] })),
    ]).then(([p, m, s, l]) => {
      data = { policy: p.policy, members: m.members, mailSettings: s, mailLog: l.mail };
    });
  }
  $effect(load);
</script>

{#if !data}
  <span class="loading loading-spinner loading-lg"></span>
{:else}
  {#key data.policy}
    <AttestPolicy {companyId} policy={data.policy} members={data.members} onSaved={load} />
  {/key}
  {#if data.mailSettings}
    <EpostInn {companyId} settings={data.mailSettings} mailLog={data.mailLog} onDone={load} />
  {/if}
{/if}
