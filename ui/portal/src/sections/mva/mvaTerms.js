// VAT periods in words. A term is named by its months ("Jan–feb"), the
// page opens on the latest term that has ENDED (the one being filed, not
// the one still running), and a deadline is said as a distance from
// today. Dates are ISO strings ("2026-04-10"); today comes from
// lib/format.js today(), so it is the local date.
const MONTHS = ["jan", "feb", "mar", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "des"];
const cap = (s) => s[0].toUpperCase() + s.slice(1);

export function termName(count, n) {
  if (count === 1) return "Hele året";
  const per = 12 / count;
  const first = (n - 1) * per;
  return per === 1 ? cap(MONTHS[first]) : cap(MONTHS[first]) + "–" + MONTHS[first + per - 1];
}

export function defaultTerm(count, today) {
  const [y, m] = today.split("-").map(Number);
  if (count === 1) return { year: y - 1, termin: 1 };
  const current = Math.floor((m - 1) / (12 / count)) + 1;
  return current === 1 ? { year: y - 1, termin: count } : { year: y, termin: current - 1 };
}

const day = (iso) => Date.UTC(...iso.split("-").map((v, i) => Number(v) - (i === 1 ? 1 : 0)));

export function deadlineText(deadline, today) {
  const days = Math.round((day(deadline) - day(today)) / 86_400_000);
  if (days === 0) return "i dag";
  if (days === 1) return "i morgen";
  if (days > 1) return "om " + days + " dager";
  return days === -1 ? "passert i går" : "passert for " + -days + " dager siden";
}
