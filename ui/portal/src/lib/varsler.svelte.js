// Varslingssenteret (docs/agent.md §8.5): det som venter på DEG, på
// tvers av selskapene du når.
//
// Derived on the server, never stored: an agent question is listed while
// its task waits for a person and disappears when someone answers. So
// there is no read-state to keep in sync, and the bell, the menu marker
// and the task list can never disagree. Fetched at sign-in, on every chat
// event, and once a minute while the tab is visible.

import { api } from "./api.js";
import { toast } from "./toast.svelte.js";

export const varsler = $state({ liste: [], lastet: false });

let hilst = false;

export async function hentVarsler() {
  try {
    // `?? []`: a reply without the list (an older server) must not reach
    // a template as undefined -- an uncaught throw there freezes the page.
    varsler.liste = (await api("/notifications"))?.varsler ?? [];
    varsler.lastet = true;
  } catch {
    // The bell is a convenience; the task list says what is wrong.
    return;
  }
  // Once per sign-in: say what is waiting, without making anyone look.
  if (!hilst) {
    hilst = true;
    const n = varsler.liste.length;
    if (n > 0) {
      toast(
        n === 1
          ? "En agent venter på svar fra deg. Se bjella øverst."
          : n + " spørsmål fra agenter venter på svar. Se bjella øverst.",
        true,
      );
    }
  }
}

/** Questions waiting in one company -- the menu and tab marker. */
export function ventendeSporsmal(companyId) {
  return varsler.liste.filter((v) => v.company_id === companyId).length;
}
