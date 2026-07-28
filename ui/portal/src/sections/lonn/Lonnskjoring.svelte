<script>
  import { post } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { download } from "../../lib/download.js";
  import Card from "../../components/Card.svelte";
  import { AGA_SONER, MANEDER } from "./konstanter.js";

  let { companyId, ansatte, kjoringer, onDone } = $props();

  const year = new Date().getFullYear();

  // En måned kan bare kjøres én gang — de kjørte månedene låses i
  // velgeren så feilen aldri når serveren.
  let kjortIAr = $derived(
    new Set(kjoringer.filter((k) => k.ar === year).map((k) => k.maned)),
  );
  let aktive = $derived(ansatte.filter((a) => !a.ansatt_til || a.ansatt_til >= today()));

  let maned = $state(1);
  let dato = $state(today());
  let sone = $state(AGA_SONER[0][0]);
  let linjer = $state([]);

  // Skjemaet bygges på nytt når grunnlaget endrer seg — den fulle
  // opptegningen app.js gjorde etter hver kjøring.
  $effect(() => {
    linjer = aktive.map((a) => ({
      employee_id: a.id,
      navn: a.navn,
      brutto: a.manedslonn_ore == null ? "" : (a.manedslonn_ore / 100).toFixed(2),
      fp: "",
      timelonn: !!a.timelonn_ore,
      fra_timer: false,
      med: true,
    }));
  });

  $effect(() => {
    const ledig = MANEDER.findIndex((_, i) => !kjortIAr.has(i + 1));
    maned = ledig >= 0 ? ledig + 1 : 1;
  });

  async function kjor() {
    const valgte = linjer
      .filter((l) => l.med)
      .map((l) => ({
        employee_id: l.employee_id,
        // Timelønn overstyrer et eventuelt bruttobeløp: serveren
        // regner det fra de låste timene.
        brutto_ore: l.fra_timer ? null : l.brutto.trim() ? parseKr(l.brutto.trim()) : null,
        feriepenger_ore: l.fp.trim() ? parseKr(l.fp.trim()) : 0,
        fra_timer: l.fra_timer,
      }));
    if (!valgte.length) {
      toast("Ingen ansatte er med i kjøringen", false);
      return;
    }
    try {
      const laget = await post("/companies/" + companyId + "/payroll", {
        ar: year,
        maned,
        utbetalt_dato: dato,
        sone,
        linjer: valgte,
      });
      toast("Lønn bokført — netto " + kr(laget.kjoring.netto_ore), true);
      // Advarsler er ikke feil, men de skal ikke forsvinne i en toast
      // som blinker forbi — de handler om tall som ikke går opp.
      (laget.advarsler || []).forEach((a) => toast(a, false));
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Lønnskjøring">
  <p class="text-sm opacity-70 mb-2">
    En kjøring blir <b>ett bilag</b>: lønn, forskuddstrekk, netto til utbetaling,
    feriepengeavsetning og arbeidsgiveravgift i én transaksjon. Netto føres mot
    <b>2930 Skyldig lønn</b> — selve utbetalingen gjøres i betalingslisten, så kjøring og utbetaling
    er to atskilte handlinger. En måned kan bare kjøres én gang; en retting er et reverserende bilag
    og en ny kjøring.
  </p>
  <p class="text-sm opacity-70 mb-2">
    Arbeidsgiveravgift på feriepenger som er <b>avsatt, men ikke utbetalt</b>, kostnadsføres
    samtidig som feriepengene opptjenes (<b>5405</b> mot <b>2780</b>) og føres tilbake når de
    utbetales. Kolonnen viser endringen: beløpet er alltid satsen av det som faktisk skyldes, så en
    utbetaling, en satsendring eller en gjeld som ikke bar avsetning fra før retter seg selv ved
    neste kjøring.
  </p>
  <p class="text-sm opacity-70 mb-2">
    Timelønn hentes fra timeføringen — men bare når <b>måneden er låst</b> i Timer-seksjonen. Ulåste
    timer kan endres etter at lønnen er bokført, og da spriker de to for alltid.
  </p>
  <div class="alert alert-warning text-sm mb-3">
    <div>
      <b>A-melding leveres ikke herfra.</b> Denne kjøringen er regnskapsføring, ikke rapportering —
      a-meldingen må fortsatt leveres på annen måte (frist den 5.). Se docs/lonn.md.
    </div>
  </div>

  {#if kjoringer.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Måned</th>
            <th>Sone</th>
            <th class="text-right">Brutto</th>
            <th class="text-right">Trekk</th>
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
          </tr>
        </thead>
        <tbody>
          {#each kjoringer as k (k.id)}
            <tr>
              <td>{MANEDER[k.maned - 1]} {k.ar}</td>
              <td class="text-xs opacity-70">sone {k.sone}</td>
              <td class="text-right">{kr(k.brutto_ore)}</td>
              <td class="text-right">{kr(k.forskuddstrekk_ore)}</td>
              <td class="text-right">{kr(k.netto_ore)}</td>
              <td class="text-right">{kr(k.feriepengeavsetning_ore)}</td>
              <td class="text-right">{kr(k.aga_ore)}</td>
              <td class="text-right">{kr(k.aga_feriepenger_ore)}</td>
              <td>
                {#each k.ansatte || [] as a (a.employee_id)}
                  <!-- Endepunktet krever bearer-token, så en vanlig lenke
                       ville fått 401 — download() henter og lagrer. -->
                  <button
                    class="btn btn-xs btn-ghost"
                    onclick={() =>
                      download(
                        "/companies/" +
                          companyId +
                          "/payroll/" +
                          k.id +
                          "/slip/" +
                          a.employee_id,
                        "lonnsslipp.pdf",
                      )}
                  >
                    {a.navn.split(" ")[0]}
                  </button>
                {/each}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  {#if aktive.length}
    <h3 class="font-semibold mb-1">Kjør lønn</h3>
    <div class="grid gap-2 mb-2 max-w-lg">
      <div class="grid grid-cols-3 gap-2">
        <select class="select select-sm select-bordered" bind:value={maned}>
          {#each MANEDER as navn, i (navn)}
            <option value={i + 1} disabled={kjortIAr.has(i + 1)}>
              {navn}{kjortIAr.has(i + 1) ? " (kjørt)" : ""}
            </option>
          {/each}
        </select>
        <input
          type="date"
          class="input input-sm input-bordered"
          title="Utbetalingsdato — styrer hvilke satser som gjelder"
          bind:value={dato}
        />
        <select class="select select-sm select-bordered" bind:value={sone}>
          {#each AGA_SONER as s (s[0])}
            <option value={s[0]}>Sone {s[1]}</option>
          {/each}
        </select>
      </div>
    </div>
    <div class="overflow-x-auto">
      <table class="table table-sm mb-2">
        <thead>
          <tr>
            <th>Ansatt</th>
            <th>Brutto (kr)</th>
            <th>Feriepenger (kr)</th>
            <th>Timelønn</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each linjer as l (l.employee_id)}
            <tr>
              <td>{l.navn}</td>
              <td>
                <input
                  class="input input-xs input-bordered w-32"
                  placeholder="Brutto"
                  bind:value={l.brutto}
                />
              </td>
              <td>
                <input
                  class="input input-xs input-bordered w-32"
                  placeholder="Feriepenger"
                  bind:value={l.fp}
                />
              </td>
              <td>
                {#if l.timelonn}
                  <label class="cursor-pointer text-xs">
                    <input type="checkbox" class="checkbox checkbox-xs" bind:checked={l.fra_timer} />
                    fra timer
                  </label>
                {:else}
                  <span class="opacity-40 text-xs">ingen timesats</span>
                {/if}
              </td>
              <td>
                <label class="cursor-pointer">
                  <input type="checkbox" class="checkbox checkbox-xs" bind:checked={l.med} />
                  med
                </label>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <button class="btn btn-sm" onclick={kjor}>Kjør lønn og bokfør</button>
  {:else}
    <p class="text-sm opacity-70">Registrer minst én ansatt for å kjøre lønn.</p>
  {/if}
</Card>
