// Sections load on demand, so the first download is only the shell and the
// overview. After start-up the rest preload in idle time, so a menu click
// still never waits on the network.

export function lazy(load) {
  const state = $state({ component: null, error: null });
  let pending = null;
  const preload = () =>
    (pending ??= load().then(
      (module) => (state.component = module.default),
      (error) => {
        pending = null; // retry on the next visit
        state.error = error;
      },
    ));
  return {
    get component() {
      preload();
      return state.component;
    },
    get error() {
      return state.error;
    },
    preload,
  };
}

export function preloadWhenIdle(loaders) {
  const run = () => loaders.forEach((l) => l.preload());
  if ("requestIdleCallback" in window) requestIdleCallback(run, { timeout: 3000 });
  else setTimeout(run, 1500);
}
