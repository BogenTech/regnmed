<script>
  // Registeret er bevis: driftsmidler registreres og avhendes, aldri
  // redigeres eller slettes. Bokført verdi lagres ALDRI — den er
  // kostpris − bokførte avskrivninger, utledet hver gang.
  //
  // Stacked rows that fit a phone; each row opens its depreciation
  // history or the disposal form in place, and focus returns to the
  // button that opened it.
  import { tick } from "svelte";
  import { post } from "../../lib/api.js";
  import { formatDate, kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";
  import Avhending from "./Avhending.svelte";
  import Historikk from "./Historikk.svelte";
  import NyttDriftsmiddel from "./NyttDriftsmiddel.svelte";
  import { gruppeNavn, kjoringTekst } from "./anlegg.js";

  let { companyId, assets, kanSkrive, kontoer, onChanged } = $props();

  // { id, hva: "historikk" | "avhend" } — one panel open at a time.
  let apen = $state(null);
  let kjorer = $state(false);
  let resultat = $state(null);

  const erApen = (a, hva) => apen?.id === a.asset_id && apen.hva === hva;

  async function lukk() {
    const knapp = apen && apen.id + "-" + apen.hva;
    apen = null;
    await tick();
    document.getElementById(knapp)?.focus();
  }

  const veksle = (a, hva) => (erApen(a, hva) ? lukk() : (apen = { id: a.asset_id, hva }));

  async function generer() {
    if (kjorer) return;
    kjorer = true;
    resultat = null;
    try {
      const svar = await post("/companies/" + companyId + "/assets/depreciate", {});
      resultat = kjoringTekst(svar);
      if (svar.generated) await onChanged();
    } catch (error) {
      toast(error.message, false);
    } finally {
      kjorer = false;
    }
  }

  // The Avhend button is gone once the asset is disposed of, so focus
  // lands on the row's other button.
  async function avhendet() {
    const id = apen?.id;
    apen = null;
    await onChanged();
    await tick();
    document.getElementById(id + "-historikk")?.focus();
  }
</script>

<Card title="Anleggsregister">
  <p class="text-sm opacity-70 mb-2">
    Driftsmidler registreres og avhendes, men endres eller slettes aldri. Bokført verdi er kostpris
    minus bokførte avskrivninger. Lineære avskrivninger bokføres som egne bilag hver måned,
    automatisk eller med knappen. En måned i en låst periode bokføres første åpne dag.
  </p>
  {#if kanSkrive && assets.some((a) => !a.avhendet_dato)}
    <div class="mb-3">
      <button class="btn btn-sm btn-outline" disabled={kjorer} onclick={generer}>
        {#if kjorer}<span class="loading loading-spinner loading-xs"></span>{/if}
        Bokfør forfalte avskrivninger
      </button>
      <div role="status" class="text-sm mt-2">
        {#if resultat}
          <p>{resultat.bokfort}</p>
          {#if resultat.feil.length}
            <div class="alert alert-warning alert-soft mt-1">
              <div>
                <b>Ikke bokført:</b>
                <ul class="list-disc ml-4">
                  {#each resultat.feil as f, i (i)}<li>{f}</li>{/each}
                </ul>
              </div>
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}

  {#if assets.length}
    <ul class="divide-y divide-base-200">
      {#each assets as a (a.asset_id)}
        <li class="py-2">
          <div class="flex flex-wrap items-start gap-x-3 gap-y-1">
            <div class="min-w-0 flex-1 basis-56" class:opacity-60={a.avhendet_dato}>
              <div class="font-medium truncate">{a.navn}</div>
              <div class="text-xs opacity-70">
                Anskaffet {formatDate(a.anskaffelsesdato)}, {a.levetid_maneder} måneder, gruppe {a.saldogruppe}
                ({gruppeNavn(a.saldogruppe)})
              </div>
            </div>
            <div class="text-right text-sm shrink-0 ml-auto" class:opacity-60={a.avhendet_dato}>
              {#if a.avhendet_dato}
                <div>Avhendet {formatDate(a.avhendet_dato)}</div>
                <div class="text-xs opacity-70">vederlag {kr(a.vederlag_ore)} kr</div>
              {:else}
                <div class="tabular-nums">{kr(a.bokfort_ore)} kr</div>
                <div class="text-xs opacity-70 tabular-nums">
                  {kr(a.akkumulert_ore)} av {kr(a.kostpris_ore - a.restverdi_ore)} avskrevet
                </div>
              {/if}
            </div>
            <div class="flex gap-1 shrink-0">
              <button
                id={a.asset_id + "-historikk"}
                class="btn btn-xs btn-ghost"
                aria-expanded={erApen(a, "historikk")}
                aria-label={"Avskrivninger for " + a.navn}
                onclick={() => veksle(a, "historikk")}
              >
                Avskrivninger
              </button>
              {#if kanSkrive && !a.avhendet_dato}
                <button
                  id={a.asset_id + "-avhend"}
                  class="btn btn-xs btn-outline"
                  aria-expanded={erApen(a, "avhend")}
                  aria-label={"Avhend " + a.navn}
                  onclick={() => veksle(a, "avhend")}
                >
                  Avhend
                </button>
              {/if}
            </div>
          </div>
          {#if erApen(a, "historikk")}
            <Historikk {companyId} asset={a} onClose={lukk} />
          {:else if erApen(a, "avhend")}
            <Avhending {companyId} asset={a} onClose={lukk} onDone={avhendet} />
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm opacity-70">Ingen driftsmidler registrert ennå.</p>
  {/if}

  {#if kanSkrive}
    <h3 class="font-semibold mt-4 mb-1">Nytt driftsmiddel</h3>
    <NyttDriftsmiddel {companyId} {kontoer} onCreated={onChanged} />
  {/if}
</Card>
