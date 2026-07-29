// Temavalg — temakontrakten med regnid: samme daisyUI-temanavn velger
// samme temablokker (ui/themes.css er en kopi av regnids kanoniske fil).
// Preferansen er per side: bare localStorage, aldri via IdP eller token.

const KEY = "regnmed-theme";
const CYCLE = ["system", "light", "dark"];

export const THEMES = [
  "system", "light", "dark", "regnid", "kontrast",
  "nord", "corporate", "business", "dim", "sunset",
];

export const ICON = { system: "🖥️", light: "☀️", dark: "🌙" };

function stored() {
  try {
    return localStorage.getItem(KEY);
  } catch (e) {
    return null;
  }
}

function apply(mode) {
  const root = document.documentElement;
  if (!mode || mode === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", mode);
  }
}

export const theme = $state({ current: stored() || "system" });

export function setTheme(mode) {
  theme.current = mode;
  try {
    localStorage.setItem(KEY, mode);
  } catch (e) {
    /* privat modus */
  }
  apply(mode);
}

export function cycleTheme() {
  setTheme(CYCLE[(CYCLE.indexOf(theme.current) + 1) % CYCLE.length]);
}
