<script>
  // A new shareholder. The identity asked for follows the kind, and is
  // permanent once registered — it is checked here only for shape; the
  // server checks the control digits and says which number is wrong.
  import { post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";

  let { companyId, onCreated } = $props();

  const IDENT = {
    person: { navn: "Fødselsnummer", felt: "fodselsnummer", hjelp: "11 siffer" },
    selskap: { navn: "Organisasjonsnummer", felt: "orgnr", hjelp: "9 siffer" },
    utenlandsk: {
      navn: "Aksjonær-ID",
      felt: "utenlandsk_id",
      hjelp: "UTL og ni siffer",
    },
  };
  const tomt = () => ({
    kind: "person",
    navn: "",
    ident: "",
    adresse: "",
    postnummer: "",
    poststed: "",
    landkode: "",
  });
  let felt = $state(tomt());
  let lagrer = $state(false);
  let ident = $derived(IDENT[felt.kind]);

  async function opprett(event) {
    event.preventDefault();
    if (lagrer) return;
    const tom = (v) => v.trim() || null;
    const body = {
      kind: felt.kind,
      navn: felt.navn.trim(),
      [ident.felt]: felt.ident.replace(/\s/g, "").toUpperCase(),
      adresse: tom(felt.adresse),
      postnummer: tom(felt.postnummer),
      poststed: tom(felt.poststed),
      landkode: tom(felt.landkode)?.toUpperCase() ?? null,
    };
    lagrer = true;
    try {
      await post("/companies/" + companyId + "/shareholders", body);
      toast(body.navn + " er registrert", true);
      felt = tomt();
      await onCreated();
      document.getElementById("ny-aksjonar-kind")?.focus();
    } catch (error) {
      toast(error.message, false);
    } finally {
      lagrer = false;
    }
  }
</script>

<form class="flex flex-wrap gap-x-3 gap-y-1 items-end text-sm" onsubmit={opprett}>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Type</span>
    <select id="ny-aksjonar-kind" class="select select-sm" bind:value={felt.kind}>
      <option value="person">Person</option>
      <option value="selskap">Selskap</option>
      <option value="utenlandsk">Utenlandsk</option>
    </select>
  </label>
  <label class="fieldset py-0 flex-1 min-w-48">
    <span class="fieldset-legend">Navn</span>
    <input class="input input-sm w-full" required bind:value={felt.navn} />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">{ident.navn} ({ident.hjelp})</span>
    <input
      class="input input-sm w-40"
      required
      inputmode={felt.kind === "utenlandsk" ? "text" : "numeric"}
      title={ident.navn + ": " + ident.hjelp + ". Kan ikke endres senere."}
      bind:value={felt.ident}
    />
  </label>
  <label class="fieldset py-0 flex-1 min-w-48">
    <span class="fieldset-legend">Adresse</span>
    <input class="input input-sm w-full" bind:value={felt.adresse} />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Postnummer</span>
    <input class="input input-sm w-24" inputmode="numeric" bind:value={felt.postnummer} />
  </label>
  <label class="fieldset py-0">
    <span class="fieldset-legend">Poststed</span>
    <input class="input input-sm w-36" bind:value={felt.poststed} />
  </label>
  {#if felt.kind === "utenlandsk"}
    <label class="fieldset py-0">
      <span class="fieldset-legend">Landkode</span>
      <input class="input input-sm w-20" maxlength="2" placeholder="SE" bind:value={felt.landkode} />
    </label>
  {/if}
  <button class="btn btn-sm btn-primary mb-1" disabled={lagrer}>Registrer aksjonær</button>
</form>
