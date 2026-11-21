<script>
  // The company as an EHF sender (docs/efaktura.md §3.6). A worker registers
  // it, but someone who can sign for the company must confirm their identity
  // at the access point.
  import { api, post } from "../../lib/api.js";
  import { safeLink } from "../../lib/ehf.js";
  import { waitForJob } from "../../lib/jobs.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId } = $props();

  let status = $state(null);
  let link = $state(null);
  let working = $state(false);

  $effect(() => {
    const id = companyId;
    status = null;
    link = null;
    if (!harRett(id, "SELSKAP_ADMIN")) return;
    api("/companies/" + id + "/peppol/sender")
      .then((answer) => {
        if (id === companyId) status = answer;
      })
      .catch(() => {});
  });

  async function activate() {
    working = true;
    try {
      const path = "/companies/" + companyId + "/peppol/sender";
      const started = await post(path, {});
      const result = await waitForJob(companyId, started.job_id);
      link = safeLink(result.verifiseringslenke);
      status = await api(path);
      if (result.er_verifisert) toast("Selskapet kan sende EHF", true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      working = false;
    }
  }
</script>

{#if status?.tilgjengelig}
  <Card title="EHF-faktura">
    {#if status.er_verifisert}
      <p>Selskapet er verifisert og kan sende EHF.</p>
    {:else}
      <p class="mb-3">
        Fra 1. januar 2027 må fakturaer til kjøpere som tar imot EHF, sendes som EHF. En som
        kan signere for selskapet må bekrefte identiteten sin én gang.
      </p>
      {#if link}
        <a class="btn btn-primary" href={link} target="_blank" rel="noopener noreferrer">
          Fullfør verifiseringen
        </a>
        <button class="btn ml-2" onclick={activate} disabled={working}>Sjekk status</button>
      {:else}
        <button class="btn btn-primary" onclick={activate} disabled={working}>
          {status.activatet ? "Fortsett verifiseringen" : "Aktiver EHF"}
        </button>
      {/if}
    {/if}
  </Card>
{/if}
