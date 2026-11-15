<script>
  // Aksjeeierboken (aksjeloven §4-5) as stacked rows that fit a phone.
  // Ownership is never stored: it is the sum of the events up to the
  // date, so a past date is just another question to the server — and
  // the total shown is that date's total, not today's.
  import { tick } from "svelte";
  import { api } from "../../lib/api.js";
  import { formatDate } from "../../lib/format.js";
  import Card from "../../components/Card.svelte";
  import LoadError from "../../components/LoadError.svelte";
  import Kontaktskjema from "./Kontaktskjema.svelte";
  import NyAksjonar from "./NyAksjonar.svelte";
  import { adresseTekst, andelTekst, identTekst } from "./aksjonaer.js";

  let { companyId, bok, kanSkrive, onChanged } = $props();

  // null = today's book as given; otherwise a view for another date.
  let historisk = $state(null);
  let valgtDato = $state("");
  let apen = $state(null);

  // A new book from the server means back to today.
  $effect(() => {
    bok;
    historisk = null;
    valgtDato = "";
  });

  let vist = $derived(historisk && !historisk.error && !historisk.laster ? historisk : bok);
  let iDag = $derived(historisk === null);

  async function perDato(ny) {
    valgtDato = ny;
    apen = null;
    if (!ny || ny === bok.dato) return (historisk = null);
    historisk = { laster: true };
    try {
      const svar = await api("/companies/" + companyId + "/shareholders?dato=" + ny);
      if (valgtDato === ny) historisk = svar;
    } catch (error) {
      if (valgtDato === ny) historisk = { error: error.message };
    }
  }

  async function lukk(id) {
    apen = null;
    await tick();
    document.getElementById("aksjonar-" + id)?.focus();
  }
</script>

<Card title="Aksjeeierbok">
  <p class="text-sm opacity-70 mb-2">
    Styret skal føre aksjeeierboken, og alle har rett til å se den (aksjeloven §4-5). Eierandelen
    regnes ut fra hendelsene fram til datoen, og boken viser fødselsdato, ikke fødselsnummer.
  </p>
  <div class="flex flex-wrap gap-x-4 gap-y-1 items-end mb-2">
    <label class="fieldset py-0">
      <span class="fieldset-legend">Per dato</span>
      <input
        type="date"
        class="input input-sm"
        value={valgtDato || bok.dato}
        onchange={(e) => perDato(e.currentTarget.value)}
      />
    </label>
    <p class="text-sm pb-2">
      {vist.totalt_antall_aksjer.toLocaleString("nb-NO")} aksjer
      {#if !iDag && !historisk?.laster && !historisk?.error}
        per {formatDate(valgtDato)}
      {/if}
    </p>
  </div>

  {#if historisk?.laster}
    <span class="loading loading-spinner loading-sm" aria-label="Laster aksjeeierboken"></span>
  {:else if historisk?.error}
    <LoadError error={historisk.error} retry={() => perDato(valgtDato)} />
  {:else if vist.aksjonarer.length}
    <ul class="divide-y divide-base-200">
      {#each vist.aksjonarer as a (a.id)}
        <li class="py-2">
          <div class="flex items-center gap-2" class:opacity-60={a.antall_aksjer === 0}>
            <div class="min-w-0 flex-1">
              <div class="truncate font-medium">{a.navn}</div>
              <div class="text-xs opacity-60">
                {[identTekst(a), adresseTekst(a)].filter(Boolean).join(", ")}
              </div>
            </div>
            <div class="text-right shrink-0 text-sm">
              <div>{a.antall_aksjer.toLocaleString("nb-NO")} aksjer</div>
              <div class="text-xs opacity-60">{andelTekst(a.andel_bp)}</div>
            </div>
            {#if kanSkrive && iDag}
              <button
                id={"aksjonar-" + a.id}
                class="btn btn-xs btn-ghost shrink-0"
                aria-expanded={apen === a.id}
                aria-label={(apen === a.id ? "Lukk " : "Endre ") + a.navn}
                onclick={() => (apen === a.id ? lukk(a.id) : (apen = a.id))}
              >
                {apen === a.id ? "Lukk" : "Endre"}
              </button>
            {/if}
          </div>
          {#if apen === a.id}
            <Kontaktskjema {companyId} aksjonar={a} onSaved={onChanged} onClose={() => lukk(a.id)} />
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70">
      {iDag ? "Ingen aksjonærer registrert ennå." : "Ingen eiere per denne datoen."}
    </p>
  {/if}

  {#if kanSkrive}
    <h3 class="font-semibold mt-4">Ny aksjonær</h3>
    <NyAksjonar {companyId} onCreated={onChanged} />
  {/if}
</Card>
