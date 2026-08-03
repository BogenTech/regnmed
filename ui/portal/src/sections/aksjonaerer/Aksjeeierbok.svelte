<script>
  import { api, post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, bok, onDone } = $props();

  // null = dagens bok slik serveren leverte den; ellers en visning hentet
  // for en annen dato.
  let historisk = $state(null);
  let valgtDato = $state("");
  let dato = $derived(valgtDato || bok.dato);
  let rader = $derived(historisk ? historisk.aksjonarer : bok.aksjonarer);
  // Adresseknappen hører til dagens bok, ikke til en historisk visning.
  let medEndre = $derived(historisk === null);

  $effect(() => {
    // Ny bok fra serveren → tilbake til dagens visning.
    if (bok) {
      historisk = null;
      valgtDato = "";
    }
  });

  let kind = $state("person");
  let navn = $state("");
  let identVerdi = $state("");
  let adresseFelt = $state("");
  let postnummer = $state("");
  let poststed = $state("");
  let landkode = $state("");

  let identPlaceholder = $derived(
    kind === "person"
      ? "Fødselsnummer (11 siffer)"
      : kind === "selskap"
        ? "Organisasjonsnummer (9 siffer)"
        : "Aksjonær-ID (UTL000000000)",
  );

  // Aksjeloven §4-5 ber om fødselsdato — ikke fødselsnummer.
  function ident(a) {
    if (a.orgnr) return "org.nr " + a.orgnr;
    if (a.utenlandsk_id) return a.utenlandsk_id;
    return a.fodselsdato ? "f. " + a.fodselsdato : "";
  }

  function adresse(a) {
    return [a.adresse, [a.postnummer, a.poststed].filter(Boolean).join(" ")]
      .filter(Boolean)
      .join(", ");
  }

  async function perDato(ny) {
    valgtDato = ny;
    try {
      historisk = await api("/companies/" + companyId + "/shareholders?dato=" + ny);
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function opprett() {
    const body = {
      kind,
      navn: navn.trim(),
      adresse: adresseFelt.trim() || null,
      postnummer: postnummer.trim() || null,
      poststed: poststed.trim() || null,
      landkode: landkode.trim().toUpperCase() || null,
    };
    if (kind === "person") body.fodselsnummer = identVerdi.trim();
    else if (kind === "selskap") body.orgnr = identVerdi.trim();
    else body.utenlandsk_id = identVerdi.trim();
    try {
      await post("/companies/" + companyId + "/shareholders", body);
      toast("Aksjonær registrert", true);
      navn = "";
      identVerdi = "";
      adresseFelt = "";
      postnummer = "";
      poststed = "";
      landkode = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function endreAdresse(a) {
    const nyttNavn = prompt("Navn", a.navn);
    if (nyttNavn === null) return;
    const nyAdresse = prompt("Adresse", "");
    if (nyAdresse === null) return;
    const nyttPostnummer = prompt("Postnummer", "");
    if (nyttPostnummer === null) return;
    const nyttPoststed = prompt("Poststed", "");
    if (nyttPoststed === null) return;
    try {
      await send("PUT", "/companies/" + companyId + "/shareholders/" + a.id + "/contact", {
        navn: nyttNavn,
        adresse: nyAdresse || null,
        postnummer: nyttPostnummer || null,
        poststed: nyttPoststed || null,
        landkode: null,
      });
      toast("Kontaktopplysninger oppdatert", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Aksjeeierbok">
  <p class="text-sm opacity-70 mb-2">
    Lovpålagt register (aksjeloven §4-5): styret skal føre aksjeeierboken, og enhver har
    innsynsrett. Eierandelen lagres aldri — den er summen av hendelsene fram til datoen, akkurat som
    en saldo er summen av bilag. Boken viser <b>fødselsdato</b>, som er det loven ber om;
    fødselsnummeret brukes bare i innsendingen til Skatteetaten.
  </p>
  <div class="flex gap-2 items-end mb-3 flex-wrap">
    <label class="text-sm">
      Per dato
      <input
        type="date"
        class="input input-sm"
        value={dato}
        onchange={(e) => perDato(e.currentTarget.value)}
      />
    </label>
    <span class="text-sm opacity-70">Totalt <b>{bok.totalt_antall_aksjer}</b> aksjer</span>
  </div>
  <!-- Tabellen finnes når boken har eiere; en historisk visning kan
       være tom uten at «ingen registrert» blir sant. -->
  {#if bok.aksjonarer.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Navn</th>
            <th>Identitet</th>
            <th>Adresse</th>
            <th class="text-right">Aksjer</th>
            <th class="text-right">Andel</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each rader as a (a.id)}
            <tr class={a.antall_aksjer === 0 ? "opacity-50" : ""}>
              <td>{a.navn}</td>
              <td class="text-xs opacity-70">{ident(a)}</td>
              <td class="text-xs opacity-70">{adresse(a)}</td>
              <td class="text-right">{a.antall_aksjer}</td>
              <td class="text-right">{(a.andel_bp / 100).toFixed(2)} %</td>
              <td>
                {#if medEndre}
                  <button class="btn btn-xs btn-ghost" onclick={() => endreAdresse(a)}>
                    Endre adresse
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen aksjonærer registrert.</p>
  {/if}

  <h3 class="font-semibold mb-1">Ny aksjonær</h3>
  <div class="grid gap-2 max-w-md">
    <select class="select select-sm" bind:value={kind}>
      <option value="person">Person (fødselsnummer)</option>
      <option value="selskap">Selskap (organisasjonsnummer)</option>
      <option value="utenlandsk">Utenlandsk (UTL-id)</option>
    </select>
    <input class="input input-sm" placeholder="Navn" bind:value={navn} />
    <input
      class="input input-sm"
      placeholder={identPlaceholder}
      bind:value={identVerdi}
    />
    <input class="input input-sm" placeholder="Adresse" bind:value={adresseFelt} />
    <div class="grid grid-cols-3 gap-2">
      <input class="input input-sm" placeholder="Postnr" bind:value={postnummer} />
      <input class="input input-sm" placeholder="Poststed" bind:value={poststed} />
      <input class="input input-sm" placeholder="Land (SE)" bind:value={landkode} />
    </div>
    <button class="btn btn-sm" onclick={opprett}>Registrer aksjonær</button>
  </div>
</Card>
