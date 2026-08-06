// Section icons — hand-rolled 24×24 outline SVG, vendored like
// themes.css: no icon library, no CDN, nothing fetched at runtime (the
// portal is embedded in the binary and frugality-gated). Each icon is
// stored ONCE as inner SVG markup; the selectable "ikonstil" only
// changes how it is rendered (stroke width, emoji fallback, or off), so
// another style costs bytes in Ikon.svelte, not another path set.
//
// The assignment section → icon is fixed on purpose: the point of the
// icons is instant recognition, and that dies if every installation
// shuffles them. The STYLE is a platform setting locked globally by
// systemadmin (migration 0053, picker in the platform console); the
// portal reads it from /portal-config at boot.

export const IKONSTILER = [
  ["linje", "Linje"],
  ["kraftig", "Kraftig"],
  ["emoji", "Emoji"],
  ["ingen", "Ingen ikoner"],
];

export const IKONER = {
  oversikt: {
    emoji: "🏠",
    d: '<rect x="4" y="4" width="6.5" height="6.5" rx="1"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1"/>',
  },
  faktura: {
    emoji: "🧾",
    d: '<path d="M6 3h8l5 5v13H6z"/><path d="M14 3v5h5"/><path d="M9 13h7M9 17h7"/>',
  },
  kunder: {
    emoji: "👥",
    d: '<circle cx="9" cy="8" r="3.25"/><path d="M3.5 20a5.5 5.5 0 0 1 11 0"/><path d="M16 5.1a3.25 3.25 0 0 1 0 5.8"/><path d="M17.5 14.6a5.5 5.5 0 0 1 3 4.9"/>',
  },
  produkter: {
    emoji: "📦",
    d: '<path d="M12 3l9 4.5v9L12 21l-9-4.5v-9z"/><path d="M3 7.5l9 4.5 9-4.5"/><path d="M12 12v9"/>',
  },
  prosjekter: {
    emoji: "📁",
    d: '<path d="M3 7V5.5A1.5 1.5 0 0 1 4.5 4h4l2 2.5h9A1.5 1.5 0 0 1 21 8v10a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z"/>',
  },
  timer: {
    emoji: "⏱️",
    d: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>',
  },
  lonn: {
    emoji: "💰",
    d: '<rect x="2.5" y="6" width="19" height="12" rx="1.5"/><circle cx="12" cy="12" r="2.5"/><path d="M6 12h.01M18 12h.01"/>',
  },
  utlegg: {
    emoji: "🧾",
    d: '<path d="M6 21V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v17l-2-1.4-2 1.4-2-1.4-2 1.4-2-1.4z"/><path d="M9.5 8h5M9.5 12h5"/>',
  },
  anlegg: {
    emoji: "🏭",
    d: '<path d="M3 21V8l5.5 3.5V8L14 11.5V4h7v17z"/><path d="M3 21h18"/>',
  },
  aksjonarer: {
    emoji: "🥧",
    d: '<path d="M21 12A9 9 0 1 1 12 3v9z"/><path d="M15 3.5A9 9 0 0 1 20.5 9H15z"/>',
  },
  reskontro: {
    emoji: "📒",
    d: '<path d="M4 19.5V4.5A2.5 2.5 0 0 1 6.5 2H20v15H6.5A2.5 2.5 0 0 0 4 19.5 2.5 2.5 0 0 0 6.5 22H20"/>',
  },
  mva: {
    emoji: "🧮",
    d: '<path d="M19 5L5 19"/><circle cx="6.75" cy="6.75" r="2.25"/><circle cx="17.25" cy="17.25" r="2.25"/>',
  },
  rapporter: {
    emoji: "📊",
    d: '<path d="M3 3v17a1 1 0 0 0 1 1h17"/><path d="M8 17v-5M13 17V8M18 17v-8"/>',
  },
  bank: {
    emoji: "🏦",
    d: '<path d="M3 9.5L12 4l9 5.5"/><path d="M4.5 10v7M9.5 10v7M14.5 10v7M19.5 10v7"/><path d="M3 20.5h18"/>',
  },
  bilag: {
    emoji: "📥",
    d: '<path d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><path d="M2 12h6l2 3h4l2-3h6"/>',
  },
  periode: {
    emoji: "📅",
    d: '<rect x="3" y="4.5" width="18" height="16" rx="2"/><path d="M3 9.5h18M8 2.5v4M16 2.5v4"/>',
  },
  admin: {
    emoji: "⚙️",
    d: '<path d="M4 6h9M19 6h1M4 12h3M11 12h9M4 18h9M19 18h1"/><circle cx="16" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="16" cy="18" r="2"/>',
  },
  brukere: {
    emoji: "👤",
    d: '<circle cx="12" cy="8" r="3.5"/><path d="M5 20.5a7 7 0 0 1 14 0"/>',
  },
  oppdrag: {
    emoji: "💼",
    d: '<rect x="3" y="7.5" width="18" height="13" rx="2"/><path d="M9 7.5V6a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v1.5"/><path d="M3 13h18"/>',
  },
  // Plattformkonsollen (docs/auth.md §8).
  selskaper: {
    emoji: "🏢",
    d: '<rect x="5" y="3" width="14" height="18"/><path d="M9 7h.01M15 7h.01M9 11h.01M15 11h.01M9 15h.01M15 15h.01"/><path d="M10 21v-3h4v3"/>',
  },
  byraer: {
    emoji: "🏛️",
    d: '<rect x="3" y="7.5" width="18" height="13" rx="2"/><path d="M9 7.5V6a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v1.5"/><path d="M3 13h18"/>',
  },
  abonnementer: {
    emoji: "💳",
    d: '<rect x="2.5" y="5" width="19" height="14" rx="2"/><path d="M2.5 10h19"/><path d="M6 15h4"/>',
  },
  medlemmer: {
    emoji: "🛡️",
    d: '<path d="M12 3l7 3v5c0 4.5-3 8.5-7 10-4-1.5-7-5.5-7-10V6z"/>',
  },
};
