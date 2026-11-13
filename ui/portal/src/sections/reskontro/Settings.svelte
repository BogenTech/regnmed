<script>
  // Which accounts are reskontro accounts. On a flagged account every
  // posting must carry a party of the matching kind, and on any other
  // account no posting may carry one (enforced in post_voucher). A new
  // company gets 1500/2400 flagged at onboarding; an opening balance
  // defers the flag on purpose, and this is where it is set.
  import { api, send } from "../../lib/api.js";
  import { kr } from "../../lib/format.js";
  import { toast } from "../../lib/toast.svelte.js";
  import { bekreft } from "../../lib/dialog.svelte.js";
  import { harRett } from "../../lib/me.svelte.js";

  let { companyId } = $props();

  let accounts = $state(null);
  let error = $state(null);
  let busy = $state(null);
  let canEdit = $derived(harRett(companyId, "RESKONTRO_SKRIV"));

  function load() {
    api("/companies/" + companyId + "/accounts")
      .then((r) => (accounts = r.kontoer))
      .catch((e) => (error = e.message));
  }
  $effect(load);

  // The usual places (kundefordringer 15xx, leverandørgjeld 24xx) and
  // anything already flagged elsewhere.
  let relevant = $derived(
    (accounts || []).filter((a) => a.reskontro_kind || /^(15|24)/.test(a.number)),
  );

  const LABEL = { "": "Ikke reskontro", kunde: "Kunder", leverandor: "Leverandører" };

  async function change(a, kind) {
    const was = a.reskontro_kind || "";
    if (kind === was) return;
    const text = kind
      ? "Fra nå må hver postering på " + a.number + " ha en " + (kind === "kunde" ? "kunde" : "leverandør") + "." +
        (a.saldo_ore ? " Kontoen har saldo " + kr(a.saldo_ore) + " kr uten part; den viser seg som avvik til den er erstattet av åpne poster (Import)." : "")
      : "Postering med part på " + a.number + " blir avvist fra nå. Poster som allerede har part, står som de er.";
    const ok = await bekreft(text, { tittel: a.number + " " + a.name + ": " + LABEL[kind] + "?", ok: "Lagre" });
    if (!ok) {
      accounts = [...accounts]; // put the select back
      return;
    }
    busy = a.number;
    try {
      await send("PUT", "/companies/" + companyId + "/accounts/" + a.number + "/reskontro", { kind: kind || null });
      toast(a.number + " er " + (kind ? "reskontrokonto for " + LABEL[kind].toLowerCase() : "ikke lenger reskontrokonto"), true);
      load();
    } catch (e) {
      toast(e.message, false);
      accounts = [...accounts];
    }
    busy = null;
  }
</script>

<section class="card border border-base-300 bg-base-100 max-w-3xl" aria-labelledby="kontoer-tittel">
  <div class="card-body gap-3">
    <h2 id="kontoer-tittel" class="card-title">Reskontrokontoer</h2>
    <p class="text-sm opacity-70">
      På en reskontrokonto må hver postering ha en kunde eller leverandør, og summen av partene skal være lik saldoen.
      Vanligvis er 1500 kundefordringer og 2400 leverandørgjeld.
    </p>
    {#if error}
      <div class="alert alert-error" role="alert"><span>{error}</span></div>
    {:else if accounts === null}
      <span class="loading loading-spinner"></span>
    {:else if relevant.length === 0}
      <p>Kontoplanen har ingen 15xx- eller 24xx-konto. Legg til 1500 og 2400 under Hovedbok først.</p>
    {:else}
      <ul class="grid gap-2">
        {#each relevant as a (a.number)}
          <li class="flex flex-wrap items-center gap-3">
            <span class="mr-auto">
              <span class="font-mono">{a.number}</span> {a.name}
              <span class="block text-sm opacity-70 tabular-nums">Saldo {kr(a.saldo_ore ?? 0)} kr</span>
            </span>
            {#if canEdit}
              <label class="sr-only" for={"res-" + a.number}>Reskontro for {a.number}</label>
              <select
                id={"res-" + a.number}
                class="select select-sm w-44"
                value={a.reskontro_kind || ""}
                disabled={busy === a.number}
                onchange={(e) => change(a, e.target.value)}
              >
                {#each Object.entries(LABEL) as [key, label] (key)}
                  <option value={key}>{label}</option>
                {/each}
              </select>
            {:else}
              <span class="badge badge-soft">{LABEL[a.reskontro_kind || ""]}</span>
            {/if}
          </li>
        {/each}
      </ul>
      <p class="text-sm"><a class="link" href={"#/c/" + companyId + "/reskontro"}>Se om reskontroen stemmer med hovedboken</a></p>
    {/if}
  </div>
</section>
