// Hash-ruting (#/c/{id}/{seksjon}) — adressene er uendret fra den
// rammeverksfrie portalen, så gamle bokmerker virker fortsatt.
//
// Spørringen (#/c/{id}/mva?year=&termin=) hører med til adressen: den
// gjør en bestemt termin delbar. Den leses HER, ett sted, så ingen
// seksjon trenger sin egen hashchange-lytter.

import { harUlagret, kanForlate, slippVakter as vakterSlipp } from "./ulagret.js";

function parse() {
  const hash = location.hash.replace(/^#\/?/, "");
  const [sti, query = ""] = hash.split("?");
  const parts = sti.split("/");
  if (parts[0] === "byra" && parts[1]) {
    return { view: "byra", firmId: parts[1], query };
  }
  // Meldinger lives outside any company: a conversation is between
  // people, and a byrå's people talk across all their clients.
  if (parts[0] === "meldinger") {
    return { view: "meldinger", conversationId: parts[1] || null, query };
  }
  if (parts[0] === "plattform") {
    return { view: "plattform", query };
  }
  if (parts[0] === "c" && parts[1]) {
    return {
      view: "company",
      companyId: parts[1],
      section: parts[2] || "oversikt",
      extra: parts[3] || null,
      query,
    };
  }
  return { view: "companies", query };
}

export const route = $state(parse());

// Back/Forward (and any address change no link click caught) cannot be
// stopped before it happens, so with unsaved input the address is put
// back first and the question asked after.
window.addEventListener("hashchange", async (e) => {
  if (harUlagret()) {
    history.replaceState(history.state, "", e.oldURL);
    if (!(await kanForlate())) return;
    vakterSlipp();
    location.href = e.newURL;
    return;
  }
  Object.assign(
    route,
    { firmId: null, companyId: null, conversationId: null, section: null, extra: null, query: "" },
    parse(),
  );
});
