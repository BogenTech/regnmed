<script>
  import { api, post } from "../../lib/api.js";
  import { kr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";
  import { MANEDER } from "./konstanter.js";
  import { slipFilename } from "./periode.js";

  let { companyId, ansatte, kjoringer, maaneder = [], onMaaneder } = $props();

  // Skatteetatens siste ord per måned. Et svar på én måned kan legge avvik
  // på en annen, så dette følger selskapet og ikke den åpne kjøringen.
  const medAvvik = $derived(maaneder.filter((m) => m.avvik.length));
  const maanedNavn = (periode) => {
    const [ar, maned] = periode.split("-");
    return MANEDER[Number(maned) - 1] + " " + ar;
  };
  const kildeFor = (periode) => {
    const kjoring = kjoringer.find((k) => k.utbetalt_dato?.startsWith(periode));
    return kjoring ? fraKjoring(kjoring) : fraManed(periode);
  };
  const ansattNavn = (id) => ansatte.find((a) => a.id === id)?.navn;

  // A-meldingen for én lagret kjøring — eller for en måned UTEN kjøring,
  // der arbeidsforholdene likevel skal rapporteres (F108). Alt som hindrer
  // levering listes samlet, og filen kan lastes ned først når listen er tom.
  let amelding = $state(null); // { kilde, svar, innsendinger, mulig }
  let sender = $state(false);
  let tomManed = $state(today().slice(0, 7));

  const fraKjoring = (k) => ({
    sti: "/companies/" + companyId + "/payroll/" + k.id + "/amelding",
    tittel: MANEDER[k.maned - 1] + " " + k.ar,
    merknad: "(utbetalingsmåneden)",
  });
  const fraManed = (m) => ({
    sti: "/companies/" + companyId + "/payroll/months/" + m + "/amelding",
    tittel: m + " uten lønnskjøring",
    merknad: "(bare arbeidsforholdene)",
  });

  async function sjekkAmelding(kilde) {
    try {
      const [svar, liste] = await Promise.all([api(kilde.sti), api(kilde.sti + "/submissions")]);
      amelding = { kilde, svar, innsendinger: liste.innsendinger, mulig: liste.innsending_mulig };
      onMaaneder?.(liste.amelding_maaneder);
    } catch (error) {
      toast(error.message, false);
    }
  }

  // Innsendingen lagres FØR den sendes: feiler den, sendes samme melding
  // uendret neste gang. Er den levert, kan kjøringen bare sendes på nytt
  // som erstatning — to meldinger for samme måned legges sammen hos
  // Skatteetaten (docs/lonn.md).
  async function sendAmelding(kilde, erstatt) {
    if (
      erstatt &&
      !(await bekreft(
        "Sende a-meldingen på nytt som en erstatning? Den forrige meldingen erstattes hos Skatteetaten.",
        { tittel: "Erstatt a-melding", ok: "Send erstatning" },
      ))
    ) {
      return;
    }
    sender = true;
    try {
      await post(kilde.sti + "/submissions", { erstatt });
      toast("A-meldingen er levert — hent tilbakemeldingen om litt", true);
    } catch (error) {
      toast(error.message, false);
    } finally {
      sender = false;
      await sjekkAmelding(kilde);
    }
  }

  async function hentTilbakemelding(kilde, innsending) {
    try {
      const svar = await post(kilde.sti + "/submissions/" + innsending.id + "/feedback", {});
      if (svar.status !== "tilbakemelding") {
        toast("Skatteetaten har ikke svart ennå — prøv igjen om litt", false);
      }
      await sjekkAmelding(kilde);
    } catch (error) {
      toast(error.message, false);
    }
  }

  const STATUS = {
    ikke_levert: "ikke levert",
    levert: "levert, venter på tilbakemelding",
    tilbakemelding: "tilbakemelding mottatt",
  };

</script>

<Card title="Lønnskjøringer og a-melding">
  <p class="text-sm opacity-70 mb-3">
    A-meldingen kan alltid lastes ned som fil fra en kjøring, og sendes herfra der innsending er satt
    opp. Sjekk at Skatteetaten har svart «mottatt» uten avvik før fristen den 5.
  </p>
  {#if medAvvik.length}
    <div role="alert" class="alert alert-warning alert-soft mb-4 text-sm">
      <div class="w-full">
        <p class="font-semibold">Skatteetaten melder avvik</p>
        <ul class="mt-1">
          {#each medAvvik as m (m.periode)}
            <li class="flex flex-wrap items-center gap-2">
              <span>
                {maanedNavn(m.periode)}: {m.avvik.length} avvik
              </span>
              <button
                class="btn btn-xs"
                aria-label={"Se avvik for " + maanedNavn(m.periode)}
                onclick={() => sjekkAmelding(kildeFor(m.periode))}
              >
                Se avvik
              </button>
            </li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}
  {#if kjoringer.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Måned</th>
            <th class="text-right">Brutto</th>
            <th class="text-right">Feriepenger utbetalt</th>
            <th class="text-right">Trekk</th>
            <th class="text-right">Kjøring og diett</th>
            <th class="text-right">Netto</th>
            <th class="text-right">Feriepenger avsatt</th>
            <th class="text-right">Aga</th>
            <th
              class="text-right"
              title="Endring i avsetningen på feriepenger som ennå ikke er utbetalt"
            >
              Aga feriepenger
            </th>
            <th>Lønnsslipp</th>
            <th>A-melding</th>
          </tr>
        </thead>
        <tbody>
          {#each kjoringer as k (k.id)}
            <tr>
              <td class="whitespace-nowrap">
                {MANEDER[k.maned - 1]} {k.ar}
                <div class="text-xs opacity-70">sone {k.sone}</div>
              </td>
              <td class="text-right whitespace-nowrap">{kr(k.brutto_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.feriepenger_utbetalt_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.forskuddstrekk_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.kjoregodtgjorelse_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.netto_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.feriepengeavsetning_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.aga_ore)}</td>
              <td class="text-right whitespace-nowrap">{kr(k.aga_feriepenger_ore)}</td>
              <td>
                {#each k.ansatte || [] as a (a.employee_id)}
                  <!-- Endepunktet krever bearer-token, så en vanlig lenke
                       ville fått 401 — download() henter og lagrer. -->
                  <button
                    class="btn btn-xs btn-ghost whitespace-nowrap"
                    aria-label={"Lønnsslipp for " + a.navn + ", " + MANEDER[k.maned - 1] + " " + k.ar}
                    onclick={() =>
                      download(
                        "/companies/" + companyId + "/payroll/" + k.id + "/slip/" + a.employee_id,
                        slipFilename(k.ar, k.maned, a.navn),
                      )}
                  >
                    {a.navn}
                  </button>
                {/each}
              </td>
              <td>
                <button
                  class="btn btn-xs btn-ghost"
                  aria-label={"Sjekk a-melding for " + MANEDER[k.maned - 1] + " " + k.ar}
                  onclick={() => sjekkAmelding(fraKjoring(k))}
                >
                  Sjekk
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  <!-- Arbeidsforholdene rapporteres hver måned, også uten lønn (F108). -->
  {#if ansatte.length}
    <div class="flex flex-wrap items-center gap-2 mb-4 text-sm">
      <label class="flex items-center gap-2">
        <span class="opacity-70">A-melding for en måned uten lønnskjøring</span>
        <input type="month" class="input input-sm w-40" bind:value={tomManed} />
      </label>
      <button
        class="btn btn-xs btn-ghost"
        disabled={!tomManed}
        onclick={() => sjekkAmelding(fraManed(tomManed))}
      >
        Sjekk
      </button>
    </div>
  {/if}
  {#if amelding}
    {@const kilde = amelding.kilde}
    {@const svar = amelding.svar}
    {@const maaned = maaneder.find((m) => m.periode === svar.kalendermaaned)}
    <div class="card border border-base-300 card-sm mb-4 max-w-2xl">
      <div class="card-body">
        <h4 class="font-semibold text-sm">
          A-melding for {kilde.tittel}
          <span class="opacity-70 font-normal">
            — rapporteres for {svar.kalendermaaned} {kilde.merknad}
          </span>
        </h4>
        {#if svar.leverbar}
          <p class="text-sm">
            Klar: {svar.inntektsmottakere} inntektsmottaker{svar.inntektsmottakere === 1
              ? ""
              : "e"}, arbeidsgiveravgift {kr(svar.arbeidsgiveravgift_ore)}.
          </p>
          {@const levert = amelding.innsendinger.some((i) => i.status !== "ikke_levert")}
          <div class="flex flex-wrap gap-2">
            {#if amelding.mulig}
              <button
                class="btn btn-sm btn-primary"
                disabled={sender}
                onclick={() => sendAmelding(kilde, levert)}
              >
                {levert ? "Send som erstatning" : "Send til Skatteetaten"}
              </button>
            {/if}
            <button
              class="btn btn-sm"
              onclick={() =>
                download(kilde.sti + "?format=xml", "a-melding-" + svar.kalendermaaned + ".xml")}
            >
              Last ned XML
            </button>
            <button class="btn btn-ghost btn-sm" onclick={() => (amelding = null)}>Lukk</button>
          </div>
          {#if !amelding.mulig}
            <p class="text-xs opacity-70">
              Innsending er ikke satt opp på denne serveren — lever filen selv.
            </p>
          {/if}
        {:else}
          <p class="text-sm mb-1">Kan ikke lages ennå — dette må rettes først:</p>
          <ul class="list-disc ml-5 text-sm text-warning mb-2">
            {#each svar.hindringer as h, i (i)}
              <li>{h}</li>
            {/each}
          </ul>
          <button class="btn btn-ghost btn-sm w-fit" onclick={() => (amelding = null)}>
            Lukk
          </button>
        {/if}
        {#if amelding.innsendinger.length}
          <h5 class="font-semibold text-xs mt-2">Innsendinger</h5>
          <ul class="text-sm">
            {#each amelding.innsendinger as i (i.id)}
              <li class="flex flex-wrap items-center gap-2">
                <span class="opacity-70">{new Date(i.opprettet).toLocaleString("nb-NO")}</span>
                <span>{STATUS[i.status]}</span>
                {#if i.erstatter_melding_id}<span class="badge badge-sm">erstatning</span>{/if}
                {#if i.tilbakemelding}
                  <span class={i.tilbakemelding.avvik?.length ? "text-warning" : ""}>
                    — {i.tilbakemelding.mottakstatus ?? "ukjent"}{i.tilbakemelding.avvik?.length
                      ? ", " + i.tilbakemelding.avvik.length + " avvik på meldingen"
                      : ""}
                  </span>
                {/if}
                {#if i.siste_feil}
                  <span class="text-warning text-xs" title={i.siste_feil}>siste forsøk feilet</span>
                {/if}
                {#if i.status === "levert"}
                  <button class="btn btn-ghost btn-xs" onclick={() => hentTilbakemelding(kilde, i)}>
                    Hent tilbakemelding
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#if maaned?.avvik.length}
          <div class="text-sm text-warning mt-2">
            <p class="font-semibold">Avvik for {maanedNavn(maaned.periode)}</p>
            <ul class="list-disc ml-5">
              {#each maaned.avvik as a, n (n)}
                <li>
                  {#if ansattNavn(a.arbeidsforhold_id)}{ansattNavn(a.arbeidsforhold_id)}:{/if}
                  {a.navn}
                  {a.detaljer}
                  <span class="opacity-70">({a.id})</span>
                </li>
              {/each}
            </ul>
            <p class="mt-1">
              Rett det som mangler, og send {amelding.innsendinger.some(
                (i) => i.status !== "ikke_levert",
              )
                ? "måneden som en erstatning"
                : "a-meldingen for måneden"}.
            </p>
          </div>
        {:else if maaned}
          <p class="text-sm mt-2">Skatteetaten har ingen avvik for måneden.</p>
        {/if}
      </div>
    </div>
  {/if}

</Card>
