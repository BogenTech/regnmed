<script>
  import { post, send } from "../../lib/api.js";
  import { kr, parseKr, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, ansatte, invitasjoner, medlemmer, onDone } = $props();

  let navn = $state("");
  let fnr = $state("");
  let stilling = $state("");
  let fra = $state(today());
  let lonn = $state("");
  let trekk = $state("");
  let fp = $state("10.2");
  let epost = $state("");

  function nullstill() {
    navn = "";
    fnr = "";
    stilling = "";
    fra = today();
    lonn = "";
    trekk = "";
    fp = "10.2";
    epost = "";
  }

  async function opprett() {
    try {
      const created = await post("/companies/" + companyId + "/employees", {
        navn: navn.trim(),
        fodselsnummer: fnr.trim(),
        stilling: stilling.trim() || null,
        ansatt_fra: fra.trim(),
        manedslonn_ore: lonn.trim() ? parseKr(lonn.trim()) : null,
        trekk_type: trekk.trim() ? "prosent" : "ingen",
        trekk_prosent_bp: trekk.trim()
          ? Math.round(parseFloat(trekk.trim().replace(",", ".")) * 100)
          : null,
        feriepenger_bp: Math.round(parseFloat(fp.trim().replace(",", ".")) * 100),
      });
      // Invitasjonen kobler den ansatte til portalbrukeren når hun selv
      // logger inn med adressen — admin velger aldri en person fra en
      // liste (docs/lonn.md).
      if (epost.trim()) {
        await inviterAnsatt(created.employee_id, epost.trim());
      }
      toast("Ansatt registrert", true);
      nullstill();
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function inviterAnsatt(employeeId, adresse) {
    const svar = await post("/companies/" + companyId + "/invitations", {
      epost: adresse,
      rolle: "ansatt",
      employee_id: employeeId,
    });
    toast(
      svar.epost_sendt
        ? "Invitasjonen er sendt — koblingen skjer når den ansatte logger inn"
        : "Invitasjonen er registrert, men e-posten gikk ikke ut — si fra til vedkommende selv",
      svar.epost_sendt,
    );
  }

  // Per-ansatt koblingsskjema, ett om gangen (samme mønster som
  // tildelingsskjemaet i plattformvisningen).
  let kobling = $state(null); // { ansatt, epost, personId }

  function apneKobling(a) {
    kobling = { ansatt: a, epost: "", personId: "" };
  }

  const invitasjonFor = (a) => (invitasjoner || []).find((i) => i.employee_id === a.id);

  // Bare direkte, aktive medlemmer kan velges; de som alt er koblet til
  // en annen ansatt filtreres bort så listen ikke tilbyr valg serveren
  // uansett nekter.
  const koblbareMedlemmer = $derived.by(() => {
    if (!medlemmer) return null;
    const koblet = new Set(ansatte.filter((a) => a.person).map((a) => a.person.person_id));
    return medlemmer.filter((m) => m.aktiv && m.kan_endres && !koblet.has(m.person_id));
  });

  async function inviterFraKobling() {
    try {
      await inviterAnsatt(kobling.ansatt.id, kobling.epost.trim());
      kobling = null;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function kobleTilMedlem() {
    try {
      await post("/companies/" + companyId + "/employees/" + kobling.ansatt.id + "/link", {
        person_id: kobling.personId,
      });
      toast("Koblet — handlingen er logget", true);
      kobling = null;
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function kobleFra(a) {
    try {
      await send("DELETE", "/companies/" + companyId + "/employees/" + a.id + "/link");
      toast("Koblingen er fjernet", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function trekkTilbake(inv) {
    try {
      await send("DELETE", "/companies/" + companyId + "/invitations/" + inv.id);
      toast("Invitasjonen er trukket tilbake", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="Ansatte">
  <p class="text-sm opacity-70 mb-2">
    Ansattregisteret. Identiteten (fødselsnummeret) er permanent og vises ikke her — listen viser
    <b>fødselsdato</b>, som er nok til å kjenne igjen en ansatt. Feriepengesatsen står per ansatt
    fordi den er et faktum om arbeidsforholdet: 10,2 % etter ferieloven §10, 12,5 % fra året man
    fyller 60, høyere på tariff. <b>Portalbruker</b>-kolonnen styrer selvbetjeningen: den koblede
    brukeren ser sin egen lønnsslipp, og timelønn hentes fra dennes timeføring. Tryggest er å
    invitere på e-post — da kobler den ansatte seg selv ved å logge inn.
  </p>
  {#if ansatte.length}
    <div class="overflow-x-auto">
      <table class="table table-sm mb-4">
        <thead>
          <tr>
            <th>Navn</th>
            <th>Stilling</th>
            <th>Født</th>
            <th class="text-right">Månedslønn</th>
            <th class="text-right">Trekk</th>
            <th class="text-right">Feriepenger</th>
            <th>Portalbruker</th>
          </tr>
        </thead>
        <tbody>
          {#each ansatte as a (a.id)}
            {@const inv = invitasjonFor(a)}
            <tr class={a.ansatt_til && a.ansatt_til < today() ? "opacity-50" : ""}>
              <td>{a.navn}</td>
              <td class="text-xs opacity-70">{a.stilling || ""}</td>
              <!-- Ferieloven krever ikke fnr her; datoen er nok for å kjenne igjen. -->
              <td class="text-xs opacity-70">{a.fodselsdato || ""}</td>
              <td class="text-right">{a.manedslonn_ore == null ? "" : kr(a.manedslonn_ore)}</td>
              <td class="text-right">
                {#if a.trekk_type === "prosent"}
                  {(a.trekk_prosent_bp / 100).toFixed(1)} %
                {:else if a.trekk_type === "tabell"}
                  <!-- Tabelltrekk regnes IKKE ut av oss: trekktabellene er
                       Skatteetatens datafiler, og en tilnærming blir den
                       ansattes restskatt. Derfor merket, ikke beregnet. -->
                  <span class="badge badge-warning badge-sm">tabell {a.trekk_tabell} ⚠</span>
                {:else}
                  frikort
                {/if}
              </td>
              <td class="text-right">{(a.feriepenger_bp / 100).toFixed(1)} %</td>
              <td class="text-xs">
                {#if a.person}
                  <span title={a.person.epost || ""}>{a.person.navn}</span>
                  <button class="btn btn-ghost btn-xs" onclick={() => kobleFra(a)}>
                    Koble fra
                  </button>
                {:else if inv}
                  <span class="opacity-70">invitert: {inv.epost}</span>
                  <button class="btn btn-ghost btn-xs" onclick={() => trekkTilbake(inv)}>
                    Trekk tilbake
                  </button>
                {:else}
                  <span class="opacity-50">ikke koblet</span>
                  <button class="btn btn-ghost btn-xs" onclick={() => apneKobling(a)}>
                    Koble …
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if kobling}
      <div class="card card-border card-sm mb-4 max-w-lg">
        <div class="card-body">
          <h4 class="font-semibold text-sm mb-2">Koble {kobling.ansatt.navn} til portalbruker</h4>
          <div class="flex flex-wrap gap-2 items-end">
            <input
              class="input input-sm"
              placeholder="E-postadresse"
              bind:value={kobling.epost}
            />
            <button
              class="btn btn-sm btn-primary"
              disabled={!kobling.epost.trim()}
              onclick={inviterFraKobling}
            >
              Inviter og koble
            </button>
          </div>
          <p class="text-xs opacity-70 mt-1 mb-2">
            Invitasjonen kobler når den ansatte selv logger inn med adressen — den kan ikke treffe
            feil person uten at noen andre har adgang til postkassen.
          </p>
          {#if koblbareMedlemmer && koblbareMedlemmer.length}
            <div class="flex flex-wrap gap-2 items-end">
              <select class="select select-sm" bind:value={kobling.personId}>
                <option value="">— eksisterende medlem —</option>
                {#each koblbareMedlemmer as m (m.person_id)}
                  <option value={m.person_id}>{m.navn}{m.epost ? " (" + m.epost + ")" : ""}</option>
                {/each}
              </select>
              <button class="btn btn-sm" disabled={!kobling.personId} onclick={kobleTilMedlem}>
                Koble
              </button>
            </div>
          {/if}
          <button class="btn btn-ghost btn-xs mt-2" onclick={() => (kobling = null)}>Avbryt</button>
        </div>
      </div>
    {/if}
  {:else}
    <p class="text-sm opacity-70 mb-3">Ingen ansatte registrert.</p>
  {/if}

  <h3 class="font-semibold mb-1">Ny ansatt</h3>
  <div class="grid gap-2 max-w-md">
    <input class="input input-sm" placeholder="Navn" bind:value={navn} />
    <div class="grid grid-cols-2 gap-2">
      <input
        class="input input-sm"
        placeholder="Fødselsnummer (11 siffer)"
        bind:value={fnr}
      />
      <input class="input input-sm" placeholder="Stilling" bind:value={stilling} />
    </div>
    <div class="grid grid-cols-2 gap-2">
      <input type="date" class="input input-sm" bind:value={fra} />
      <input class="input input-sm" placeholder="Månedslønn (kr)" bind:value={lonn} />
    </div>
    <div class="grid grid-cols-2 gap-2">
      <input
        class="input input-sm"
        placeholder="Trekkprosent (f.eks. 35)"
        bind:value={trekk}
      />
      <input
        class="input input-sm"
        title="Feriepengesats i prosent"
        bind:value={fp}
      />
    </div>
    <input
      class="input input-sm"
      placeholder="E-post — inviter til portalen og koble (valgfritt)"
      bind:value={epost}
    />
    <button class="btn btn-sm" onclick={opprett}>Registrer ansatt</button>
  </div>
</Card>
