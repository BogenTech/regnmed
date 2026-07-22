// regnmed portal — a deliberately frugal single-page app: no framework,
// no build step (Tailwind builds only the CSS). Served by regnmed-api on
// the same origin, so API calls need no CORS. Auth: OIDC authorization
// code + PKCE against regnid; the token exchange is proxied through
// /auth/token (same origin) so the IdP needs no browser CORS.
(function () {
  "use strict";

  var app = document.getElementById("app");
  var config = null; // {issuer, client_id}
  var me = null;     // /me payload
  var companies = [];

  // ---------- small utilities ----------

  function esc(s) {
    return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function kr(ore) {
    var negative = ore < 0;
    var abs = Math.abs(ore);
    var whole = Math.floor(abs / 100).toString().replace(/\B(?=(\d{3})+(?!\d))/g, " ");
    return (negative ? "−" : "") + whole + "," + String(abs % 100).padStart(2, "0");
  }
  function parseKr(text) {
    var cleaned = String(text).replace(/[\s ]/g, "").replace(",", ".");
    var value = Number(cleaned);
    if (!isFinite(value)) throw new Error("ugyldig beløp: " + text);
    return Math.round(value * 100);
  }
  function today() {
    return new Date().toISOString().slice(0, 10);
  }
  function toast(message, ok) {
    var el = document.createElement("div");
    el.className = "toast toast-top toast-end z-50";
    el.innerHTML = '<div class="alert ' + (ok ? "alert-success" : "alert-error") +
      ' shadow"><span>' + esc(message) + "</span></div>";
    document.body.appendChild(el);
    setTimeout(function () { el.remove(); }, 5000);
  }

  // ---------- auth (code + PKCE) ----------

  function tokens() {
    try { return JSON.parse(sessionStorage.getItem("regnmed-tokens")); } catch (e) { return null; }
  }
  function saveTokens(t) { sessionStorage.setItem("regnmed-tokens", JSON.stringify(t)); }
  function clearTokens() { sessionStorage.removeItem("regnmed-tokens"); }

  function b64url(bytes) {
    return btoa(String.fromCharCode.apply(null, new Uint8Array(bytes)))
      .replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }
  function randomString() {
    var bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    return b64url(bytes);
  }

  async function login() {
    var verifier = randomString();
    var state = randomString();
    var challenge = b64url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
    sessionStorage.setItem("regnmed-pkce", JSON.stringify({ verifier: verifier, state: state }));
    var url = config.issuer.replace(/\/$/, "") + "/authorize" +
      "?response_type=code&client_id=" + encodeURIComponent(config.client_id) +
      "&redirect_uri=" + encodeURIComponent(location.origin + "/callback") +
      "&scope=" + encodeURIComponent("openid profile email") +
      "&state=" + state + "&nonce=" + randomString() +
      "&code_challenge=" + challenge + "&code_challenge_method=S256";
    location.assign(url);
  }

  async function handleCallback() {
    var params = new URLSearchParams(location.search);
    var pkce = JSON.parse(sessionStorage.getItem("regnmed-pkce") || "null");
    sessionStorage.removeItem("regnmed-pkce");
    if (!params.get("code") || !pkce || params.get("state") !== pkce.state) {
      throw new Error("ugyldig innloggingssvar");
    }
    var response = await fetch("/auth/token", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        code: params.get("code"),
        code_verifier: pkce.verifier,
        redirect_uri: location.origin + "/callback",
      }),
    });
    if (!response.ok) throw new Error("innlogging feilet (" + response.status + ")");
    saveTokens(await response.json());
    history.replaceState(null, "", "/");
  }

  function logout() {
    var t = tokens();
    clearTokens();
    var end = config.issuer.replace(/\/$/, "") + "/end_session";
    if (t && t.id_token) end += "?id_token_hint=" + encodeURIComponent(t.id_token);
    location.assign(end);
  }

  async function api(path, options) {
    options = options || {};
    var t = tokens();
    if (!t) { renderLogin(); throw new Error("ikke innlogget"); }
    options.headers = Object.assign(
      { authorization: "Bearer " + t.access_token },
      options.headers || {}
    );
    var response = await fetch(path, options);
    if (response.status === 401) { clearTokens(); renderLogin(); throw new Error("utløpt økt"); }
    if (!response.ok) {
      var detail = "";
      try { detail = (await response.json()).error || ""; } catch (e) { /* not json */ }
      throw new Error(detail || "feil (" + response.status + ")");
    }
    var type = response.headers.get("content-type") || "";
    return type.includes("application/json") ? response.json() : response;
  }
  function post(path, body) {
    return api(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  }

  // ---------- layout ----------

  function themeControls() {
    var options = window.regnmedTheme.THEMES.map(function (t) {
      return '<option value="' + t + '">' + t + "</option>";
    }).join("");
    return '<select id="theme-picker" class="select select-sm select-bordered w-32">' +
      options + "</select>" +
      '<button id="theme-toggle" class="btn btn-ghost btn-sm text-lg"></button>';
  }

  function shell(companyId, section, content) {
    var company = companies.find(function (c) { return c.company_id === companyId; });
    var items = [
      ["oversikt", "Oversikt"], ["faktura", "Faktura"], ["reskontro", "Reskontro"],
      ["mva", "Mva"], ["bank", "Bank"], ["bilag", "Bilag"], ["periode", "Periode"],
    ].map(function (item) {
      return '<li><a href="#/c/' + companyId + "/" + item[0] + '" class="' +
        (section === item[0] ? "active" : "") + '">' + item[1] + "</a></li>";
    }).join("");
    app.innerHTML =
      '<div class="navbar bg-base-100 shadow-sm">' +
      '<div class="flex-1 gap-2 items-baseline"><a href="#/" class="btn btn-ghost text-xl">regnmed</a>' +
      '<span class="text-sm opacity-70">' + esc(company ? company.name : "") + "</span></div>" +
      '<div class="flex-none gap-2">' + themeControls() +
      '<button id="logout" class="btn btn-ghost btn-sm">Logg ut</button></div></div>' +
      '<div class="flex"><ul class="menu bg-base-100 w-44 min-h-full">' + items + "</ul>" +
      '<main class="flex-1 p-6 max-w-5xl">' + content + "</main></div>";
    wireChrome();
  }

  function wireChrome() {
    window.regnmedTheme.render();
    var picker = document.getElementById("theme-picker");
    if (picker) picker.onchange = function () { window.regnmedTheme.set(picker.value); };
    var toggle = document.getElementById("theme-toggle");
    if (toggle) toggle.onclick = function () { window.regnmedTheme.cycle(); };
    var logoutBtn = document.getElementById("logout");
    if (logoutBtn) logoutBtn.onclick = logout;
  }

  function card(title, body) {
    return '<div class="card bg-base-100 shadow-sm mb-6"><div class="card-body">' +
      '<h2 class="card-title">' + title + "</h2>" + body + "</div></div>";
  }

  // ---------- views ----------

  function renderLogin() {
    app.innerHTML =
      '<div class="min-h-screen flex items-center justify-center">' +
      '<div class="card bg-base-100 shadow-sm w-full max-w-sm"><div class="card-body items-center">' +
      '<h1 class="card-title text-2xl">regnmed</h1>' +
      '<p class="opacity-70 text-sm mb-2">Regnskap du kan etterprøve.</p>' +
      '<button id="login" class="btn btn-primary w-full">Logg inn</button>' +
      '<div class="mt-2 flex gap-2 items-center">' + themeControls() + "</div>" +
      "</div></div></div>";
    document.getElementById("login").onclick = login;
    wireChrome();
  }

  async function renderCompanies() {
    me = await api("/me");
    companies = me.companies;
    var rows = companies.map(function (c) {
      return '<a href="#/c/' + c.company_id + '/oversikt" class="card bg-base-100 shadow-sm hover:shadow-md transition-shadow">' +
        '<div class="card-body"><h2 class="card-title">' + esc(c.name) + "</h2>" +
        '<p class="text-sm opacity-70">' + esc(c.orgnr) + " · " + esc(c.access) +
        " via " + esc(c.via) + "</p></div></a>";
    }).join("");
    app.innerHTML =
      '<div class="navbar bg-base-100 shadow-sm">' +
      '<div class="flex-1"><span class="btn btn-ghost text-xl">regnmed</span></div>' +
      '<div class="flex-none gap-2">' + themeControls() +
      '<button id="logout" class="btn btn-ghost btn-sm">Logg ut</button></div></div>' +
      '<main class="p-6 max-w-3xl mx-auto">' +
      '<h1 class="text-lg mb-4">Hei, ' + esc(me.name || me.email || "") + " — velg selskap:</h1>" +
      '<div class="grid gap-4 sm:grid-cols-2">' +
      (rows || '<p class="opacity-70">Ingen selskaper ennå — be om tilgang eller et oppdrag.</p>') +
      "</div></main>";
    wireChrome();
  }

  async function renderOversikt(id) {
    var year = new Date().getFullYear();
    var termin = Math.floor(new Date().getMonth() / 2) + 1;
    var results = await Promise.all([
      api("/companies/" + id + "/invoices?open=true"),
      api("/companies/" + id + "/reports/mva?year=" + year + "&termin=" + termin).catch(function () { return null; }),
      api("/companies/" + id + "/period-lock"),
      api("/companies/" + id + "/vouchers"),
    ]);
    var open = results[0].invoices;
    var mva = results[1];
    var lock = results[2];
    var vouchers = results[3].vouchers;
    var openSum = open.reduce(function (sum, i) { return sum + i.remaining_ore; }, 0);
    var stats =
      '<div class="stats shadow-sm bg-base-100 w-full mb-6">' +
      '<div class="stat"><div class="stat-title">Utestående fakturaer</div>' +
      '<div class="stat-value text-2xl">' + kr(openSum) + '</div>' +
      '<div class="stat-desc">' + open.length + " åpne</div></div>" +
      '<div class="stat"><div class="stat-title">Mva ' + termin + ". termin</div>" +
      '<div class="stat-value text-2xl">' + (mva ? kr(mva.netto_ore) : "–") + "</div>" +
      '<div class="stat-desc">' + (mva && mva.netto_ore >= 0 ? "å betale" : "til gode") + "</div></div>" +
      '<div class="stat"><div class="stat-title">Periode låst t.o.m.</div>' +
      '<div class="stat-value text-2xl">' + esc(lock.locked_through || "åpen") + "</div></div></div>";
    var recent = vouchers.slice(0, 8).map(function (v) {
      return "<tr><td>" + esc(v.voucher) + "</td><td>" + esc(v.date) + "</td><td>" +
        esc(v.description) + "</td></tr>";
    }).join("");
    shell(id, "oversikt", stats + card("Siste bilag",
      '<table class="table table-sm"><thead><tr><th>Bilag</th><th>Dato</th><th>Tekst</th></tr></thead>' +
      "<tbody>" + recent + "</tbody></table>"));
  }

  async function renderFaktura(id) {
    var results = await Promise.all([
      api("/companies/" + id + "/invoices"),
      api("/companies/" + id + "/parties?kind=kunde"),
    ]);
    var invoices = results[0].invoices;
    var parties = results[1].parties;
    var rows = invoices.map(function (i) {
      var action = !i.is_credit_note && i.remaining_ore !== 0
        ? '<button class="btn btn-xs btn-outline" data-credit="' + i.invoice_id + '">Kreditnota</button>'
        : "";
      return "<tr><td>" + i.invoice_no + "</td><td>" + esc(i.party_name) + "</td><td>" +
        esc(i.invoice_date) + "</td><td class='font-mono'>" + esc(i.kid) + "</td>" +
        "<td class='text-right'>" + kr(i.gross_ore) + "</td>" +
        "<td class='text-right'>" + kr(i.remaining_ore) + "</td><td>" + action + "</td></tr>";
    }).join("");
    var partyOptions = parties.map(function (p) {
      return '<option value="' + esc(p.party_no) + '">' + esc(p.party_no) + " " + esc(p.name) + "</option>";
    }).join("");
    var form = parties.length === 0
      ? '<p class="opacity-70">Opprett en kunde under Reskontro først.</p>'
      : '<form id="new-invoice" class="grid gap-2 max-w-md">' +
        '<select name="party_no" class="select select-bordered">' + partyOptions + "</select>" +
        '<div class="grid grid-cols-2 gap-2">' +
        '<label class="form-control"><span class="label-text">Fakturadato</span>' +
        '<input name="invoice_date" type="date" class="input input-bordered" value="' + today() + '"></label>' +
        '<label class="form-control"><span class="label-text">Forfall</span>' +
        '<input name="due_date" type="date" class="input input-bordered" value="' + today() + '"></label></div>' +
        '<input name="description" class="input input-bordered" placeholder="Beskrivelse" required>' +
        '<div class="grid grid-cols-3 gap-2">' +
        '<input name="quantity" class="input input-bordered" value="1" title="Antall">' +
        '<input name="unit_price" class="input input-bordered" placeholder="Pris (kr)" required>' +
        '<select name="vat_code" class="select select-bordered">' +
        '<option value="3">3 — 25 %</option><option value="31">31 — 15 %</option>' +
        '<option value="33">33 — 12 %</option><option value="5">5 — fritatt</option>' +
        '<option value="6">6 — utenfor</option></select></div>' +
        '<button class="btn btn-primary">Opprett faktura</button></form>';
    shell(id, "faktura",
      card("Ny faktura", form) +
      card("Fakturaer",
        '<table class="table table-sm"><thead><tr><th>Nr</th><th>Kunde</th><th>Dato</th>' +
        "<th>KID</th><th class='text-right'>Beløp</th><th class='text-right'>Utestående</th><th></th></tr></thead>" +
        "<tbody>" + rows + "</tbody></table>"));

    var form_ = document.getElementById("new-invoice");
    if (form_) form_.onsubmit = async function (event) {
      event.preventDefault();
      var d = new FormData(form_);
      try {
        var issued = await post("/companies/" + id + "/invoices", {
          party_no: d.get("party_no"),
          invoice_date: d.get("invoice_date"),
          due_date: d.get("due_date"),
          lines: [{
            description: d.get("description"),
            quantity_milli: Math.round(Number(String(d.get("quantity")).replace(",", ".")) * 1000),
            unit_price_ore: parseKr(d.get("unit_price")),
            vat_code: d.get("vat_code"),
          }],
        });
        toast("Faktura " + issued.invoice_no + " opprettet (KID " + issued.kid + ")", true);
        renderFaktura(id);
      } catch (error) { toast(error.message, false); }
    };
    app.querySelectorAll("[data-credit]").forEach(function (button) {
      button.onclick = async function () {
        if (!confirm("Opprette kreditnota for hele fakturaen?")) return;
        try {
          await post("/companies/" + id + "/invoices/" + button.dataset.credit + "/credit-note", {});
          toast("Kreditnota opprettet", true);
          renderFaktura(id);
        } catch (error) { toast(error.message, false); }
      };
    });
  }

  async function renderReskontro(id, partyId) {
    var parties = (await api("/companies/" + id + "/parties")).parties;
    if (partyId) {
      var party = parties.find(function (p) { return p.party_id === partyId; });
      var items = (await api("/companies/" + id + "/parties/" + partyId + "/items")).items;
      var rows = items.map(function (i) {
        return "<tr><td>" + esc(i.voucher) + "</td><td>" + esc(i.date) + "</td><td>" +
          esc(i.description || "") + "</td><td class='text-right'>" + kr(i.amount_ore) +
          "</td><td class='text-right'>" + kr(i.remaining_ore) + "</td></tr>";
      }).join("");
      shell(id, "reskontro", card(esc(party ? party.name : "") +
        ' <a href="#/c/' + id + '/reskontro" class="btn btn-ghost btn-xs">tilbake</a>',
        '<table class="table table-sm"><thead><tr><th>Bilag</th><th>Dato</th><th>Tekst</th>' +
        "<th class='text-right'>Beløp</th><th class='text-right'>Åpent</th></tr></thead>" +
        "<tbody>" + rows + "</tbody></table>"));
      return;
    }
    var rows = parties.map(function (p) {
      return '<tr><td>' + esc(p.party_no) + '</td><td><a class="link" href="#/c/' + id +
        "/reskontro/" + p.party_id + '">' + esc(p.name) + "</a></td><td>" + esc(p.kind) +
        "</td><td class='text-right'>" + kr(p.saldo_ore) + "</td></tr>";
    }).join("");
    shell(id, "reskontro",
      card("Ny part",
        '<form id="new-party" class="flex flex-wrap gap-2 items-end">' +
        '<select name="kind" class="select select-bordered"><option value="kunde">kunde</option>' +
        '<option value="leverandor">leverandør</option></select>' +
        '<input name="name" class="input input-bordered" placeholder="Navn" required>' +
        '<input name="orgnr" class="input input-bordered w-32" placeholder="Orgnr (valgfritt)">' +
        '<button class="btn btn-primary">Opprett</button></form>') +
      card("Kunde- og leverandørspesifikasjon",
        '<table class="table table-sm"><thead><tr><th>Nr</th><th>Navn</th><th>Type</th>' +
        "<th class='text-right'>Saldo</th></tr></thead><tbody>" + rows + "</tbody></table>"));
    document.getElementById("new-party").onsubmit = async function (event) {
      event.preventDefault();
      var d = new FormData(event.target);
      try {
        var created = await post("/companies/" + id + "/parties", {
          kind: d.get("kind"), name: d.get("name"),
          orgnr: d.get("orgnr") || null,
        });
        toast("Part " + created.party_no + " opprettet", true);
        renderReskontro(id);
      } catch (error) { toast(error.message, false); }
    };
  }

  async function renderMva(id) {
    var year = new Date().getFullYear();
    var termin = Math.floor(new Date().getMonth() / 2) + 1;
    var hash = location.hash.split("?")[1];
    if (hash) {
      var params = new URLSearchParams(hash);
      year = Number(params.get("year") || year);
      termin = Number(params.get("termin") || termin);
    }
    var report = null;
    try { report = await api("/companies/" + id + "/reports/mva?year=" + year + "&termin=" + termin); }
    catch (e) { /* none */ }
    var lines = report ? report.lines.map(function (l) {
      return "<tr><td>" + esc(l.code) + "</td><td>" + esc(l.description) +
        "</td><td class='text-right'>" + kr(l.grunnlag_ore) + "</td><td class='text-right'>" +
        kr(l.avgift_ore) + "</td></tr>";
    }).join("") : "";
    var terminButtons = [1, 2, 3, 4, 5, 6].map(function (t) {
      return '<a class="join-item btn btn-sm ' + (t === termin ? "btn-primary" : "") +
        '" href="#/c/' + id + "/mva?year=" + year + "&termin=" + t + '">' + t + "</a>";
    }).join("");
    shell(id, "mva",
      card("Mva-spesifikasjon " + termin + ". termin " + year,
        '<div class="join mb-4">' + terminButtons + "</div>" +
        (report
          ? '<table class="table table-sm"><thead><tr><th>Kode</th><th>Beskrivelse</th>' +
            "<th class='text-right'>Grunnlag</th><th class='text-right'>Avgift</th></tr></thead>" +
            "<tbody>" + lines + "</tbody></table>" +
            '<div class="stats bg-base-200 mt-4"><div class="stat"><div class="stat-title">Utgående</div>' +
            '<div class="stat-value text-lg">' + kr(report.utgaende_ore) + "</div></div>" +
            '<div class="stat"><div class="stat-title">Inngående</div><div class="stat-value text-lg">' +
            kr(report.inngaende_ore) + "</div></div>" +
            '<div class="stat"><div class="stat-title">' + (report.netto_ore >= 0 ? "Å betale" : "Til gode") +
            '</div><div class="stat-value text-lg">' + kr(Math.abs(report.netto_ore)) + "</div></div></div>"
          : '<p class="opacity-70">Ingen mva-posteringer i terminen.</p>')) +
      card("Eksport",
        '<div class="flex gap-2 flex-wrap">' +
        '<a class="btn btn-outline" id="dl-melding">Mva-melding (XML)</a>' +
        '<a class="btn btn-outline" id="dl-saft">SAF-T ' + year + " (XML)</a></div>" +
        '<p class="text-sm opacity-70 mt-2">Filene genereres direkte fra hovedboken og validerer mot Skatteetatens skjema.</p>'));
    function download(path, filename) {
      return async function () {
        try {
          var response = await api(path);
          var blob = await response.blob();
          var a = document.createElement("a");
          a.href = URL.createObjectURL(blob);
          a.download = filename;
          a.click();
          URL.revokeObjectURL(a.href);
        } catch (error) { toast(error.message, false); }
      };
    }
    document.getElementById("dl-melding").onclick =
      download("/companies/" + id + "/reports/mva-melding?year=" + year + "&termin=" + termin,
        "mva-melding-" + year + "-termin" + termin + ".xml");
    document.getElementById("dl-saft").onclick =
      download("/companies/" + id + "/reports/saft?year=" + year, "saf-t-" + year + ".xml");
  }

  async function renderBank(id) {
    var account = "1920";
    var recon = null;
    try { recon = await api("/companies/" + id + "/bank/reconciliation?account=" + account); }
    catch (e) { /* no statements yet */ }
    var entryOptions = recon ? recon.unmatched_entries.map(function (e) {
      return '<option value="' + e.entry_id + '">' + esc(e.voucher) + " " + esc(e.date) +
        " " + kr(e.amount_ore) + "</option>";
    }).join("") : "";
    var unmatched = recon ? recon.unmatched_bank.map(function (t) {
      return "<tr><td>" + esc(t.booking_date) + "</td><td>" + esc(t.description) +
        "</td><td class='text-right'>" + kr(t.amount_ore) + "</td>" +
        '<td class="flex gap-1"><select class="select select-xs select-bordered" data-entry-for="' +
        t.bank_transaction_id + '">' + entryOptions + "</select>" +
        '<button class="btn btn-xs" data-match="' + t.bank_transaction_id + '">Koble</button></td></tr>';
    }).join("") : "";
    shell(id, "bank",
      card("Kontoutskrift (camt.053)",
        '<input type="file" id="camt-file" class="file-input file-input-bordered" accept=".xml">' +
        '<p class="text-sm opacity-70 mt-2">Last ned fra nettbanken og last opp her — konto ' + account + ".</p>") +
      (recon
        ? card("Avstemming " + esc(recon.account),
            '<div class="stats bg-base-200 mb-4"><div class="stat"><div class="stat-title">Hovedbok</div>' +
            '<div class="stat-value text-lg">' + kr(recon.ledger_balance_ore) + "</div></div>" +
            '<div class="stat"><div class="stat-title">Bank (' + esc(recon.statement_to_date || "") + ")</div>" +
            '<div class="stat-value text-lg">' + (recon.statement_closing_ore != null ? kr(recon.statement_closing_ore) : "–") +
            "</div></div>" +
            '<div class="stat"><div class="stat-title">Koblet</div><div class="stat-value text-lg">' +
            recon.matched_count + "</div></div></div>" +
            "<h3 class='font-semibold mb-2'>Ukoblede banktransaksjoner</h3>" +
            '<table class="table table-sm"><tbody>' + (unmatched || "<tr><td class='opacity-70'>Ingen — avstemt!</td></tr>") +
            "</tbody></table>")
        : card("Avstemming", '<p class="opacity-70">Ingen kontoutskrifter importert ennå.</p>')));
    document.getElementById("camt-file").onchange = async function (event) {
      var file = event.target.files[0];
      if (!file) return;
      try {
        var result = await api("/companies/" + id + "/bank/statements?account=" + account, {
          method: "POST", body: await file.text(),
        });
        toast(result.transactions + " transaksjoner, " + result.auto_matched + " koblet automatisk", true);
        renderBank(id);
      } catch (error) { toast(error.message, false); }
    };
    app.querySelectorAll("[data-match]").forEach(function (button) {
      button.onclick = async function () {
        var select = app.querySelector('[data-entry-for="' + button.dataset.match + '"]');
        try {
          await post("/companies/" + id + "/bank/matches", {
            bank_transaction_id: button.dataset.match, entry_id: select.value,
          });
          toast("Koblet", true);
          renderBank(id);
        } catch (error) { toast(error.message, false); }
      };
    });
  }

  async function renderBilag(id) {
    var vouchers = (await api("/companies/" + id + "/vouchers")).vouchers;
    var rows = vouchers.map(function (v) {
      return "<tr><td>" + esc(v.voucher) + "</td><td>" + esc(v.date) + "</td><td>" +
        esc(v.description) + '</td><td><label class="btn btn-xs btn-outline">Vedlegg' +
        '<input type="file" class="hidden" data-attach="' + v.voucher_id + '"></label> ' +
        '<button class="btn btn-xs btn-ghost" data-list="' + v.voucher_id + '">Vis</button></td></tr>';
    }).join("");
    shell(id, "bilag", card("Bilag",
      '<table class="table table-sm"><thead><tr><th>Bilag</th><th>Dato</th><th>Tekst</th><th></th></tr></thead>' +
      "<tbody>" + rows + "</tbody></table><div id='attachment-list' class='mt-4'></div>"));
    app.querySelectorAll("[data-attach]").forEach(function (input) {
      input.onchange = async function () {
        var file = input.files[0];
        if (!file) return;
        try {
          var uploaded = await api("/companies/" + id + "/vouchers/" + input.dataset.attach +
            "/attachments?filename=" + encodeURIComponent(file.name), {
            method: "POST",
            headers: { "content-type": file.type || "application/octet-stream" },
            body: file,
          });
          toast("Vedlegg lagret (sha256 " + uploaded.sha256.slice(0, 12) + "…)", true);
        } catch (error) { toast(error.message, false); }
      };
    });
    app.querySelectorAll("[data-list]").forEach(function (button) {
      button.onclick = async function () {
        var listing = await api("/companies/" + id + "/vouchers/" + button.dataset.list + "/attachments");
        var target = document.getElementById("attachment-list");
        target.innerHTML = listing.attachments.length
          ? listing.attachments.map(function (a) {
              return '<div class="flex gap-2 items-center text-sm py-1">' +
                '<a class="link" href="/companies/' + id + "/attachments/" + a.attachment_id +
                '" data-download="' + a.attachment_id + '">' + esc(a.filename) + "</a>" +
                '<span class="opacity-60">' + a.byte_size + ' B · sha256 <span class="font-mono">' +
                esc(a.sha256.slice(0, 16)) + "…</span> · " + esc(a.uploaded_by) + "</span></div>";
            }).join("")
          : '<p class="opacity-70 text-sm">Ingen vedlegg på bilaget.</p>';
        target.querySelectorAll("[data-download]").forEach(function (link) {
          link.onclick = async function (event) {
            event.preventDefault();
            var response = await api(link.getAttribute("href"));
            var blob = await response.blob();
            var a = document.createElement("a");
            a.href = URL.createObjectURL(blob);
            a.download = link.textContent;
            a.click();
            URL.revokeObjectURL(a.href);
          };
        });
      };
    });
  }

  async function renderPeriode(id) {
    var lock = await api("/companies/" + id + "/period-lock");
    var history = lock.history.map(function (h) {
      return "<tr><td>" + esc(h.locked_through) + "</td><td>" + esc(h.set_by) +
        "</td><td>" + esc(h.at.slice(0, 16).replace("T", " ")) + "</td></tr>";
    }).join("");
    shell(id, "periode",
      card("Periodelåsing",
        '<p class="mb-2">Låst til og med: <strong>' + esc(lock.locked_through || "ingen lås") + "</strong></p>" +
        '<form id="lock-form" class="flex gap-2 items-end">' +
        '<input type="date" name="locked_through" class="input input-bordered" required>' +
        '<button class="btn btn-primary">Lås periode</button></form>' +
        '<p class="text-sm opacity-70 mt-2">Bilag datert i låst periode avvises; rettelser føres i åpen periode. Gjenåpning krever admin og logges.</p>') +
      card("Historikk",
        '<table class="table table-sm"><thead><tr><th>Låst t.o.m.</th><th>Av</th><th>Når</th></tr></thead>' +
        "<tbody>" + history + "</tbody></table>"));
    document.getElementById("lock-form").onsubmit = async function (event) {
      event.preventDefault();
      var d = new FormData(event.target);
      try {
        await api("/companies/" + id + "/period-lock", {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ locked_through: d.get("locked_through") }),
        });
        toast("Periode låst", true);
        renderPeriode(id);
      } catch (error) { toast(error.message, false); }
    };
  }

  // ---------- router ----------

  async function route() {
    if (!tokens()) return renderLogin();
    try {
      if (!companies.length) { me = await api("/me"); companies = me.companies; }
      var parts = location.hash.replace(/^#\/?/, "").split("?")[0].split("/");
      if (parts[0] === "c" && parts[1]) {
        var id = parts[1];
        var section = parts[2] || "oversikt";
        if (section === "oversikt") return await renderOversikt(id);
        if (section === "faktura") return await renderFaktura(id);
        if (section === "reskontro") return await renderReskontro(id, parts[3] || null);
        if (section === "mva") return await renderMva(id);
        if (section === "bank") return await renderBank(id);
        if (section === "bilag") return await renderBilag(id);
        if (section === "periode") return await renderPeriode(id);
      }
      return await renderCompanies();
    } catch (error) {
      if (error.message !== "ikke innlogget" && error.message !== "utløpt økt") {
        toast(error.message, false);
      }
    }
  }

  // ---------- boot ----------

  (async function boot() {
    config = await (await fetch("/portal-config")).json();
    if (location.pathname === "/callback") {
      try { await handleCallback(); } catch (error) {
        app.innerHTML = '<div class="p-8"><div class="alert alert-error">' + esc(error.message) +
          '</div><a class="btn mt-4" href="/">Til forsiden</a></div>';
        return;
      }
    }
    window.addEventListener("hashchange", route);
    route();
  })();
})();
