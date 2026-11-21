<script>
  // The company's approval of regnmed in Altinn (docs/gov.md). regnmed asks,
  // and someone with authority at the company approves it in Altinn.
  import { api, post } from "../../lib/api.js";
  import { safeLink } from "../../lib/ehf.js";
  import { harRett } from "../../lib/me.svelte.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId } = $props();

  let state_ = $state(null);
  let working = $state(false);
  let loadFailed = $state(false);
  let link = $derived(safeLink(state_?.godkjenningslenke));

  // Not reactive: a click makes any check still in flight out of date.
  let clicks = 0;

  const url = (id) => "/companies/" + id + "/altinn/system-user";

  $effect(() => {
    const id = companyId;
    const clicksAtLoad = clicks;
    state_ = null;
    loadFailed = false;
    if (!harRett(id, "SELSKAP_ADMIN")) return;
    api(url(id))
      .then((answer) => {
        if (id !== companyId) return;
        state_ = answer;
        // Back from Altinn, or access was withdrawn there since last time.
        const open = ["New", "Accepted", "Revoked"].includes(answer.status);
        if (open && answer.kontrollerbar) return post(url(id) + "/check", {}).then((fresh) => {
          if (id === companyId && clicks === clicksAtLoad) state_ = fresh;
        }).catch(() => {});
      })
      .catch(() => {
        if (id === companyId) loadFailed = true;
      });
  });

  async function send(path) {
    const id = companyId;
    clicks += 1;
    working = true;
    try {
      const answer = await post(path(id), {});
      if (id !== companyId) return;
      state_ = answer;
      if (answer.godkjent) toast("regnmed er godkjent i Altinn", true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      working = false;
    }
  }

  const requestAccess = () => send(url);
  // An approval with no request to look up can only be confirmed by asking again.
  const checkStatus = () => send(state_?.kontrollerbar ? (id) => url(id) + "/check" : url);
</script>

{#if loadFailed}
  <Card title="Altinn-tilgang">
    <p>Kunne ikke hente status for Altinn-tilgangen. Last siden på nytt.</p>
  </Card>
{:else if state_?.tilgjengelig}
  <Card title="Altinn-tilgang">
    {#if state_.godkjent}
      <p>regnmed er godkjent i Altinn og kan levere a-melding og aksjonærregisteroppgave og hente skattekort.</p>
      <button class="btn mt-3" onclick={checkStatus} disabled={working} aria-busy={working}>
        Sjekk status
      </button>
    {:else}
      <p class="mb-3">
        For å levere a-melding og aksjonærregisteroppgave og hente skattekort må noen med fullmakt
        i selskapet godkjenne regnmed i Altinn.
      </p>
      {#if state_.status === "Revoked"}
        <p class="mb-3 text-sm font-medium" role="status">
          Tilgangen er trukket tilbake i Altinn. regnmed kan ikke levere for selskapet før den er
          godkjent på nytt.
        </p>
      {:else if state_.status && state_.status !== "New"}
        <p class="mb-3 text-sm opacity-70">Forrige forespørsel ble ikke godkjent. Du kan sende en ny.</p>
      {/if}
      {#if link}
        <a class="btn btn-primary" href={link}>Godkjenn i Altinn</a>
        <button class="btn ml-2" onclick={checkStatus} disabled={working} aria-busy={working}>
          Sjekk status
        </button>
      {:else}
        <button class="btn btn-primary" onclick={requestAccess} disabled={working} aria-busy={working}>
          Koble til Altinn
        </button>
      {/if}
    {/if}
  </Card>
{/if}
