// Number formatting through Intl, so units and separators follow the language (spec section 13).

const UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const;

/** A size in bytes in the largest sensible unit, for example "54.9 GB". */
export function formatBytes(bytes: number, locale = 'en'): string {
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: UNITS[unit],
    unitDisplay: 'short',
    maximumFractionDigits: unit >= 3 ? 1 : 0,
  }).format(value);
}
