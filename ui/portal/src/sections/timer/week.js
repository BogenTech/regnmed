// Week arithmetic for the timesheet. Dates are ISO strings computed in
// UTC from a LOCAL "today" (format.js today()) — taking the date from
// toISOString() on a local Date would put Monday 00:30 in Norway on
// Sunday.

const MONTHS = [
  "januar", "februar", "mars", "april", "mai", "juni",
  "juli", "august", "september", "oktober", "november", "desember",
];
const DAYS = ["mandag", "tirsdag", "onsdag", "torsdag", "fredag", "lørdag", "søndag"];

export function addDays(iso, days) {
  const d = new Date(iso + "T00:00:00Z");
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

export function mondayOf(iso) {
  const day = (new Date(iso + "T00:00:00Z").getUTCDay() + 6) % 7;
  return addDays(iso, -day);
}

// ISO 8601 week number: the week holding the year's first Thursday is 1.
export function isoWeek(iso) {
  const thursday = new Date(addDays(mondayOf(iso), 3) + "T00:00:00Z");
  const jan1 = Date.UTC(thursday.getUTCFullYear(), 0, 1);
  return Math.floor((thursday - jan1) / 86_400_000 / 7) + 1;
}

// "Uke 46 · 9.–15. november 2026", spelling out both months or years
// when the week crosses them.
export function weekTitle(monday) {
  const sunday = addDays(monday, 6);
  const [y1, m1, d1] = monday.split("-").map(Number);
  const [y2, m2, d2] = sunday.split("-").map(Number);
  let span;
  if (y1 !== y2) span = `${d1}. ${MONTHS[m1 - 1]} ${y1} – ${d2}. ${MONTHS[m2 - 1]} ${y2}`;
  else if (m1 !== m2) span = `${d1}. ${MONTHS[m1 - 1]} – ${d2}. ${MONTHS[m2 - 1]} ${y2}`;
  else span = `${d1}.–${d2}. ${MONTHS[m2 - 1]} ${y2}`;
  return `Uke ${isoWeek(monday)} · ${span}`;
}

// "mandag 9. november" — what a screen reader says for a cell's day.
export function dayName(iso) {
  const day = (new Date(iso + "T00:00:00Z").getUTCDay() + 6) % 7;
  const [, m, d] = iso.split("-").map(Number);
  return `${DAYS[day]} ${d}. ${MONTHS[m - 1]}`;
}

// "7,5", "7.5" and "7:30" are all valid; an empty cell is zero hours.
// Returns minutes.
export function parseHours(text) {
  const t = String(text).trim();
  if (!t) return 0;
  let minutes;
  if (t.includes(":")) {
    const [h, m] = t.split(":");
    if (Number(m) >= 60) throw new Error("ugyldige timer: " + text);
    minutes = Math.round(Number(h || 0) * 60 + Number(m || 0));
  } else {
    minutes = Math.round(Number(t.replace(/\s/g, "").replace(",", ".")) * 60);
  }
  if (!Number.isFinite(minutes) || minutes < 0) throw new Error("ugyldige timer: " + text);
  if (minutes > 1440) throw new Error("et døgn har 24 timer: " + text);
  return minutes;
}
