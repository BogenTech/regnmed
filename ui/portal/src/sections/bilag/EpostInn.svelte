<script>
  // E-post-inn (#35, docs/epost-inn.md): adressen er en KAPABILITET.
  // Ukjent avsender → KARANTENE, aldri stille import og aldri stille
  // forkasting — noen må bestemme, og valget står i loggen.
  import { api, post } from "../../lib/api.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema, sporsmal } from "../../lib/dialog.svelte.js";
  import Card from "../../components/Card.svelte";

  let { companyId, settings, mailLog, onDone } = $props();

  const BADGE = { mottatt: "badge-success", karantene: "badge-warning", avvist: "badge-ghost" };

  let karantene = $derived(mailLog.filter((m) => m.status === "karantene"));
  let nyAvsender = $state("");

  async function adresse() {
    if (
      settings.adresse &&
      !(await bekreft("Ny adresse gjør den gamle ubrukelig med én gang. Fortsette?", {
        tittel: "Ny mottaksadresse",
        ok: "Lag ny adresse",
        farlig: true,
      }))
    ) {
      return;
    }
    try {
      const r = await post("/companies/" + companyId + "/inbox/settings/address", {});
      toast("Mottaksadresse: " + (r.adresse || r.local_part), true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function leggTilAvsender() {
    const sender = nyAvsender.trim();
    if (!sender) return;
    try {
      await post("/companies/" + companyId + "/inbox/settings/senders", { sender });
      toast("Avsender lagt til", true);
      nyAvsender = "";
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function fjernAvsender(a) {
    try {
      await api("/companies/" + companyId + "/inbox/settings/senders/" + a.id, { method: "DELETE" });
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function slippInn(m) {
    // The old confirm() could not abort — OK/Avbryt only chose whether the
    // sender was allowlisted. The modal makes the three-way choice honest.
    const svar = await skjema(
      "Slipp inn e-posten",
      [{ navn: "tillat", etikett: "Legg avsenderen til på listen", type: "checkbox", standard: true }],
      { melding: "Vedleggene blir dokumenter i innboksen.", ok: "Slipp inn" },
    );
    if (!svar) return;
    try {
      const r = await post("/companies/" + companyId + "/inbox/mail/" + m.mail_id + "/release", {
        tillat_avsender: svar.tillat,
      });
      toast(r.dokumenter + " dokument(er) lagt i innboksen", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }

  async function avvisMail(m) {
    const note = await sporsmal("Hvorfor avvises e-posten?", {
      type: "textarea",
      ok: "Avvis",
      farlig: true,
    });
    if (!note) return;
    try {
      await post("/companies/" + companyId + "/inbox/mail/" + m.mail_id + "/reject", { note });
      toast("Avvist — e-posten står igjen i loggen", true);
      onDone();
    } catch (error) {
      toast(error.message, false);
    }
  }
</script>

<Card title="E-post-inn">
  {#if karantene.length}
    <span class="badge badge-warning badge-sm w-fit">{karantene.length} i karantene</span>
  {/if}
  <p class="text-sm opacity-70 mb-2">
    Leverandører kan sende bilag rett til innboksen. Vedlegg fra en avsender på listen blir
    dokumenter med én gang; alle andre havner i karantene til noen bestemmer — aldri stille
    importert, aldri stille forkastet.
  </p>
  {#if settings.adresse}
    <div class="flex gap-2 items-center flex-wrap mb-2">
      <code class="text-sm bg-base-200 px-2 py-1 rounded">{settings.adresse}</code>
      <button class="btn btn-xs btn-ghost" onclick={adresse}>Ny adresse</button>
    </div>
  {:else if settings.local_part}
    <p class="text-sm mb-2">
      <code>{settings.local_part}</code> — mottaksdomenet er ikke satt opp ennå (MAIL_IN_DOMAIN).
    </p>
  {:else}
    <button class="btn btn-sm btn-outline mb-2 w-fit" onclick={adresse}>
      Opprett mottaksadresse
    </button>
  {/if}
  {#if !settings.aktiv}
    <p class="text-xs opacity-60 mb-2">
      Mail-railen er ikke konfigurert i dette miljøet — adressen tar ikke imot noe før den er det.
    </p>
  {/if}
  {#if karantene.length}
    <h3 class="font-semibold text-sm mt-3 mb-1">Venter på avgjørelse</h3>
    <table class="table table-xs mb-2">
      <thead>
        <tr><th>Fra</th><th>Emne</th><th>Vedlegg</th><th>Mottatt</th><th></th></tr>
      </thead>
      <tbody>
        {#each karantene as m (m.mail_id)}
          <tr>
            <td>{m.fra}</td>
            <td>{m.emne || ""}</td>
            <td>{m.antall_vedlegg}</td>
            <td>{m.mottatt.slice(0, 16).replace("T", " ")}</td>
            <td>
              <button class="btn btn-xs btn-success" onclick={() => slippInn(m)}>Slipp inn</button>
              <button class="btn btn-xs btn-ghost" onclick={() => avvisMail(m)}>Avvis</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="flex gap-2 items-center flex-wrap mt-2">
    <input
      class="input input-sm w-56"
      placeholder="post@leverandor.no eller @leverandor.no"
      bind:value={nyAvsender}
    />
    <button class="btn btn-sm btn-ghost" onclick={leggTilAvsender}>+ avsender</button>
    {#if settings.avsendere.length}
      <div class="flex gap-1 flex-wrap">
        {#each settings.avsendere as a (a.id)}
          <span class="badge badge-outline gap-1">
            {a.sender}
            <button class="btn btn-xs btn-ghost px-1" onclick={() => fjernAvsender(a)}>×</button>
          </span>
        {/each}
      </div>
    {/if}
  </div>
  {#if mailLog.length}
    <div class="mt-3">
      {#each mailLog.slice(0, 8) as m (m.mail_id)}
        <div class="text-xs opacity-70 py-0.5">
          <span class="badge badge-xs {BADGE[m.status]}">{m.status}</span>
          {m.fra} — {m.emne || "(uten emne)"} ({m.antall_vedlegg} vedlegg){m.note
            ? " · " + m.note
            : ""}
        </div>
      {/each}
    </div>
  {/if}
</Card>
