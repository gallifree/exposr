import i18n from "../i18n";

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatBytes(n: number, perSecond = false): string {
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const num = new Intl.NumberFormat(i18n.language, { maximumFractionDigits: v < 10 && i > 0 ? 1 : 0 }).format(v);
  return `${num} ${UNITS[i]}${perSecond ? "/s" : ""}`;
}

export function formatNumber(n: number): string {
  return new Intl.NumberFormat(i18n.language).format(n);
}

const STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["second", 60],
  ["minute", 60],
  ["hour", 24],
  ["day", 30],
  ["month", 12],
  ["year", Infinity],
];

export function timeAgo(iso: string | null | undefined): string {
  if (!iso) return "—";
  let delta = (new Date(iso).getTime() - Date.now()) / 1000;
  const rtf = new Intl.RelativeTimeFormat(i18n.language, { numeric: "auto" });
  for (const [unit, size] of STEPS) {
    if (Math.abs(delta) < size) return rtf.format(Math.round(delta), unit);
    delta /= size;
  }
  return "—";
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  return new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));
}

export function formatDuration(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  return `${m}m`;
}
