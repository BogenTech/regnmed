// Meldingenes levende forbindelse (docs/meldinger.md).
//
// One server-sent-event stream per signed-in person, shared by the
// navbar badge and the open conversation. An event says only "look
// now"; both then fetch through the ordinary guarded endpoints. fetch,
// not EventSource, because EventSource cannot send the Authorization
// header.

import { api, post, request } from "./api.js";
import { toast } from "./toast.svelte.js";

// `varsler` is the bell's list (docs/meldinger.md §7.2): one item per
// conversation with unread messages. Derived on the server from the read
// marks, so the bell, the menu badge and the conversation list cannot
// disagree -- `unread` is the same list, summed.
export const chat = $state({ tick: 0, unread: 0, varsler: [] });

let abort = null;
let hilst = false;

export async function refreshUnread() {
  try {
    // `?? []`: a reply without the list (an older server) must not reach
    // a template as undefined -- an uncaught throw there freezes the page.
    chat.varsler = (await api("/notifications"))?.varsler ?? [];
    chat.unread = chat.varsler.reduce((sum, v) => sum + (v.uleste || 0), 0);
  } catch {
    // The badge is a convenience; the conversation view says what is wrong.
    return;
  }
  // Once per sign-in: say what is waiting, without making anyone look.
  if (!hilst) {
    hilst = true;
    if (chat.unread > 0) {
      toast(
        chat.unread === 1
          ? "Du har én ulest melding. Se bjella øverst."
          : "Du har " + chat.unread + " uleste meldinger. Se bjella øverst.",
        true,
      );
    }
  }
}

/** The bell's «Marker alle som lest». The tick makes an open Meldinger
 * reload its list, so its badges go with the bell's. */
export async function markAllRead() {
  try {
    await post("/chat/read-all", {});
    chat.tick += 1;
    await refreshUnread();
  } catch (error) {
    toast(error.message, false);
  }
}

function bump() {
  chat.tick += 1;
  refreshUnread();
}

async function run(signal) {
  let wait = 1000;
  while (!signal.aborted) {
    try {
      const response = await request("/chat/stream", {
        signal,
        headers: { accept: "text/event-stream" },
      });
      wait = 1000;
      // Open again: anything said while we were away.
      bump();
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let end;
        while ((end = buffer.indexOf("\n\n")) >= 0) {
          const block = buffer.slice(0, end);
          buffer = buffer.slice(end + 2);
          if (block.split("\n").includes("event: message")) bump();
        }
      }
    } catch {
      if (signal.aborted) return;
    }
    // Server gone (a deploy): back off, but keep trying -- the backstop
    // poll in the conversation view covers the gap.
    await new Promise((r) => setTimeout(r, wait));
    wait = Math.min(wait * 2, 30_000);
  }
}

export function connect() {
  if (abort) return;
  abort = new AbortController();
  run(abort.signal);
}

export function disconnect() {
  abort?.abort();
  abort = null;
}
