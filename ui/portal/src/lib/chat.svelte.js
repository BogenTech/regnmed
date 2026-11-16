// Meldingenes levende forbindelse (docs/meldinger.md).
//
// One server-sent-event stream per signed-in person, shared by the
// navbar badge and the open conversation. An event says only "look
// now"; both then fetch through the ordinary guarded endpoints. fetch,
// not EventSource, because EventSource cannot send the Authorization
// header.

import { api, request } from "./api.js";

export const chat = $state({ tick: 0, unread: 0 });

let abort = null;

export async function refreshUnread() {
  try {
    chat.unread = (await api("/chat/unread")).unread;
  } catch {
    // The badge is a convenience; the conversation view says what is wrong.
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
