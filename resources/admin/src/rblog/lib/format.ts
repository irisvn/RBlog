// Dates in the reader's language (docs/ui-guidelines.md §9), on `Intl`.

type When = string | number | Date | null | undefined;

function parse(at: When): Date | null {
  if (at == null || at === "") return null;
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** A moment in full: date, time and time zone (tooltips). */
export function dateTime(at: When, locale: string): string {
  const date = parse(at);
  if (!date) return "";
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  }).format(date);
}

/** A day: `Oct 24, 2026` (`24 thg 10, 2026` in Vietnamese). */
export function date(at: When, locale: string): string {
  const day = parse(at);
  return day ? new Intl.DateTimeFormat(locale, { year: "numeric", month: "short", day: "numeric" }).format(day) : "";
}

/** A moment relative to now: `2 hours ago`, `in 5 days`. */
export function relative(at: When, locale: string, now = Date.now()): string {
  const day = parse(at);
  if (!day) return "";
  const seconds = Math.round((day.getTime() - now) / 1000);
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const steps: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ["year", 31536000],
    ["month", 2592000],
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, length] of steps) {
    if (Math.abs(seconds) >= length) return format.format(Math.trunc(seconds / length), unit);
  }
  return format.format(seconds, "second");
}
