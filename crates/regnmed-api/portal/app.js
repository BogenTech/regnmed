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
      ["mva", "Mva"], ["rapporter", "Rapporter"], ["bank", "Bank"], ["bilag", "Bilag"],
      ["periode", "Periode"], ["oppdrag", "Oppdrag"],
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
      (rows || '<p class="opacity-70">Ingen selskaper ennå — be om tilgang, et oppdrag, eller start under.</p>') +
      "</div>" +
      '<div class="card bg-base-100 shadow-sm mt-8"><div class="card-body">' +
      '<h2 class="card-title">Nytt selskap fra Enhetsregisteret</h2>' +
      '<div class="flex gap-2"><input id="onboard-orgnr" class="input input-bordered" ' +
      'placeholder="Organisasjonsnummer" maxlength="9">' +
      '<button id="onboard-lookup" class="btn">Slå opp</button></div>' +
      '<div id="onboard-preview"></div></div></div>' +
      '<div id="my-firms"></div></main>';
    wireChrome();
    api("/firms/mine").then(function (mine) {
      if (!mine.firms.length) return;
      document.getElementById("my-firms").innerHTML =
        '<h2 class="text-lg mt-8 mb-4">Mine byråer</h2><div class="grid gap-4 sm:grid-cols-2">' +
        mine.firms.map(function (f) {
          return '<a href="#/byra/' + f.firm_id + '" class="card bg-base-100 shadow-sm hover:shadow-md">' +
            '<div class="card-body"><h2 class="card-title">' + esc(f.name) +
            (f.pending_requests > 0 ? ' <span class="badge badge-primary">' + f.pending_requests + " nye</span>" : "") +
            "</h2><p class='text-sm opacity-70'>" + esc(f.kind) +
            (f.verified ? " · autorisert" : "") + "</p></div></a>";
        }).join("") + "</div>";
    }).catch(function () { /* section is optional */ });
    document.getElementById("onboard-lookup").onclick = async function () {
      var orgnr = document.getElementById("onboard-orgnr").value.trim();
      var target = document.getElementById("onboard-preview");
      try {
        var facts = await api("/registry/enheter/" + encodeURIComponent(orgnr));
        target.innerHTML =
          '<div class="mt-4 p-4 bg-base-200 rounded-box">' +
          '<p class="font-semibold">' + esc(facts.navn) + " (" + esc(facts.organisasjonsform || "") + ")</p>" +
          '<p class="text-sm opacity-70">' + esc(facts.naeringskode || "") + "</p>" +
          '<div class="flex gap-2 mt-2">' +
          (facts.mva_registrert ? '<span class="badge badge-ghost">MVA-registrert</span>' : "") +
          (facts.autorisasjon.regnskap ? '<span class="badge badge-success">Autorisert regnskapsførerselskap</span>' : "") +
          (facts.autorisasjon.revisjon ? '<span class="badge badge-success">Autorisert revisjonsselskap</span>' : "") +
          (facts.konkurs ? '<span class="badge badge-error">Konkurs</span>' : "") +
          "</div>" +
          '<button id="onboard-create" class="btn btn-primary btn-sm mt-4">Opprett selskap</button>' +
          "</div>";
        document.getElementById("onboard-create").onclick = async function () {
          try {
            var created = await post("/companies", { orgnr: orgnr });
            toast(created.navn + " opprettet med " + created.seeded_accounts + " kontoer", true);
            companies = [];
            renderCompanies();
          } catch (error) { toast(error.message, false); }
        };
      } catch (error) { toast(error.message, false); }
    };
  }

  async function renderOversikt(id) {
    var year = new Date().getFullYear();
    var termin = Math.floor(new Date().getMonth() / 2) + 1;
    var results = await Promise.all([
      api("/companies/" + id + "/invoices?open=true"),
      api("/companies/" + id + "/reports/mva?year=" + year + "&termin=" + termin).catch(function () { return null; }),
      api("/companies/" + id + "/period-lock"),
      api("/companies/" + id + "/vouchers"),
      api("/companies/" + id + "/invoices/overdue").catch(function () { return null; }),
    ]);
    var open = results[0].invoices;
    var mva = results[1];
    var lock = results[2];
    var vouchers = results[3].vouchers;
    var overdue = results[4];
    var overdueSum = overdue
      ? overdue.invoices.reduce(function (sum, i) { return sum + i.remaining_ore; }, 0)
      : 0;
    var openSum = open.reduce(function (sum, i) { return sum + i.remaining_ore; }, 0);
    var stats =
      '<div class="stats shadow-sm bg-base-100 w-full mb-6">' +
      '<div class="stat"><div class="stat-title">Utestående fakturaer</div>' +
      '<div class="stat-value text-2xl">' + kr(openSum) + '</div>' +
      '<div class="stat-desc">' + open.length + " åpne</div></div>" +
      '<div class="stat"><div class="stat-title">Forfalt</div>' +
      '<div class="stat-value text-2xl' + (overdueSum > 0 ? " text-error" : "") + '">' + kr(overdueSum) + "</div>" +
      '<div class="stat-desc">' + (overdue && overdue.invoices.length
        ? '<a class="link" href="#/c/' + id + '/faktura">' + overdue.invoices.length + " til oppfølging</a>"
        : "ingenting forfalt") + "</div></div>" +
      '<div class="stat"><div class="stat-title">Mva ' + termin + ". termin</div>" +
      '<div class="stat-value text-2xl">' + (mva ? kr(mva.netto_ore) : "–") + "</div>" +
      '<div class="stat-desc">' + (mva && mva.netto_ore >= 0 ? "å betale" : "til gode") + "</div></div>" +
      '<div class="stat"><div class="stat-title">Periode låst t.o.m.</div>' +
      '<div class="stat-value text-2xl">' + esc(lock.locked_through || "åpen") + "</div></div></div>";
    var recent = vouchers.slice(0, 8).map(function (v) {
      return "<tr><td>" + esc(v.voucher) + "</td><td>" + esc(v.date) + "</td><td>" +
        esc(v.description) + "</td></tr>";
    }).join("");
    var importCard = vouchers.length === 0
      ? card("Kom fra et annet system?",
          '<p class="text-sm opacity-70 mb-2">Last opp en SAF-T-eksport (alle norske systemer kan lage en) — ' +
          "kontoplan, kunder/leverandører og hele historikken importeres i én operasjon. " +
          "Har det gamle systemet en annen kontoplan, foreslår vi mapping til NS 4102 som du godkjenner først.</p>" +
          '<input type="file" id="saft-file" class="file-input file-input-bordered" accept=".xml">' +
          '<div id="mapping-step" class="mt-3"></div>') +
        card("Ingen SAF-T? Legg inn åpningsbalansen manuelt",
          '<p class="text-sm opacity-70 mb-2">Saldo per konto på overgangsdagen — må gå i null.</p>' +
          '<div class="flex gap-2 mb-2"><input id="ob-date" type="date" class="input input-sm input-bordered"></div>' +
          '<div id="ob-lines">' +
          '<div class="flex gap-2 mb-1 ob-line"><input class="input input-sm input-bordered w-24" placeholder="Konto" data-f="account">' +
          '<input class="input input-sm input-bordered w-36" placeholder="Beløp (debet +)" data-f="amount"></div>' +
          '<div class="flex gap-2 mb-1 ob-line"><input class="input input-sm input-bordered w-24" placeholder="Konto" data-f="account">' +
          '<input class="input input-sm input-bordered w-36" placeholder="Beløp (kredit −)" data-f="amount"></div></div>' +
          '<button id="ob-add" class="btn btn-xs btn-ghost">+ linje</button> ' +
          '<button id="ob-post" class="btn btn-sm btn-primary">Legg inn åpningsbalanse</button>')
      : "";
    var anchors = (await api("/companies/" + id + "/anchors").catch(function () { return { anchors: [] }; })).anchors;
    var latest = anchors[0];
    var anchorBody = latest
      ? '<p class="text-sm opacity-70 mb-2">Sist forankret ' + esc(latest.created_at.slice(0, 16).replace("T", " ")) +
        " (bilag t.o.m. sekvens " + latest.last_seq + (latest.witnesses.length ? ", bevitnet eksternt" : "") + ").</p>" +
        '<p class="text-xs font-mono opacity-50 mb-2 break-all">rot ' + esc(latest.root_hash) + "</p>"
      : '<p class="text-sm opacity-70 mb-2">Ikke forankret ennå — kjøres periodisk av systemet.</p>';
    var anchorCard = card("Forankring",
      anchorBody +
      '<p class="text-sm opacity-70 mb-2">Hovedbokens hash-kjede forankres under en offentlig rot utenfor databasen — ' +
      "omskrevet historikk kan derfor bevises, ikke bare mistenkes.</p>" +
      '<button id="anchor-verify" class="btn btn-sm btn-outline">Verifiser kjeden mot forankringen</button>' +
      '<div id="anchor-result" class="mt-2"></div>');
    shell(id, "oversikt", stats + importCard + card("Siste bilag",
      '<table class="table table-sm"><thead><tr><th>Bilag</th><th>Dato</th><th>Tekst</th></tr></thead>' +
      "<tbody>" + recent + "</tbody></table>") + anchorCard);
    document.getElementById("anchor-verify").onclick = async function () {
      var result = document.getElementById("anchor-result");
      result.innerHTML = '<span class="loading loading-spinner loading-sm"></span>';
      try {
        var check = await api("/companies/" + id + "/anchors/verify");
        result.innerHTML = check.ok
          ? '<div class="alert alert-success text-sm py-2">Kjeden verifisert fra genesis: ' +
            check.vouchers_checked + " bilag, " + check.attachments_checked + " vedlegg, " +
            check.anchors_checked + " forankringer stemmer.</div>"
          : '<div class="alert alert-error text-sm py-2">' +
            check.problems.map(esc).join("<br>") + "</div>";
      } catch (error) { result.innerHTML = '<div class="alert alert-error text-sm py-2">' + esc(error.message) + "</div>"; }
    };
    function importDone(result) {
      toast(result.vouchers + " bilag, " + result.accounts + " kontoer importert" +
        (result.warnings.length ? " (" + result.warnings.length + " merknader)" : ""), true);
      renderOversikt(id);
    }
    var saftInput = document.getElementById("saft-file");
    if (saftInput) saftInput.onchange = async function (event) {
      var file = event.target.files[0];
      if (!file) return;
      var xml = await file.text();
      try {
        var analysis = await api("/companies/" + id + "/import/saft/analyze", {
          method: "POST", body: xml,
        });
        if (!analysis.needs_mapping) {
          importDone(await api("/companies/" + id + "/import/saft", { method: "POST", body: xml }));
          return;
        }
        // Kontoplan wizard: review and complete the suggested mapping.
        var step = document.getElementById("mapping-step");
        step.innerHTML = '<div class="border border-base-300 rounded-lg p-3">' +
          '<p class="text-sm font-semibold mb-1">Kontoplanen må mappes til NS 4102</p>' +
          '<p class="text-xs opacity-70 mb-2">' + analysis.transactions + " transaksjoner, " +
          analysis.customers + " kunder, " + analysis.suppliers + " leverandører. " +
          "Forslagene under er heuristikk — du bestemmer.</p>" +
          '<table class="table table-xs"><thead><tr><th>Konto i filen</th><th>Navn</th>' +
          "<th>NS 4102</th><th>Forslag</th></tr></thead><tbody>" +
          analysis.accounts.map(function (a) {
            return "<tr><td>" + esc(a.account_id) + "</td><td>" + esc(a.name) + "</td>" +
              '<td><input class="input input-xs input-bordered w-20" data-map="' + esc(a.account_id) +
              '" value="' + esc(a.suggested || "") + '"></td>' +
              '<td class="text-xs opacity-60">' + esc(a.reason) +
              (a.standard_name ? " → " + esc(a.standard_name) : "") + "</td></tr>";
          }).join("") + "</tbody></table>" +
          '<button id="import-mapped" class="btn btn-sm btn-primary mt-2">Importer med denne mappingen</button></div>';
        document.getElementById("import-mapped").onclick = async function () {
          var mapping = {};
          var missing = false;
          step.querySelectorAll("[data-map]").forEach(function (input) {
            var value = input.value.trim();
            if (!value) { missing = true; input.classList.add("input-error"); return; }
            input.classList.remove("input-error");
            if (value !== input.dataset.map) mapping[input.dataset.map] = value;
          });
          if (missing) { toast("alle kontoer må ha et NS 4102-nummer", false); return; }
          try {
            importDone(await api("/companies/" + id + "/import/saft", {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ file: xml, mapping: mapping }),
            }));
          } catch (error) { toast(error.message, false); }
        };
      } catch (error) { toast(error.message, false); }
    };
    var obPost = document.getElementById("ob-post");
    if (obPost) {
      document.getElementById("ob-date").value = new Date().getFullYear() + "-01-01";
      document.getElementById("ob-add").onclick = function () {
        document.getElementById("ob-lines").insertAdjacentHTML("beforeend",
          '<div class="flex gap-2 mb-1 ob-line"><input class="input input-sm input-bordered w-24" placeholder="Konto" data-f="account">' +
          '<input class="input input-sm input-bordered w-36" placeholder="Beløp" data-f="amount"></div>');
      };
      obPost.onclick = async function () {
        var lines = [];
        document.querySelectorAll(".ob-line").forEach(function (row) {
          var account = row.querySelector('[data-f="account"]').value.trim();
          var amount = row.querySelector('[data-f="amount"]').value.trim().replace(/\s/g, "");
          if (!account || !amount) return;
          lines.push({ account: account, amount_ore: Math.round(Number(amount.replace(",", ".")) * 100) });
        });
        try {
          var result = await post("/companies/" + id + "/opening-balance", {
            date: document.getElementById("ob-date").value, lines: lines,
          });
          toast("Åpningsbalanse bokført som bilag " + result.voucher, true);
          renderOversikt(id);
        } catch (error) { toast(error.message, false); }
      };
    }
  }

  async function renderFaktura(id) {
    var results = await Promise.all([
      api("/companies/" + id + "/invoices"),
      api("/companies/" + id + "/parties?kind=kunde"),
      api("/companies/" + id + "/invoices/overdue").catch(function () { return { invoices: [], buckets: {} }; }),
      api("/companies/" + id + "/dimensions").catch(function () { return { dimensions: [] }; }),
    ]);
    var invoices = results[0].invoices;
    var parties = results[1].parties;
    var overdue = results[2];
    var dims = results[3].dimensions;
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
        (dims.length
          ? '<div class="flex gap-2" id="invoice-dims">' +
            dimSelect(dims, "avdeling", "select select-bordered flex-1", "avdeling") +
            dimSelect(dims, "prosjekt", "select select-bordered flex-1", "prosjekt") + "</div>"
          : "") +
        '<button class="btn btn-primary">Opprett faktura</button></form>';
    var stegNavn = { paminnelse: "påminnelse", purring: "purring", inkassovarsel: "inkassovarsel" };
    var nesteSteg = { paminnelse: "purring", purring: "inkassovarsel", inkassovarsel: "inkassovarsel" };
    var overdueRows = overdue.invoices.map(function (i) {
      var badge = i.bucket === "30+" ? "badge-error" : i.bucket === "15-30" ? "badge-warning" : "badge-ghost";
      return "<tr><td>" + i.invoice_no + "</td><td>" + esc(i.party_name) + "</td><td>" + esc(i.due_date) +
        '</td><td><span class="badge badge-sm ' + badge + '">' + i.days_overdue + " dager</span></td>" +
        "<td class='text-right'>" + kr(i.remaining_ore) + "</td>" +
        "<td class='text-xs opacity-70'>" + (i.last_steg ? esc(stegNavn[i.last_steg]) + " " + esc(i.last_sent) : "–") + "</td>" +
        '<td><button class="btn btn-xs btn-outline" data-purr="' + i.invoice_id +
        '" data-steg="' + (i.last_steg ? nesteSteg[i.last_steg] : "paminnelse") + '">Purring</button></td></tr>';
    }).join("");
    var overdueCard = overdue.invoices.length === 0 ? "" :
      card("Forfalte fakturaer",
        '<div class="flex gap-2 mb-2 text-sm">' +
        ["1-14", "15-30", "30+"].map(function (b) {
          var sum = overdue.buckets[b] || 0;
          return '<span class="badge badge-ghost">' + b + " dager: " + kr(sum) + "</span>";
        }).join("") + "</div>" +
        '<table class="table table-sm"><thead><tr><th>Nr</th><th>Kunde</th><th>Forfall</th>' +
        "<th>Alder</th><th class='text-right'>Utestående</th><th>Siste skritt</th><th></th></tr></thead>" +
        "<tbody>" + overdueRows + "</tbody></table>" +
        '<div id="purring-form"></div>');
    shell(id, "faktura",
      overdueCard +
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
            avdeling: (function () {
              var s = form_.querySelector('#invoice-dims [data-f="avdeling"]');
              return s && s.value ? s.value : null;
            })(),
            prosjekt: (function () {
              var s = form_.querySelector('#invoice-dims [data-f="prosjekt"]');
              return s && s.value ? s.value : null;
            })(),
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
    app.querySelectorAll("[data-purr]").forEach(function (button) {
      button.onclick = function () { openPurringForm(id, button.dataset.purr, button.dataset.steg); };
    });
  }

  // Purring: alltid en eksplisitt menneskelig handling — forhåndsvis
  // kravet (gebyrtak og rente hentes fra satsregisteret), så registrer.
  async function openPurringForm(id, invoiceId, suggestedSteg) {
    var target = document.getElementById("purring-form");
    var base = "/companies/" + id + "/invoices/" + invoiceId + "/reminders";
    var history = (await api(base).catch(function () { return { reminders: [] }; })).reminders;
    var historyRows = history.map(function (r) {
      return "<tr><td>" + esc(r.steg) + "</td><td>" + esc(r.sent_date) + "</td><td>" + esc(r.frist_date) +
        "</td><td class='text-right'>" + kr(r.gebyr_ore + r.rente_ore) + "</td>" +
        "<td>" + (r.voucher ? esc(r.voucher) : "–") + "</td>" +
        '<td><button class="link text-xs" data-purr-doc="' + r.reminder_id + '">tekst</button></td></tr>';
    }).join("");
    var frist = new Date(Date.now() + 14 * 86400000).toISOString().slice(0, 10);
    target.innerHTML =
      '<div class="border border-base-300 rounded-lg p-3 mt-3">' +
      (history.length
        ? '<p class="text-sm font-semibold mb-1">Purrehistorikk</p>' +
          '<table class="table table-xs mb-3"><thead><tr><th>Skritt</th><th>Sendt</th><th>Frist</th>' +
          "<th class='text-right'>Gebyr+rente</th><th>Bilag</th><th></th></tr></thead><tbody>" +
          historyRows + "</tbody></table>"
        : "") +
      '<div class="grid gap-2 max-w-md">' +
      '<label class="form-control"><span class="label-text">Skritt</span>' +
      '<select id="purr-steg" class="select select-sm select-bordered">' +
      '<option value="paminnelse">Betalingspåminnelse (gebyrfri)</option>' +
      '<option value="purring">Purring</option>' +
      '<option value="inkassovarsel">Inkassovarsel (14 dagers frist)</option></select></label>' +
      '<label class="form-control"><span class="label-text">Betalingsfrist</span>' +
      '<input id="purr-frist" type="date" class="input input-sm input-bordered" value="' + frist + '"></label>' +
      '<label class="label cursor-pointer justify-start gap-2"><input id="purr-gebyr" type="checkbox" class="checkbox checkbox-sm">' +
      '<span class="label-text">Purregebyr (maks-sats)</span></label>' +
      '<label class="label cursor-pointer justify-start gap-2"><input id="purr-rente" type="checkbox" class="checkbox checkbox-sm">' +
      '<span class="label-text">Krev forsinkelsesrente</span></label>' +
      '<label class="label cursor-pointer justify-start gap-2"><input id="purr-naering" type="checkbox" class="checkbox checkbox-sm">' +
      '<span class="label-text">Næringsdrivende skyldner (standardkompensasjon)</span></label>' +
      '<div class="flex gap-2"><button id="purr-preview" class="btn btn-sm">Forhåndsvis</button>' +
      '<button id="purr-send" class="btn btn-sm btn-primary" disabled>Registrer</button></div>' +
      '<div id="purr-result"></div></div></div>';
    document.getElementById("purr-steg").value = suggestedSteg;
    target.querySelectorAll("[data-purr-doc]").forEach(function (link) {
      link.onclick = async function () {
        try {
          var response = await api(base + "/" + link.dataset.purrDoc + "?format=tekst");
          var blob = await response.blob();
          var a = document.createElement("a");
          a.href = URL.createObjectURL(blob);
          a.download = "purring.txt";
          a.click();
          URL.revokeObjectURL(a.href);
        } catch (error) { toast(error.message, false); }
      };
    });
    function body(gebyrOre) {
      return {
        steg: document.getElementById("purr-steg").value,
        frist_date: document.getElementById("purr-frist").value,
        gebyr_ore: document.getElementById("purr-gebyr").checked ? gebyrOre : 0,
        med_rente: document.getElementById("purr-rente").checked,
        naeringsdrivende: document.getElementById("purr-naering").checked,
      };
    }
    var previewed = null;
    document.getElementById("purr-preview").onclick = async function () {
      var result = document.getElementById("purr-result");
      try {
        // First pass resolves the current maks-sats, second previews with it.
        var probe = await post(base + "?preview=true", body(0));
        previewed = await post(base + "?preview=true", body(probe.maks_gebyr_ore));
        result.innerHTML =
          '<p class="text-sm mt-2">Å betale: <b>' + kr(previewed.total_ore) + "</b>" +
          (previewed.gebyr_ore ? " (gebyr " + kr(previewed.gebyr_ore) : "") +
          (previewed.rente_ore ? (previewed.gebyr_ore ? ", " : " (") + "rente " + kr(previewed.rente_ore) : "") +
          (previewed.gebyr_ore || previewed.rente_ore ? ")" : "") + "</p>" +
          '<pre class="bg-base-200 rounded p-2 text-xs overflow-x-auto mt-2">' + esc(previewed.document) + "</pre>";
        document.getElementById("purr-send").disabled = false;
      } catch (error) { previewed = null; result.innerHTML = ""; toast(error.message, false); }
    };
    document.getElementById("purr-send").onclick = async function () {
      if (!previewed) return;
      try {
        var created = await post(base, body(previewed.gebyr_ore));
        toast(created.steg + " registrert" + (created.voucher ? " (bilag " + created.voucher + ")" : ""), true);
        renderFaktura(id);
      } catch (error) { toast(error.message, false); }
    };
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

  // Lovpålagte spesifikasjoner (bokføringsforskriften §3-1): rene
  // SUM-spørringer mot hovedboken, aldri lagret tilstand.
  async function renderRapporter(id) {
    var year = new Date().getFullYear();
    var rapport = "saldobalanse";
    var dimFilter = { avdeling: "", prosjekt: "" };
    var hash = location.hash.split("?")[1];
    if (hash) {
      var params = new URLSearchParams(hash);
      year = Number(params.get("year") || year);
      rapport = params.get("rapport") || rapport;
      dimFilter.avdeling = params.get("avdeling") || "";
      dimFilter.prosjekt = params.get("prosjekt") || "";
    }
    var from = year + "-01-01", to = year + "-12-31";
    var tabs = [
      ["saldobalanse", "Saldobalanse"], ["resultat", "Resultat"], ["balanse", "Balanse"],
      ["kontospesifikasjon", "Kontospesifikasjon"], ["bokforingsspesifikasjon", "Bokføringsspesifikasjon"],
      ["revisjon", "Revisjon"],
    ].map(function (t) {
      return '<a class="join-item btn btn-sm ' + (t[0] === rapport ? "btn-primary" : "") +
        '" href="#/c/' + id + "/rapporter?rapport=" + t[0] + "&year=" + year + '">' + t[1] + "</a>";
    }).join("");
    var yearNav = '<div class="join">' +
      '<a class="join-item btn btn-sm" href="#/c/' + id + "/rapporter?rapport=" + rapport + "&year=" + (year - 1) + '">«</a>' +
      '<span class="join-item btn btn-sm btn-disabled">' + year + "</span>" +
      '<a class="join-item btn btn-sm" href="#/c/' + id + "/rapporter?rapport=" + rapport + "&year=" + (year + 1) + '">»</a></div>';
    var body = "", title = "";
    function seksjonRows(s) {
      return s.lines.map(function (l) {
        return "<tr><td>" + esc(l.number) + "</td><td>" + esc(l.name) +
          "</td><td class='text-right'>" + kr(l.saldo_ore) + "</td></tr>";
      }).join("") +
      "<tr class='font-semibold'><td></td><td>Sum " + esc(s.heading.toLowerCase()) +
      "</td><td class='text-right'>" + kr(s.sum_ore) + "</td></tr>";
    }
    if (rapport === "saldobalanse") {
      title = "Saldobalanse " + year;
      var sb = await api("/companies/" + id + "/reports/saldobalanse?from=" + from + "&to=" + to);
      body = '<table class="table table-sm"><thead><tr><th>Konto</th><th>Navn</th>' +
        "<th class='text-right'>Inngående</th><th class='text-right'>Debet</th>" +
        "<th class='text-right'>Kredit</th><th class='text-right'>Utgående</th></tr></thead><tbody>" +
        sb.accounts.map(function (a) {
          return "<tr><td>" + esc(a.number) + "</td><td>" + esc(a.name) +
            "</td><td class='text-right'>" + kr(a.inngaende_ore) +
            "</td><td class='text-right'>" + kr(a.debet_ore) +
            "</td><td class='text-right'>" + kr(a.kredit_ore) +
            "</td><td class='text-right'>" + kr(a.utgaende_ore) + "</td></tr>";
        }).join("") + "</tbody></table>";
    } else if (rapport === "resultat") {
      title = "Resultatregnskap " + year;
      var filterQuery = (dimFilter.avdeling ? "&avdeling=" + encodeURIComponent(dimFilter.avdeling) : "") +
        (dimFilter.prosjekt ? "&prosjekt=" + encodeURIComponent(dimFilter.prosjekt) : "");
      var resultatData = await Promise.all([
        api("/companies/" + id + "/reports/resultat?from=" + from + "&to=" + to + filterQuery),
        api("/companies/" + id + "/dimensions").catch(function () { return { dimensions: [] }; }),
      ]);
      var r = resultatData[0];
      var rDims = resultatData[1].dimensions;
      var dimPicker = function (kind) {
        var all = rDims.filter(function (d) { return d.kind === kind; });
        if (!all.length) return "";
        return '<select class="select select-sm select-bordered" data-resultat-dim="' + kind + '">' +
          '<option value="">Alle ' + (kind === "avdeling" ? "avdelinger" : "prosjekter") + "</option>" +
          all.map(function (d) {
            return '<option value="' + esc(d.code) + '"' +
              (dimFilter[kind] === d.code ? " selected" : "") + ">" +
              esc(d.code) + " " + esc(d.name) + (d.active ? "" : " (avsluttet)") + "</option>";
          }).join("") + "</select>";
      };
      body = (rDims.length
          ? '<div class="flex gap-2 mb-3">' + dimPicker("avdeling") + dimPicker("prosjekt") + "</div>"
          : "") +
        '<table class="table table-sm"><tbody>' +
        r.seksjoner.map(seksjonRows).join("") +
        "<tr class='font-bold'><td></td><td>Driftsresultat</td><td class='text-right'>" +
        kr(r.driftsresultat_ore) + "</td></tr>" +
        "<tr class='font-bold'><td></td><td>Årsresultat</td><td class='text-right'>" +
        kr(r.arsresultat_ore) + "</td></tr></tbody></table>";
    } else if (rapport === "balanse") {
      title = "Balanse per " + to;
      var b = await api("/companies/" + id + "/reports/balanse?date=" + to);
      body = '<table class="table table-sm"><tbody>' +
        seksjonRows(b.eiendeler) + seksjonRows(b.egenkapital_gjeld) +
        "<tr><td></td><td>Udisponert resultat</td><td class='text-right'>" +
        kr(b.udisponert_resultat_ore) + "</td></tr>" +
        "<tr class='font-bold'><td></td><td>Differanse (skal være 0)</td><td class='text-right'>" +
        kr(b.differanse_ore) + "</td></tr></tbody></table>";
    } else if (rapport === "kontospesifikasjon") {
      title = "Kontospesifikasjon " + year;
      var ks = await api("/companies/" + id + "/reports/kontospesifikasjon?from=" + from + "&to=" + to);
      body = '<table class="table table-sm"><thead><tr><th>Konto</th><th>Bilag</th><th>Dato</th>' +
        "<th>Tekst</th><th class='text-right'>Beløp</th><th class='text-right'>Saldo</th></tr></thead><tbody>" +
        ks.posts.map(function (p) {
          var badges = (p.party_no ? ' <span class="opacity-60">(' + esc(p.party_no) + ")</span>" : "") +
            (p.avdeling ? ' <span class="badge badge-ghost badge-xs">' + esc(p.avdeling) + "</span>" : "") +
            (p.prosjekt ? ' <span class="badge badge-ghost badge-xs">' + esc(p.prosjekt) + "</span>" : "");
          return "<tr><td>" + esc(p.account) + "</td><td>" + esc(p.bilag) + "</td><td>" + esc(p.date) +
            "</td><td>" + esc(p.description) + badges +
            "</td><td class='text-right'>" + kr(p.amount_ore) +
            "</td><td class='text-right'>" + kr(p.saldo_ore) + "</td></tr>";
        }).join("") + "</tbody></table>";
    } else if (rapport === "revisjon") {
      title = "Verifikasjonsrapport";
      var rr = await api("/companies/" + id + "/reports/revisjon");
      body = '<div class="alert ' + (rr.alle_ok ? "alert-success" : "alert-error") + ' mb-4">' +
        (rr.alle_ok ? "Alle kontroller OK" : "Avvik funnet — se kontrollene under") +
        ' <span class="text-xs font-mono opacity-70">kjedehode: sekvens ' + rr.kjede_sekvens + "</span></div>" +
        '<table class="table table-sm mb-4"><tbody>' +
        rr.kontroller.map(function (k) {
          return "<tr><td>" + (k.ok ? "✓" : "✗") + "</td><td class='font-semibold'>" + esc(k.navn) +
            "</td><td>" + esc(k.detalj) + "</td></tr>";
        }).join("") + "</tbody></table>" +
        (rr.ankere.length
          ? '<p class="text-sm font-semibold mb-1">Eksterne forankringer</p>' +
            '<table class="table table-sm mb-4"><tbody>' +
            rr.ankere.map(function (a) {
              return "<tr><td>" + esc(a.tidspunkt.slice(0, 16).replace("T", " ")) +
                "</td><td>sekvens " + a.siste_sekvens +
                "</td><td class='font-mono text-xs break-all'>" + esc(a.root) +
                (a.vitner.length ? "<br><span class='opacity-60'>" + a.vitner.map(esc).join("<br>") + "</span>" : "") +
                "</td></tr>";
            }).join("") + "</tbody></table>"
          : '<p class="text-sm opacity-70 mb-4">Ingen forankringer omfatter selskapet ennå.</p>') +
        '<button id="dl-revisjon" class="btn btn-outline btn-sm">Last ned rapport (tekst)</button>' +
        '<p class="text-xs opacity-60 mt-2">Rapporten kan etterprøves uavhengig av regnmed: ' +
        "kjeden re-beregnes fra genesis, røttene sammenlignes med den offentlige /anchors-strømmen, " +
        "og RFC 3161-vitner verifiseres frakoblet.</p>";
    } else {
      title = "Bokføringsspesifikasjon " + year;
      var bs = await api("/companies/" + id + "/reports/bokforingsspesifikasjon?from=" + from + "&to=" + to);
      body = bs.vouchers.map(function (v) {
        return '<div class="mb-3"><span class="font-semibold">' + esc(v.bilag) + "</span> " +
          esc(v.date) + " — " + esc(v.description) +
          '<table class="table table-sm"><tbody>' +
          v.lines.map(function (l) {
            return "<tr><td>" + esc(l.account) + " " + esc(l.account_name) +
              "</td><td>" + (l.vat_code ? "mva " + esc(l.vat_code) : "") +
              "</td><td class='text-right'>" + kr(l.amount_ore) + "</td></tr>";
          }).join("") + "</tbody></table></div>";
      }).join("") || '<p class="opacity-70">Ingen bilag i perioden.</p>';
    }
    shell(id, "rapporter", card(title,
      '<div class="flex gap-2 flex-wrap mb-4"><div class="join">' + tabs + "</div>" + yearNav + "</div>" + body));
    app.querySelectorAll("[data-resultat-dim]").forEach(function (select) {
      select.onchange = function () {
        dimFilter[select.dataset.resultatDim] = select.value;
        var query = "rapport=resultat&year=" + year +
          (dimFilter.avdeling ? "&avdeling=" + encodeURIComponent(dimFilter.avdeling) : "") +
          (dimFilter.prosjekt ? "&prosjekt=" + encodeURIComponent(dimFilter.prosjekt) : "");
        location.hash = "#/c/" + id + "/rapporter?" + query;
      };
    });
    var dlRevisjon = document.getElementById("dl-revisjon");
    if (dlRevisjon) dlRevisjon.onclick = async function () {
      try {
        var response = await api("/companies/" + id + "/reports/revisjon?format=tekst");
        var blob = await response.blob();
        var a = document.createElement("a");
        a.href = URL.createObjectURL(blob);
        a.download = "verifikasjonsrapport.txt";
        a.click();
        URL.revokeObjectURL(a.href);
      } catch (error) { toast(error.message, false); }
    };
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
      card("Kontoutskrift (camt.053 eller CSV)",
        '<input type="file" id="camt-file" class="file-input file-input-bordered" accept=".xml,.csv,.txt">' +
        '<p class="text-sm opacity-70 mt-2">Last ned fra nettbanken (camt.053-XML eller CSV-eksport med ' +
        "kolonneoverskrifter) og last opp her — konto " + account + ".</p>") +
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

  // Active dimensions as <select> options; empty string = no dimension.
  function dimSelect(dims, kind, cls, dataF) {
    var active = dims.filter(function (d) { return d.kind === kind && d.active; });
    if (!active.length) return "";
    return '<select class="' + cls + '" data-f="' + dataF + '" title="' + kind + '">' +
      '<option value="">(' + kind + ")</option>" +
      active.map(function (d) {
        return '<option value="' + esc(d.code) + '">' + esc(d.code) + " " + esc(d.name) + "</option>";
      }).join("") + "</select>";
  }

  async function renderBilag(id) {
    var results = await Promise.all([
      api("/companies/" + id + "/vouchers"),
      api("/companies/" + id + "/inbox"),
      api("/companies/" + id + "/dimensions").catch(function () { return { dimensions: [] }; }),
    ]);
    var vouchers = results[0].vouchers;
    var inbox = results[1].documents;
    var dims = results[2].dimensions;
    var open = inbox.filter(function (d) { return d.status === "ny"; });
    var decided = inbox.filter(function (d) { return d.status !== "ny"; });
    var inboxRows = open.map(function (d) {
      return "<tr><td><a class='link' href='#' data-dl-doc='" + d.document_id + "' data-name='" +
        esc(d.filename) + "'>" + esc(d.filename) + "</a></td><td>" +
        esc(d.uploaded_at.slice(0, 10)) + "</td><td>" + esc(d.uploaded_by) + "</td><td>" +
        '<button class="btn btn-xs btn-primary" data-bokfor="' + d.document_id + '">Bokfør</button> ' +
        '<button class="btn btn-xs btn-ghost" data-avvis="' + d.document_id + '">Avvis</button></td></tr>';
    }).join("");
    var decidedRows = decided.slice(0, 6).map(function (d) {
      return '<div class="text-xs opacity-60 py-0.5">' + esc(d.filename) + " — " + esc(d.status) +
        (d.note ? " (" + esc(d.note) + ")" : "") + " · " + esc(d.decided_by || "") + "</div>";
    }).join("");
    var inboxCard = card("Innboks — dokumentasjon som venter på bokføring",
      '<label class="btn btn-sm btn-outline mb-3">Last opp bilag' +
      '<input type="file" id="inbox-upload" class="hidden"></label>' +
      (open.length
        ? '<table class="table table-sm"><thead><tr><th>Dokument</th><th>Mottatt</th><th>Fra</th><th></th></tr></thead>' +
          "<tbody>" + inboxRows + "</tbody></table>"
        : '<p class="text-sm opacity-70">Ingen dokumenter venter.</p>') +
      '<div id="bokfor-form" class="mt-3"></div>' + decidedRows);
    var rows = vouchers.map(function (v) {
      return "<tr><td>" + esc(v.voucher) + "</td><td>" + esc(v.date) + "</td><td>" +
        esc(v.description) + '</td><td><label class="btn btn-xs btn-outline">Vedlegg' +
        '<input type="file" class="hidden" data-attach="' + v.voucher_id + '"></label> ' +
        '<button class="btn btn-xs btn-ghost" data-list="' + v.voucher_id + '">Vis</button></td></tr>';
    }).join("");
    var dimRows = dims.map(function (d) {
      return "<tr" + (d.active ? "" : ' class="opacity-50"') + "><td>" + esc(d.kind) + "</td><td>" +
        esc(d.code) + "</td><td>" + esc(d.name) + "</td><td>" +
        '<button class="btn btn-xs btn-ghost" data-dim-toggle="' + esc(d.kind) + ":" + esc(d.code) +
        '" data-active="' + d.active + '">' + (d.active ? "Avslutt" : "Gjenåpne") + "</button></td></tr>";
    }).join("");
    var dimCard = card("Dimensjoner — avdeling og prosjekt",
      '<p class="text-sm opacity-70 mb-2">Koden er permanent (den inngår i bilagshashen); navnet kan endres, ' +
      "og avsluttede dimensjoner avviser nye posteringer.</p>" +
      (dims.length
        ? '<table class="table table-xs mb-2"><thead><tr><th>Type</th><th>Kode</th><th>Navn</th><th></th></tr></thead>' +
          "<tbody>" + dimRows + "</tbody></table>"
        : "") +
      '<div class="flex gap-2">' +
      '<select id="dim-kind" class="select select-sm select-bordered">' +
      '<option value="avdeling">avdeling</option><option value="prosjekt">prosjekt</option></select>' +
      '<input id="dim-code" class="input input-sm input-bordered w-24" placeholder="Kode">' +
      '<input id="dim-name" class="input input-sm input-bordered" placeholder="Navn">' +
      '<button id="dim-create" class="btn btn-sm">Opprett</button></div>');
    shell(id, "bilag", inboxCard + card("Bilag",
      '<table class="table table-sm"><thead><tr><th>Bilag</th><th>Dato</th><th>Tekst</th><th></th></tr></thead>' +
      "<tbody>" + rows + "</tbody></table><div id='attachment-list' class='mt-4'></div>") + dimCard);
    document.getElementById("dim-create").onclick = async function () {
      try {
        await post("/companies/" + id + "/dimensions", {
          kind: document.getElementById("dim-kind").value,
          code: document.getElementById("dim-code").value.trim(),
          name: document.getElementById("dim-name").value.trim(),
        });
        toast("Dimensjon opprettet", true);
        renderBilag(id);
      } catch (error) { toast(error.message, false); }
    };
    app.querySelectorAll("[data-dim-toggle]").forEach(function (button) {
      button.onclick = async function () {
        var parts = button.dataset.dimToggle.split(":");
        try {
          await api("/companies/" + id + "/dimensions/" + parts[0] + "/" + encodeURIComponent(parts[1]), {
            method: "PUT",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ active: button.dataset.active !== "true" }),
          });
          renderBilag(id);
        } catch (error) { toast(error.message, false); }
      };
    });
    var inboxUpload = document.getElementById("inbox-upload");
    if (inboxUpload) inboxUpload.onchange = async function () {
      var file = inboxUpload.files[0];
      if (!file) return;
      try {
        await api("/companies/" + id + "/inbox?filename=" + encodeURIComponent(file.name), {
          method: "POST",
          headers: { "content-type": file.type || "application/octet-stream" },
          body: file,
        });
        toast("Dokument mottatt i innboksen", true);
        renderBilag(id);
      } catch (error) { toast(error.message, false); }
    };
    app.querySelectorAll("[data-dl-doc]").forEach(function (link) {
      link.onclick = async function (event) {
        event.preventDefault();
        try {
          var response = await api("/companies/" + id + "/inbox/" + link.dataset.dlDoc + "/content");
          var blob = await response.blob();
          var a = document.createElement("a");
          a.href = URL.createObjectURL(blob);
          a.download = link.dataset.name;
          a.click();
          URL.revokeObjectURL(a.href);
        } catch (error) { toast(error.message, false); }
      };
    });
    app.querySelectorAll("[data-avvis]").forEach(function (button) {
      button.onclick = async function () {
        var note = prompt("Hvorfor avvises dokumentet?");
        if (!note) return;
        try {
          await post("/companies/" + id + "/inbox/" + button.dataset.avvis + "/avvis", { note: note });
          toast("Avvist", true);
          renderBilag(id);
        } catch (error) { toast(error.message, false); }
      };
    });
    app.querySelectorAll("[data-bokfor]").forEach(function (button) {
      button.onclick = function () {
        var docId = button.dataset.bokfor;
        var target = document.getElementById("bokfor-form");
        function lineHtml() {
          return '<div class="flex gap-2 mb-1 bokfor-line">' +
            '<input class="input input-sm input-bordered w-24" placeholder="Konto" data-f="account">' +
            '<input class="input input-sm input-bordered w-32" placeholder="Beløp (f.eks. -125,50)" data-f="amount">' +
            '<input class="input input-sm input-bordered w-16" placeholder="Mva" data-f="vat">' +
            dimSelect(dims, "avdeling", "select select-sm select-bordered w-28", "avdeling") +
            dimSelect(dims, "prosjekt", "select select-sm select-bordered w-28", "prosjekt") +
            "</div>";
        }
        target.innerHTML = '<div class="border border-base-300 rounded-lg p-3">' +
          '<p class="text-sm font-semibold mb-2">Bokfør dokument</p>' +
          '<div class="flex gap-2 mb-2">' +
          '<input id="bf-date" type="date" class="input input-sm input-bordered">' +
          '<input id="bf-desc" class="input input-sm input-bordered flex-1" placeholder="Tekst">' +
          "</div><div id='bf-lines'>" + lineHtml() + lineHtml() + "</div>" +
          '<button id="bf-add" class="btn btn-xs btn-ghost">+ linje</button> ' +
          '<button id="bf-post" class="btn btn-sm btn-primary">Bokfør</button></div>';
        document.getElementById("bf-date").value = new Date().toISOString().slice(0, 10);
        document.getElementById("bf-add").onclick = function () {
          document.getElementById("bf-lines").insertAdjacentHTML("beforeend", lineHtml());
        };
        document.getElementById("bf-post").onclick = async function () {
          var lines = [];
          target.querySelectorAll(".bokfor-line").forEach(function (row) {
            var account = row.querySelector('[data-f="account"]').value.trim();
            var amount = row.querySelector('[data-f="amount"]').value.trim().replace(/\s/g, "");
            if (!account || !amount) return;
            var ore = Math.round(Number(amount.replace(",", ".")) * 100);
            var vat = row.querySelector('[data-f="vat"]').value.trim();
            var dimValue = function (name) {
              var select = row.querySelector('[data-f="' + name + '"]');
              return select && select.value ? select.value : null;
            };
            lines.push({
              account: account, amount_ore: ore, vat_code: vat || null,
              avdeling: dimValue("avdeling"), prosjekt: dimValue("prosjekt"),
            });
          });
          try {
            var posted = await post("/companies/" + id + "/inbox/" + docId + "/bokfor", {
              journal_code: "GL",
              date: document.getElementById("bf-date").value,
              description: document.getElementById("bf-desc").value || "Bokført fra innboks",
              lines: lines,
            });
            toast("Bokført som bilag " + posted.voucher, true);
            renderBilag(id);
          } catch (error) { toast(error.message, false); }
        };
      };
    });
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

  async function renderOppdrag(id) {
    var results = await Promise.all([
      api("/companies/" + id + "/engagements"),
      api("/directory/firms"),
    ]);
    var engagements = results[0].engagements;
    var firms = results[1].firms;
    var active = engagements.filter(function (e) { return !e.valid_to; });
    var rows = engagements.map(function (e) {
      var action = !e.valid_to
        ? '<button class="btn btn-xs btn-outline" data-end="' + e.engagement_id + '">Avslutt</button>'
        : '<span class="opacity-60 text-sm">avsluttet ' + esc(e.valid_to) + "</span>";
      return "<tr><td>" + esc(e.firm) + "</td><td>" + esc(e.kind) + "</td><td>" +
        esc(e.valid_from) + "</td><td>" + action + "</td></tr>";
    }).join("");
    var directory = firms.map(function (f) {
      var has = active.some(function (e) { return e.firm_id === f.firm_id; });
      return "<tr><td>" + esc(f.name) + "</td><td>" + esc(f.orgnr) + "</td><td>" +
        esc(f.kind) + "</td><td>" + f.client_count + "</td><td>" +
        (has ? '<span class="badge badge-ghost">aktivt oppdrag</span>'
             : '<button class="btn btn-xs btn-primary" data-request="' + f.firm_id + '">Be om oppdrag</button>') +
        "</td></tr>";
    }).join("");
    shell(id, "oppdrag",
      card("Oppdrag",
        engagements.length
          ? '<table class="table table-sm"><thead><tr><th>Byrå</th><th>Type</th><th>Fra</th><th></th></tr></thead><tbody>' +
            rows + "</tbody></table>"
          : '<p class="opacity-70">Ingen oppdrag ennå — finn et autorisert byrå under.</p>') +
      card("Autoriserte byråer (Finanstilsynet-verifisert)",
        '<table class="table table-sm"><thead><tr><th>Navn</th><th>Orgnr</th><th>Type</th>' +
        "<th>Klienter</th><th></th></tr></thead><tbody>" + directory + "</tbody></table>"));
    app.querySelectorAll("[data-request]").forEach(function (button) {
      button.onclick = async function () {
        try {
          await post("/companies/" + id + "/engagement-requests", { firm_id: button.dataset.request });
          toast("Forespørsel sendt", true);
          renderOppdrag(id);
        } catch (error) { toast(error.message, false); }
      };
    });
    app.querySelectorAll("[data-end]").forEach(function (button) {
      button.onclick = async function () {
        if (!confirm("Avslutte oppdraget?")) return;
        try {
          await post("/companies/" + id + "/engagements/" + button.dataset.end + "/end", {});
          toast("Oppdrag avsluttet", true);
          renderOppdrag(id);
        } catch (error) { toast(error.message, false); }
      };
    });
  }

  async function renderByra(firmId) {
    var results = await Promise.all([
      api("/firms/" + firmId + "/requests"),
      api("/firms/" + firmId + "/clients"),
      api("/firms/mine"),
    ]);
    var firm = results[2].firms.find(function (f) { return f.firm_id === firmId; });
    var pending = results[0].requests.filter(function (r) { return r.status === "pending"; });
    var requestRows = pending.map(function (r) {
      return "<tr><td>" + esc(r.company) + " (" + esc(r.orgnr) + ")</td><td>" + esc(r.kind) +
        "</td><td>" + esc(r.message || "") + "</td>" +
        '<td class="flex gap-1"><button class="btn btn-xs btn-primary" data-decide="' + r.request_id +
        '" data-accept="1">Godta</button><button class="btn btn-xs" data-decide="' + r.request_id +
        '" data-accept="">Avslå</button></td></tr>';
    }).join("");
    var clientRows = results[1].clients.map(function (e) {
      return "<tr><td>" + esc(e.company) + "</td><td>" + esc(e.kind) + "</td><td>" +
        esc(e.valid_from) + "</td><td>" + (e.valid_to ? "avsluttet " + esc(e.valid_to) : "aktivt") + "</td></tr>";
    }).join("");
    app.innerHTML =
      '<div class="navbar bg-base-100 shadow-sm">' +
      '<div class="flex-1 gap-2 items-baseline"><a href="#/" class="btn btn-ghost text-xl">regnmed</a>' +
      '<span class="text-sm opacity-70">' + esc(firm ? firm.name : "") + "</span></div>" +
      '<div class="flex-none gap-2">' + themeControls() +
      '<button id="logout" class="btn btn-ghost btn-sm">Logg ut</button></div></div>' +
      '<main class="p-6 max-w-4xl mx-auto">' +
      card("Innkommende forespørsler",
        pending.length
          ? '<table class="table table-sm"><thead><tr><th>Selskap</th><th>Type</th><th>Melding</th><th></th></tr></thead><tbody>' +
            requestRows + "</tbody></table>"
          : '<p class="opacity-70">Ingen ventende forespørsler.</p>') +
      card("Klienter",
        '<table class="table table-sm"><thead><tr><th>Selskap</th><th>Type</th><th>Fra</th><th>Status</th></tr></thead><tbody>' +
        clientRows + "</tbody></table>") +
      "</main>";
    wireChrome();
    app.querySelectorAll("[data-decide]").forEach(function (button) {
      button.onclick = async function () {
        try {
          await post("/firms/" + firmId + "/requests/" + button.dataset.decide + "/decision",
            { accept: !!button.dataset.accept });
          toast(button.dataset.accept ? "Oppdrag godtatt" : "Avslått", true);
          renderByra(firmId);
        } catch (error) { toast(error.message, false); }
      };
    });
  }

  // ---------- router ----------

  async function route() {
    if (!tokens()) return renderLogin();
    try {
      if (!companies.length) { me = await api("/me"); companies = me.companies; }
      var parts = location.hash.replace(/^#\/?/, "").split("?")[0].split("/");
      if (parts[0] === "byra" && parts[1]) return await renderByra(parts[1]);
      if (parts[0] === "c" && parts[1]) {
        var id = parts[1];
        var section = parts[2] || "oversikt";
        if (section === "oversikt") return await renderOversikt(id);
        if (section === "faktura") return await renderFaktura(id);
        if (section === "reskontro") return await renderReskontro(id, parts[3] || null);
        if (section === "mva") return await renderMva(id);
        if (section === "rapporter") return await renderRapporter(id);
        if (section === "bank") return await renderBank(id);
        if (section === "bilag") return await renderBilag(id);
        if (section === "periode") return await renderPeriode(id);
        if (section === "oppdrag") return await renderOppdrag(id);
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
