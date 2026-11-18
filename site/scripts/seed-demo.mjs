// Enriches the seed_browser demo company through the PUBLIC API (plus a little SQL for the firm).
// Usage: node seed-demo.mjs <apiBase> <shotsDir> <psql-command-prefix words...>
// Only fictional data. Run once against a throwaway database.
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const [base, dir, ...psql] = process.argv.slice(2);
const token = readFileSync(`${dir}/token.txt`, "utf8").trim();
const company = readFileSync(`${dir}/company.txt`, "utf8").trim();
const C = `${base}/companies/${company}`;

async function call(method, path, body, headers = {}) {
  const raw = typeof body === "string" || body instanceof Uint8Array;
  const r = await fetch(path.startsWith("http") ? path : C + path, {
    method,
    headers: { authorization: `Bearer ${token}`, ...(body && !raw ? { "content-type": "application/json" } : {}), ...headers },
    body: body === undefined ? undefined : raw ? body : JSON.stringify(body),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`${method} ${path} -> ${r.status} ${text.slice(0, 300)}`);
  try { return JSON.parse(text); } catch { return text; }
}
const sql = (q) => execFileSync(psql[0], [...psql.slice(1), "-v", "ON_ERROR_STOP=1", "-At", "-c", q], { encoding: "utf8" }).trim();
const tolerate = async (label, f) => { try { return await f(); } catch (e) { console.warn("skip", label, String(e).slice(0, 160)); } };

// --- accounts -------------------------------------------------------------
for (const [number, name] of [["6300", "Leie lokaler"], ["6800", "Kontorrekvisita"], ["6900", "Telefon"], ["7140", "Reisekostnad"], ["7770", "Bankgebyr"], ["6540", "Inventar"], ["6860", "Møter, kurs og oppdateringer"]])
  await tolerate(`account ${number}`, () => call("POST", "/accounts", { number, name }));

// Opening position so the bank account does not start the year negative.
await call("POST", "/vouchers", { journal_code: "GL", date: "2026-01-01", description: "Åpningsbalanse bank", lines: [{ account: "1920", amount_ore: 250_000_00 }, { account: "2050", amount_ore: -250_000_00 }] });

// --- customers ------------------------------------------------------------
const customers = [
  ["Nordlys Bygg AS", "Fjordgata 12, 9008 Tromsø", "okonomi@nordlysbygg.example"],
  ["Fjordkraft Marine AS", "Kaigaten 4, 5016 Bergen", "faktura@fjordkraftmarine.example"],
  ["Bakeriet på Hjørnet AS", "Storgata 31, 7013 Trondheim", "post@bakerietpahjornet.example"],
  ["Solbakken Eiendom AS", "Bygdøy allé 80, 0268 Oslo", "regnskap@solbakkeneiendom.example"],
  ["Vestland Elektro AS", "Industriveien 9, 4020 Stavanger", "faktura@vestlandelektro.example"],
];
const parties = [];
for (const [name, address, email] of customers) parties.push((await call("POST", "/parties", { kind: "kunde", name, address, email })).party_no ?? null);
console.log("parties", parties);

// --- projects + timesheet --------------------------------------------------
for (const [code, name, kunde, rate] of [["P100", "Nettbutikk Nordlys", parties[0], 1_250_00], ["P200", "Rådgivning Fjordkraft", parties[1], 1_450_00]]) {
  await tolerate(`project ${code}`, async () => {
    await call("POST", "/dimensions", { kind: "prosjekt", code, name, kunde });
    await call("PUT", `/dimensions/prosjekt/${code}`, { name, kunde, fakturerbar_default: true });
    await call("POST", `/dimensions/prosjekt/${code}/satser`, { timesats_ore: rate, valid_from: "2026-01-01" });
  });
}
const days = [];
for (let d = new Date("2026-10-19"); d <= new Date("2026-11-18"); d.setUTCDate(d.getUTCDate() + 1)) if (![0, 6].includes(d.getUTCDay())) days.push(d.toISOString().slice(0, 10));
for (const [i, date] of days.entries()) {
  const heavy = i % 3 !== 2;
  const plan = heavy ? [["P100", 270, "Produktsider og checkout"], ["P200", 180, "Likviditetsanalyse"]] : [["P200", 300, "Styrearbeid og rapportering"], ["P100", 150, "Feilretting og testing"]];
  for (const [project, minutes, description] of plan) await tolerate("time", () => call("POST", "/timesheet", { dato: date, minutter: minutes, beskrivelse: description, prosjekt: project }));
}

// --- invoices (net, month-day, paid?) --------------------------------------
const invoices = [
  [0, 48_000, "2026-02-05", "2026-02-19", "2026-02-18", "Utvikling – fase 1"],
  [1, 36_500, "2026-02-20", "2026-03-06", "2026-03-04", "Rådgivning februar"],
  [2, 12_800, "2026-03-10", "2026-03-24", "2026-03-24", "Regnskapsoppsett og opplæring"],
  [3, 64_000, "2026-03-28", "2026-04-11", "2026-04-09", "Konsulentbistand mars"],
  [4, 27_200, "2026-04-15", "2026-04-29", "2026-04-28", "Elektronisk innkjøpsløsning"],
  [0, 52_500, "2026-05-06", "2026-05-20", "2026-05-19", "Utvikling – fase 2"],
  [1, 41_000, "2026-05-22", "2026-06-05", "2026-06-05", "Rådgivning mai"],
  [3, 72_000, "2026-06-30", "2026-07-14", "2026-07-10", "Konsulentbistand juni"],
  [2, 9_600, "2026-08-12", "2026-08-26", "2026-08-25", "Månedlig regnskapsstøtte"],
  [4, 33_400, "2026-09-09", "2026-09-23", "2026-09-22", "Systemintegrasjon"],
  [0, 58_000, "2026-09-30", "2026-10-14", "2026-10-13", "Utvikling – fase 3"],
  [1, 44_500, "2026-10-12", "2026-10-26", null, "Rådgivning september"],
  [3, 85_000, "2026-10-29", "2026-11-12", null, "Konsulentbistand oktober"],
  [2, 9_600, "2026-11-10", "2026-11-24", null, "Månedlig regnskapsstøtte"],
  [4, 21_000, "2026-11-16", "2026-11-30", null, "Støtte og vedlikehold"],
];
const created = [];
for (const [c, net, invoice_date, due_date, paid, description] of invoices) {
  const inv = await call("POST", "/invoices", {
    party_no: parties[c], invoice_date, due_date,
    lines: [{ description, account: "3000", quantity_milli: 1000, unit_price_ore: net * 100, vat_code: "3" }],
  });
  created.push({ inv, net, paid });
}
console.log("invoice keys", Object.keys(created[0].inv));
const idOf = (i) => i.id ?? i.invoice_id ?? i.invoice?.id;
for (const { inv, net, paid } of created) if (paid) await call("POST", `/invoices/${idOf(inv)}/payments`, { dato: paid, belop_ore: net * 125, konto: "1920" });

// A reminder on the long-overdue January invoice from the base seed.
await tolerate("reminder", async () => {
  const overdue = await call("GET", "/invoices/overdue");
  const list = Array.isArray(overdue) ? overdue : overdue.invoices ?? overdue.items ?? [];
  console.log("overdue", list.length);
});

// --- costs paid from the bank ------------------------------------------------
const cost = (date, description, account, gross) => call("POST", "/vouchers", { journal_code: "GL", date, description, lines: [{ account, amount_ore: gross * 100, vat_code: account === "7770" ? undefined : "1", description }, { account: "1920", amount_ore: -gross * 100 }] });
for (let m = 1; m <= 11; m++) {
  const mm = String(m).padStart(2, "0");
  await cost(`2026-${mm}-01`, "Husleie kontor", "6300", 18_750);
  if (m % 2 === 1) await cost(`2026-${mm}-15`, "Programvarelisenser", "6860", 3_900);
  if (m % 3 === 0) await cost(`2026-${mm}-20`, "Kontorrekvisita", "6800", 1_250);
  await cost(`2026-${mm}-25`, "Mobilabonnement", "6900", 899);
}
await cost("2026-10-22", "Reise til kundemøte Tromsø", "7140", 6_480);
await call("POST", "/vouchers", { journal_code: "GL", date: "2026-11-05", description: "Bankgebyr oktober", lines: [{ account: "7770", amount_ore: 12_500 }, { account: "1920", amount_ore: -12_500 }] });

// --- bank statement (camt.053): some entries match posted vouchers, some do not
const entries = [
  ["CRDT", 48_000 * 125, "2026-02-18", "Betaling faktura"], ["CRDT", 36_500 * 125, "2026-03-04", "Betaling faktura"], ["CRDT", 64_000 * 125, "2026-04-09", "Betaling faktura"],
  ["CRDT", 58_000 * 125, "2026-10-13", "Betaling faktura"], ["CRDT", 33_400 * 125, "2026-09-22", "Betaling faktura"],
  ["DBIT", 18_750 * 100, "2026-10-01", "Husleie kontor"], ["DBIT", 18_750 * 100, "2026-11-01", "Husleie kontor"], ["DBIT", 899 * 100, "2026-10-25", "Mobilabonnement"],
  ["DBIT", 2_150_00, "2026-11-03", "Kortkjøp Elkjøp"], ["CRDT", 3_000_00, "2026-11-09", "Innbetaling uten KID"], ["DBIT", 640_00, "2026-11-12", "Parkering Aker Brygge"],
];
const camt = `<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02"><BkToCstmrStmt><Stmt><Id>DEMO-2026-11</Id><Acct><Id><IBAN>NO9386011117947</IBAN></Id></Acct>
${entries.map(([k, o, d, t]) => `<Ntry><Amt Ccy="NOK">${Math.floor(o / 100)}.${String(o % 100).padStart(2, "0")}</Amt><CdtDbtInd>${k}</CdtDbtInd><Sts>BOOK</Sts><BookgDt><Dt>${d}</Dt></BookgDt><AddtlNtryInf>${t}</AddtlNtryInf></Ntry>`).join("\n")}
</Stmt></BkToCstmrStmt></Document>`;
await tolerate("bank", () => call("POST", "/bank/statements?account=1920", camt, { "content-type": "application/xml" }));

// --- inbox: a supplier invoice with a text layer (bilagstolkning fills the form)
await tolerate("supplier", () => call("POST", "/parties", { kind: "leverandor", name: "Skyhost Nordic AS", orgnr: "923609016", address: "Datavegen 2, 0484 Oslo", email: "faktura@skyhost.example" }));
const lines = ["Faktura 4711", "Skyhost Nordic AS", "Org.nr 923 609 016 MVA", "Fakturadato 12.11.2026", "Forfallsdato 26.11.2026", "Kontonummer 1234.56.78903", "Serverdrift november", "Sum eks. mva 3 200,00", "Mva 25 % 800,00", "A betale 4 000,00"];
// A tiny single-page PDF with a real text layer (what bilagstolkning reads); no compression.
const content = "BT /F1 14 Tf 60 780 Td 18 TL " + lines.map((l) => `(${l}) Tj T*`).join(" ") + " ET";
const objs = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>", `<< /Length ${content.length} >>\nstream\n${content}\nendstream`, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"];
let pdf = "%PDF-1.4\n"; const offs = [];
objs.forEach((o, i) => { offs.push(pdf.length); pdf += `${i + 1} 0 obj\n${o}\nendobj\n`; });
const xref = pdf.length;
pdf += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n` + offs.map((o) => String(o).padStart(10, "0") + " 00000 n \n").join("") + `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF`;
await tolerate("inbox", () => call("POST", "/inbox?filename=skyhost-faktura-4711.pdf", new TextEncoder().encode(pdf), { "content-type": "application/pdf" }));

// --- an integration (agent) ----------------------------------------------
await tolerate("integration", async () => {
  await call("POST", "/integrations", { client_id: "bokforingsagent", navn: "Bokføringsagent", kontakt: "agent@fjordvik.example", access: "bokforing" });
  const list = await call("GET", "/integrations");
  const agent = (Array.isArray(list) ? list : list.integrations ?? []).find((i) => i.client_id === "bokforingsagent");
  for (const [melding, frist] of [["Foreslå kontering for alle dokumenter i innboksen som venter, og legg forslagene i notatet. Ikke bokfør noe.", "2026-11-25"], ["Sjekk at alle leverandørfakturaer i oktober har vedlegg, og lag en liste over dem som mangler.", "2026-11-30"]])
    await call("POST", "/agent/oppgaver", { integration_id: agent?.id ?? agent?.integration_id, melding, frist });
});

// --- a byrå with clients (SQL: creating a verified firm needs the Finanstilsynet register) ---
sql(`
update company set name = 'Fjordvik Konsult AS' where id = '${company}';
with me as (select person_id from company_member where company_id = '${company}' and role = 'admin' limit 1),
f as (insert into firm(id, orgnr, name, kind, autorisasjon_verified_at, autorisasjon_ref) values (gen_random_uuid(), '913400215', 'Nordlys Regnskap AS', 'regnskap', now(), 'FT-demo') returning id)
insert into firm_member(firm_id, person_id, role) select f.id, me.person_id, 'admin' from f, me;
with a as (insert into person(id, oidc_sub, name, email) values (gen_random_uuid(), 'demo|marte', 'Marte Lund', 'marte.lund@nordlysregnskap.example') returning id),
b as (insert into person(id, oidc_sub, name, email) values (gen_random_uuid(), 'demo|jonas', 'Jonas Berg', 'jonas.berg@nordlysregnskap.example') returning id)
insert into firm_member(firm_id, person_id, role) select (select id from firm where orgnr = '913400215'), id, 'ansatt' from (select id from a union all select id from b) x;
`);
const clients = [["Bakeri Hansen AS", "914200117"], ["Trollfjord Turistservice AS", "915300228"], ["Kvalvik Maskin AS", "916400339"], ["Hamar Fysioterapi AS", "917500441"], ["Lofoten Fisk & Ferie AS", "918600552"], ["Sørlandet Rørlegger AS", "919700663"]];
for (const [name, orgnr] of clients)
  sql(`with c as (insert into company(id, orgnr, name) values (gen_random_uuid(), '${orgnr}', '${name}') returning id)
       insert into engagement(id, firm_id, company_id, kind, valid_from) select gen_random_uuid(), (select id from firm where orgnr='913400215'), c.id, 'regnskap', date '2026-01-01' from c;`);
console.log("seeded");
