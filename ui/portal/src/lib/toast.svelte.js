// Varsler øverst til høyre, rendret av Toasts.svelte.
//
// A confirmation goes by itself after a few seconds. An error stays
// until it is closed: our error messages say what to do next, and five
// seconds is not enough to read one. At most MAX are shown; the oldest
// goes first.

export const toasts = $state([]);

const MAX = 4;
let nextId = 1;

export function toast(message, ok) {
  const id = nextId++;
  toasts.push({ id, message, ok });
  if (toasts.length > MAX) toasts.splice(0, toasts.length - MAX);
  if (ok) setTimeout(() => closeToast(id), 5000);
}

export function closeToast(id) {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}
