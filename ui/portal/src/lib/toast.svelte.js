// Varsler øverst til høyre — samme rolle som toast() i app.js, men som
// reaktiv liste rendret av Toasts.svelte.

export const toasts = $state([]);

let nextId = 1;

export function toast(message, ok) {
  const id = nextId++;
  toasts.push({ id, message, ok });
  setTimeout(() => {
    const i = toasts.findIndex((t) => t.id === id);
    if (i >= 0) toasts.splice(i, 1);
  }, 5000);
}
