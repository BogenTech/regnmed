<script>
  // Trekkpålegg registrert for hånd (docs/trekkpalegg.md §6, #339): fra
  // brevet, med brevet lagret ved siden av. En endring eller en stopp er en
  // ny versjon; ingenting rettes i ettertid. Synlig bare med LONN_LES —
  // uten den svarer serveren nei, og kortet viser seg ikke.
  import { api, post, send } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, sporsmal } from "../../lib/dialog.svelte.js";
  import { download } from "../../lib/download.js";
  import { kr, formatDate, today } from "../../lib/format.js";
  import { strictOre } from "../../lib/planer.js";
  import Card from "../../components/Card.svelte";

  let { companyId, ansatte, onDone } = $props();

  const TYPER = [
    ["samordnet", "Samordnet trekk (ny ordning)"],
    ["skatt", "Skatt og avgift (gammel ordning)"],
    ["ovrige", "Øvrige krav (gammel ordning)"],
  ];
  const typeNavn = (t) => TYPER.find((x) => x[0] === t)?.[1] ?? t;

  let data = $state(null);
  let arbeider = $state(false);

  async function hent() {
    try {
      data = await api("/companies/" + companyId + "/payroll/garnishments");
    } catch {
      data = null; // no LONN_LES: the card is not for this person
    }
  }
  $effect(() => {
    companyId;
    hent();
  });

  const tom = () => ({
    endrer: null, // the order being changed, or null for a new one
    employee_id: "",
    type: "samordnet",
    form: "belop",
    belop: "",
    prosent: "",
    fra: today(),
    til: "",
    kid: "",
    orgnr: "",
    konto: "",
    frist: "",
    notat: "",
    fil: null,
  });
  let skjema = $state(null);

  function endre(p) {
    const v = p.versjoner[0];
    const tekst = (ore) => (ore / 100).toFixed(2).replace(".", ",");
    skjema = {
      ...tom(),
      endrer: p.id,
      employee_id: p.employee_id,
      type: p.type,
      form: v.prosent_bp != null ? "prosent" : "belop",
      belop: v.belop_ore != null ? tekst(v.belop_ore) : "",
      prosent: v.prosent_bp != null ? tekst(v.prosent_bp) : "",
      fra: v.gjelder_fra,
      til: v.gjelder_til ?? "",
      kid: v.kid ?? "",
      orgnr: v.mottaker_orgnr ?? "",
      konto: v.mottaker_konto ?? "",
      frist: v.frist_dager != null ? String(v.frist_dager) : "",
    };
  }

  async function lagre(event) {
    event.preventDefault();
    const s = skjema;
    const form = s.type === "ovrige" ? "belop" : s.form;
    // Money and percent are read as TEXT, never through a float: "12,34"
    // is 1 234 øre or 1 234 basis points.
    const tall = form === "belop" ? strictOre(s.belop) : strictOre(s.prosent);
    if (tall == null) {
      toast(form === "belop" ? "Skriv beløpet i kroner, f.eks. 4 000 eller 4 000,50" : "Skriv prosenten med høyst to desimaler, f.eks. 12,5", false);
      return;
    }
    if (!s.endrer && !s.fil) {
      toast("Last opp brevet pålegget kom i", false);
      return;
    }
    arbeider = true;
    try {
      let brev_id = null;
      if (s.fil) {
        const svar = await api(
          "/companies/" + companyId + "/payroll/garnishments/letters?filename=" + encodeURIComponent(s.fil.name),
          { method: "POST", headers: { "content-type": s.fil.type || "application/octet-stream" }, body: s.fil },
        );
        brev_id = svar.brev_id;
      }
      const gammel = s.type !== "skatt";
      const body = {
        employee_id: s.employee_id || null,
        type: s.type,
        belop_ore: form === "belop" ? tall : null,
        prosent_bp: form === "prosent" ? tall : null,
        gjelder_fra: s.fra,
        gjelder_til: s.til || null,
        kid: gammel ? s.kid : null,
        mottaker_orgnr: gammel ? s.orgnr : null,
        mottaker_konto: gammel ? s.konto : null,
        frist_dager: s.type === "ovrige" && s.frist !== "" ? Number(s.frist) : null,
        brev_id,
        notat: s.notat || null,
      };
      const base = "/companies/" + companyId + "/payroll/garnishments";
      if (s.endrer) await send("PUT", base + "/" + s.endrer, body);
      else await post(base, body);
      toast(s.endrer ? "Ny versjon av pålegget er registrert" : "Pålegget er registrert", true);
      skjema = null;
      await hent();
      onDone?.();
    } catch (error) {
      toast(error.message, false);
    } finally {
      arbeider = false;
    }
  }

  async function stopp(p) {
    const notat = await sporsmal("Stoppe trekkpålegget for " + p.navn + "?", {
      melding: "Fra neste lønnskjøring trekkes ingenting. Stoppen lagres som en ny versjon og kan ikke angres — et nytt brev registreres som et nytt pålegg.",
      valgfri: true,
      ok: "Stopp pålegget",
      farlig: true,
    });
    if (notat === null) return;
    try {
      await post("/companies/" + companyId + "/payroll/garnishments/" + p.id + "/stop", { notat: notat || null });
      toast("Pålegget er stoppet", true);
      await hent();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function attester(p, v, godkjent) {
    let note = null;
    if (godkjent) {
      if (!(await bekreft("Godkjenne versjon " + v.versjon + " av trekkpålegget for " + p.navn + "? Sammenlign beløp og KID med brevet først."))) return;
    } else {
      note = await sporsmal("Hvorfor avvises pålegget?", { ok: "Avvis" });
      if (!note) return;
    }
    try {
      await post(
        "/companies/" + companyId + "/payroll/garnishments/" + p.id + "/versions/" + v.versjon + "/attest",
        { godkjent, note },
      );
      await hent();
    } catch (error) {
      toast(error.message, false);
    }
  }

  const storrelse = (v) =>
    v.belop_ore != null ? kr(v.belop_ore) + " kr/mnd" : (v.prosent_bp / 100).toFixed(2).replace(".", ",") + " %";

  // Payroll settings: the account and the company's own KID for skatt.
  let innst = $state(null);
  function aapneInnst() {
    const i = data.innstillinger ?? {};
    innst = {
      konto: i.konto ?? "",
      skatt_kid: i.skatt_kid ?? "",
      skatt_konto: i.skatt_konto ?? "",
      skatt_mottaker_orgnr: i.skatt_mottaker_orgnr ?? "",
      gjelder_fra: today(),
    };
  }
  async function lagreInnst(event) {
    event.preventDefault();
    try {
      await send("PUT", "/companies/" + companyId + "/payroll/garnishment-settings", innst);
      toast("Innstillingene er lagret", true);
      innst = null;
      await hent();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

{#if data}
  <Card title="Trekkpålegg">
    <p class="text-sm opacity-80 mb-3 max-w-3xl">
      Pålegg om utleggstrekk som kommer i brev, registreres her og trekkes i lønnskjøringen: bokført
      som leverandørpost med KID, betalt fra betalingslisten og rapportert i a-meldingen. Pålegg
      hentet direkte fra Skatteetaten kommer senere.
    </p>

    {#if data.palegg.length}
      <div class="overflow-x-auto mb-3">
        <table class="table table-sm">
          <thead>
            <tr>
              <th>Ansatt</th>
              <th>Type</th>
              <th class="text-right">Trekk</th>
              <th>Gjelder</th>
              <th>KID</th>
              <th>Status</th>
              <th><span class="sr-only">Handlinger</span></th>
            </tr>
          </thead>
          <tbody>
            {#each data.palegg as p (p.id)}
              {@const v = p.versjoner[0]}
              <tr>
                <td class="whitespace-nowrap">{p.navn}</td>
                <td class="text-xs">{typeNavn(p.type)}</td>
                <td class="text-right whitespace-nowrap">{storrelse(v)}</td>
                <td class="whitespace-nowrap text-xs">
                  {formatDate(v.gjelder_fra)}{v.gjelder_til ? " – " + formatDate(v.gjelder_til) : ""}
                </td>
                <td class="font-mono text-xs">{v.kid ?? "selskapets"}</td>
                <td>
                  <span class="badge badge-sm" class:badge-warning={p.status !== "aktiv" && p.status !== "stoppet"}>
                    {p.status}
                  </span>
                  <span class="text-xs opacity-60">v{v.versjon}</span>
                </td>
                <td class="whitespace-nowrap">
                  <button class="btn btn-ghost btn-xs" onclick={() => download(v.brev_url, v.brev_filnavn)}>
                    Brevet
                  </button>
                  {#if p.status === "venter på attestering"}
                    <button class="btn btn-ghost btn-xs" onclick={() => attester(p, v, true)}>Godkjenn</button>
                    <button class="btn btn-ghost btn-xs" onclick={() => attester(p, v, false)}>Avvis</button>
                  {/if}
                  {#if p.status !== "stoppet"}
                    <button class="btn btn-ghost btn-xs" onclick={() => endre(p)}>Endre</button>
                    <button class="btn btn-ghost btn-xs text-error" onclick={() => stopp(p)}>Stopp</button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    {#if skjema}
      <form class="border border-base-300 rounded-box p-3 mb-3 grid gap-3 max-w-3xl" onsubmit={lagre}>
        <h3 class="font-semibold">{skjema.endrer ? "Ny versjon av pålegget" : "Registrer pålegg fra brev"}</h3>
        <div class="grid sm:grid-cols-2 gap-3">
          <label class="flex flex-col gap-1 text-sm">
            Ansatt
            <select class="select select-sm" required disabled={!!skjema.endrer} bind:value={skjema.employee_id}>
              <option value="" disabled>Velg ansatt</option>
              {#each ansatte as a (a.id)}<option value={a.id}>{a.navn}</option>{/each}
            </select>
          </label>
          <label class="flex flex-col gap-1 text-sm">
            Type (står i brevet)
            <select class="select select-sm" disabled={!!skjema.endrer} bind:value={skjema.type}>
              {#each TYPER as t (t[0])}<option value={t[0]}>{t[1]}</option>{/each}
            </select>
          </label>
        </div>
        <fieldset class="grid sm:grid-cols-2 gap-3">
          <legend class="text-sm mb-1">Trekk per måned</legend>
          <div class="flex gap-3 text-sm">
            <label class="label cursor-pointer gap-2">
              <input type="radio" class="radio radio-sm" value="belop" bind:group={skjema.form} /> Beløp
            </label>
            <label class="label cursor-pointer gap-2" class:opacity-50={skjema.type === "ovrige"}>
              <input type="radio" class="radio radio-sm" value="prosent" disabled={skjema.type === "ovrige"} bind:group={skjema.form} />
              Prosent
            </label>
          </div>
          {#if skjema.form === "belop" || skjema.type === "ovrige"}
            <label class="flex flex-col gap-1 text-sm">
              Beløp per måned (kr)
              <input class="input input-sm text-right" inputmode="decimal" required bind:value={skjema.belop} />
            </label>
          {:else}
            <label class="flex flex-col gap-1 text-sm">
              Prosent av bruttolønn (høyst to desimaler)
              <input class="input input-sm text-right" inputmode="decimal" required bind:value={skjema.prosent} />
            </label>
          {/if}
        </fieldset>
        <div class="grid sm:grid-cols-2 gap-3">
          <label class="flex flex-col gap-1 text-sm">
            Gjelder fra
            <input type="date" class="input input-sm" required bind:value={skjema.fra} />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            Gjelder til (valgfri)
            <input type="date" class="input input-sm" bind:value={skjema.til} />
          </label>
        </div>
        {#if skjema.type === "skatt"}
          <p class="text-sm opacity-80">
            Utleggstrekk skatt betales samlet med selskapets egen KID for utleggstrekk skatt, ikke en KID
            per ansatt. Den legges inn under «Innstillinger» nedenfor.
          </p>
        {:else}
          <div class="grid sm:grid-cols-3 gap-3">
            <label class="flex flex-col gap-1 text-sm">
              KID
              <input class="input input-sm font-mono" inputmode="numeric" required bind:value={skjema.kid} />
            </label>
            <label class="flex flex-col gap-1 text-sm">
              Mottakers org.nr.
              <input class="input input-sm font-mono" inputmode="numeric" required bind:value={skjema.orgnr} />
            </label>
            <label class="flex flex-col gap-1 text-sm">
              Mottakers kontonummer
              <input class="input input-sm font-mono" inputmode="numeric" required bind:value={skjema.konto} />
            </label>
          </div>
        {/if}
        {#if skjema.type === "ovrige"}
          <label class="flex flex-col gap-1 text-sm max-w-xs">
            Betalingsfrist (dager etter utbetaling)
            <input type="number" min="0" max="365" class="input input-sm" required bind:value={skjema.frist} />
          </label>
        {/if}
        <label class="flex flex-col gap-1 text-sm">
          Brevet (PDF eller bilde){skjema.endrer ? " — valgfritt, ellers beholdes forrige" : ""}
          <input
            type="file"
            accept="application/pdf,image/*"
            class="file-input file-input-sm"
            required={!skjema.endrer}
            onchange={(e) => (skjema.fil = e.currentTarget.files?.[0] ?? null)}
          />
        </label>
        <label class="flex flex-col gap-1 text-sm">
          Notat (valgfritt)
          <input class="input input-sm" bind:value={skjema.notat} />
        </label>
        <div class="flex gap-2">
          <button class="btn btn-primary btn-sm" disabled={arbeider}>
            {skjema.endrer ? "Lagre ny versjon" : "Registrer pålegget"}
          </button>
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => (skjema = null)}>Avbryt</button>
        </div>
      </form>
    {:else}
      <button class="btn btn-sm w-fit mb-3" onclick={() => (skjema = tom())}>Registrer pålegg fra brev</button>
    {/if}

    <details class="text-sm" open={!!innst}>
      <summary class="cursor-pointer">Innstillinger for utleggstrekk</summary>
      {#if innst}
        <form class="grid gap-3 mt-2 max-w-3xl" onsubmit={lagreInnst}>
          <div class="grid sm:grid-cols-2 gap-3">
            <label class="flex flex-col gap-1">
              Konto for utleggstrekk (leverandørreskontro)
              <input class="input input-sm font-mono" placeholder="2610 velges automatisk" bind:value={innst.konto} />
            </label>
            <label class="flex flex-col gap-1">
              Gjelder fra
              <input type="date" class="input input-sm" required bind:value={innst.gjelder_fra} />
            </label>
          </div>
          <p class="opacity-80">Fra Skatteetatens betalingsinformasjon for utleggstrekk skatt:</p>
          <div class="grid sm:grid-cols-3 gap-3">
            <label class="flex flex-col gap-1">
              KID for utleggstrekk skatt
              <input class="input input-sm font-mono" inputmode="numeric" bind:value={innst.skatt_kid} />
            </label>
            <label class="flex flex-col gap-1">
              Kontonummer
              <input class="input input-sm font-mono" inputmode="numeric" bind:value={innst.skatt_konto} />
            </label>
            <label class="flex flex-col gap-1">
              Mottakers org.nr.
              <input class="input input-sm font-mono" inputmode="numeric" bind:value={innst.skatt_mottaker_orgnr} />
            </label>
          </div>
          <div class="flex gap-2">
            <button class="btn btn-primary btn-sm">Lagre innstillingene</button>
            <button type="button" class="btn btn-ghost btn-sm" onclick={() => (innst = null)}>Avbryt</button>
          </div>
        </form>
      {:else}
        <dl class="grid grid-cols-2 gap-x-4 gap-y-1 mt-2 max-w-xl">
          <dt class="opacity-70">Konto</dt>
          <dd>{data.innstillinger?.konto ?? "velges ved første trekk (2610)"}</dd>
          <dt class="opacity-70">KID skatt</dt>
          <dd class="font-mono">{data.innstillinger?.skatt_kid ?? "mangler"}</dd>
          <dt class="opacity-70">Konto skatt</dt>
          <dd class="font-mono">{data.innstillinger?.skatt_konto ?? "mangler"}</dd>
          <dt class="opacity-70">Mottaker skatt</dt>
          <dd class="font-mono">{data.innstillinger?.skatt_mottaker_orgnr ?? "mangler"}</dd>
        </dl>
        <button class="btn btn-ghost btn-sm mt-2" onclick={aapneInnst}>Endre innstillingene</button>
      {/if}
    </details>
  </Card>
{/if}
