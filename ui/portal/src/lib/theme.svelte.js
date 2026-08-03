// Temavalg — temakontrakten med regnid: samme daisyUI-temanavn velger
// samme temablokker (ui/themes.css er en kopi av regnids kanoniske fil).
// Preferansen er per side: bare localStorage, aldri via IdP eller token.
//
// Listene under må stemme med `themes: all` i app.css. De er gruppert
// etter daisyUIs egen `color-scheme` på hver temablokk — ikke etter
// skjønn — så en bruker som vil ha et mørkt tema slipper å gjette.
// Rekkefølgen innenfor hver gruppe er daisyUIs egen.

const KEY = "regnmed-theme";
const CYCLE = ["system", "light", "dark"];

// Våre egne temaer (ui/portal/themes.css). Står først fordi de er
// husstilen; resten er daisyUIs innebygde.
const EGNE = ["regnid", "kontrast"];

const LYSE = [
  "light", "cupcake", "bumblebee", "emerald", "corporate", "retro",
  "cyberpunk", "valentine", "garden", "lofi", "pastel", "fantasy",
  "wireframe", "cmyk", "autumn", "acid", "lemonade", "winter", "nord",
  "caramellatte", "silk",
];

const MORKE = [
  "dark", "synthwave", "halloween", "forest", "aqua", "black", "luxury",
  "dracula", "business", "night", "coffee", "dim", "sunset", "abyss",
];

// "system" er ikke et tema, men fraværet av ett: attributtet fjernes og
// daisyUI lar OS-preferansen avgjøre (light, eller dark når OS-et sier
// dark). Derfor står det alene, ikke i en fargegruppe.
export const THEME_GROUPS = [
  { label: "Følg systemet", themes: ["system"] },
  { label: "Egne", themes: EGNE },
  { label: "Lyse", themes: LYSE },
  { label: "Mørke", themes: MORKE },
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
