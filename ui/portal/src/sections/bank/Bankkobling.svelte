<script module>
  // Returns from the bank that are already handled. Module level so a remount
  // (the section is keyed) can never complete the same authorisation twice.
  const behandlet = new Set();
</script>

<script>
  // Bankkobling: bank transactions fetched straight from the bank instead of from
  // a file. The user logs in AT THE BANK (BankID); regnmed only keeps the consent
  // reference, and the consent has an end date the card keeps in view. Without a
  // configured provider the card does not exist; without the add-on it only says so.
  import { api, post } from "../../lib/api.js";
  import { formatDate, formatTime, today } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft, skjema } from "../../lib/dialog.svelte.js";
  import { route } from "../../lib/router.svelte.js";
  import { bankvalg, koblingsstatus, maskert, retur } from "../../lib/bankkobling.js";

  let { companyId, accounts, query, canReconcile, onSynced } = $props();

  const base = $derived("/companies/" + companyId + "/bank/feed");
  const idag = today();

  let feed = $state(null); // null = loading or unavailable: nothing is drawn
  let busy = $state(null); // connection id, "ny" or "fullfor"

  function load() {
    api(base)
      .then((r) => (feed = r))
      .catch(() => (feed = null));
  }
  $effect(() => {
    load();
  });

  // The bank sent the browser back here. Take the parameters out of the address
  // first, so a reload or Back cannot replay them, then finish the authorisation.
  $effect(() => {
    const r = retur(query);
    if (!r) return;
    const key = r.tilstand ?? "feil:" + r.feil;
    if (behandlet.has(key)) return;
    behandlet.add(key);
    history.replaceState(history.state, "", location.pathname + location.search + "#/c/" + companyId + "/bank");
    route.query = "";
    if (r.feil) return toast(r.feil, false);
    busy = "fullfor";
    post(base + "/complete", { code: r.kode, state: r.tilstand })
      .then((k) => {
        toast("Banken er koblet til" + (k.gyldig_til ? ". Samtykket gjelder til " + formatDate(k.gyldig_til) : ""), true);
        onSynced?.();
      })
      .catch((e) => toast(e.message, false))
      .finally(() => {
        busy = null;
        load();
      });
  });

  async function kobleTil(kropp, id) {
    busy = id;
    try {
      const r = await post(base + "/connect", kropp);
      // Off to the bank; the user comes back to #/c/{id}/bank with the result.
      window.location.assign(r.url);
    } catch (e) {
      toast(e.message, false);
      busy = null;
    }
  }

  async function ny() {
    if (busy) return;
    busy = "ny";
    let banker;
    try {
      banker = (await api(base + "/banks")).banker ?? [];
    } catch (e) {
      busy = null;
      return toast(e.message, false);
    }
    busy = null;
    if (banker.length === 0) return toast("Fant ingen banker å koble til akkurat nå", false);
    const svar = await skjema(
      "Koble til bank",
      [
        {
          navn: "account_number",
          etikett: "Bankkonto i regnskapet",
          type: "select",
          standard: accounts[0]?.number,
          valg: accounts.map((a) => [a.number, a.number + " " + a.name]),
        },
        { navn: "bank", etikett: "Bank", type: "select", standard: "0", valg: bankvalg(banker) },
        {
          navn: "kontonummer",
          etikett: "Kontonummer i banken",
          valgfri: true,
          hjelp: "Tomt = selskapets kontonummer fra Firmaopplysninger.",
        },
      ],
      {
        ok: "Gå til banken",
        melding:
          "Du logger inn hos banken med BankID og godkjenner at regnmed får lese kontobevegelsene. regnmed ser aldri innloggingen din.",
      },
    );
    if (!svar) return;
    const kontonummer = (svar.kontonummer ?? "").replace(/\D/g, "");
    const valgt = banker[Number(svar.bank)];
    await kobleTil(
      {
        account_number: svar.account_number,
        bank: valgt.name,
        ...(valgt.provider ? { provider: valgt.provider } : {}),
        ...(kontonummer ? { kontonummer } : {}),
      },
      "ny",
    );
  }

  // Renewal is simply a new authorisation for the same account at the same bank.
  function forny(k) {
    if (busy) return;
    const kontonummer = (k.kontonummer ?? "").replace(/\D/g, "");
    kobleTil(
      {
        account_number: k.account_number,
        bank: k.bank,
        ...(k.leverandor ? { provider: k.leverandor } : {}),
        ...(kontonummer ? { kontonummer } : {}),
      },
      k.id,
    );
  }

  async function hent(k) {
    if (busy) return;
    busy = k.id;
    try {
      const r = await post(base + "/" + k.id + "/sync", {});
      toast(
        r.nye === 0
          ? "Ingen nye bankposter"
          : r.nye + (r.nye === 1 ? " ny bankpost hentet, " : " nye bankposter hentet, ") + r.auto_matchet + " koblet automatisk",
        true,
      );
      onSynced?.();
    } catch (e) {
      toast(e.message, false);
    }
    busy = null;
    load();
  }

  async function kobleFra(k) {
    if (busy) return;
    const ok = await bekreft(
      "Koble " + k.bank + " fra konto " + k.account_number + "? Bankposter som alt er hentet blir stående. Etterpå må kontoutskrift lastes opp som fil igjen.",
      { tittel: "Koble fra banken", ok: "Koble fra", farlig: true },
    );
    if (!ok) return;
    busy = k.id;
    try {
      await post(base + "/" + k.id + "/end", {});
      toast("Banken er koblet fra", true);
    } catch (e) {
      toast(e.message, false);
    }
    busy = null;
    load();
  }

  const synlige = $derived((feed?.koblinger ?? []).filter((k) => k.status !== "avsluttet"));
</script>

{#if feed?.konfigurert}
  <section class="card border border-base-300 bg-base-100 mb-6" aria-labelledby="bankkobling-tittel">
    <div class="card-body gap-3 overflow-x-auto">
      <div class="flex flex-wrap items-center gap-3">
        <h2 id="bankkobling-tittel" class="card-title">Bankkobling</h2>
        {#if busy === "fullfor"}
          <span class="loading loading-spinner loading-sm" aria-label="Fullfører koblingen til banken"></span>
        {/if}
        {#if feed.tillegg && canReconcile}
          <button class="btn btn-sm btn-primary ml-auto" onclick={ny} disabled={!!busy}>
            {#if busy === "ny"}<span class="loading loading-spinner loading-xs"></span>{/if}
            Koble til bank
          </button>
        {/if}
      </div>

      {#if !feed.tillegg}
        <p>
          Direkte bankkobling er en tilleggstjeneste som ikke er aktiv for dette selskapet. Se
          <a class="link" href={"#/c/" + companyId + "/admin/abonnement"}>Administrasjon → Abonnement</a>.
        </p>
      {:else}
        <p class="text-sm opacity-80">
          Bankpostene hentes rett fra banken. Du logger inn hos banken med BankID, regnmed ser aldri innloggingen, og
          samtykket må fornyes innen datoen som står på koblingen.
        </p>
        {#if synlige.length === 0}
          <p>Ingen bank er koblet til ennå.</p>
        {:else}
          <table class="table table-sm">
            <thead>
              <tr><th>Bank</th><th>Konto i regnskapet</th><th>Status</th><th>Samtykke til</th><th>Sist hentet</th><th></th></tr>
            </thead>
            <tbody>
              {#each synlige as k (k.id)}
                {@const s = koblingsstatus(k, idag)}
                <tr>
                  <td>
                    <div class="font-medium">{k.bank}</div>
                    {#if k.kontonummer}<div class="text-xs opacity-60 tabular-nums">{maskert(k.kontonummer)}</div>{/if}
                  </td>
                  <td class="tabular-nums">{k.account_number}</td>
                  <td>
                    <span class="badge badge-sm {s.klasse}">{s.tekst}</span>
                    {#if k.siste_feil}<div class="text-xs text-error mt-1" role="status">{k.siste_feil}</div>{/if}
                  </td>
                  <td class="tabular-nums">{k.gyldig_til ? formatDate(k.gyldig_til) : "–"}</td>
                  <td class="tabular-nums">{k.sist_hentet ? formatTime(k.sist_hentet) : "–"}</td>
                  <td class="text-right whitespace-nowrap">
                    {#if canReconcile}
                      {#if s.forny}
                        <button class="btn btn-xs btn-warning" onclick={() => forny(k)} disabled={!!busy}>Forny samtykke</button>
                      {/if}
                      {#if k.status === "aktiv"}
                        <button class="btn btn-xs" onclick={() => hent(k)} disabled={!!busy}>
                          {#if busy === k.id}<span class="loading loading-spinner loading-xs"></span>{/if}
                          Hent nå
                        </button>
                      {/if}
                      <button class="btn btn-xs btn-ghost" onclick={() => kobleFra(k)} disabled={!!busy}>Koble fra</button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}
    </div>
  </section>
{/if}
