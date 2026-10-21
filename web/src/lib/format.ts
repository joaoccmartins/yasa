// One fixed date format everywhere (Portuguese style, 24h), so lists scan and compare cleanly.
const dateTime = new Intl.DateTimeFormat('pt-PT', { dateStyle: 'short', timeStyle: 'short' })

export function formatUnixTime(seconds: number | null, fallback = '-') {
  return seconds == null ? fallback : dateTime.format(new Date(seconds * 1000))
}

// SIIE shows codes zero-padded: região "05", núcleo "0505", agrupamento "0053". The API sends
// plain numbers; padding is display only.
const CODE_DIGITS = { regiao: 2, nucleo: 4, agrupamento: 4 } as const

export function formatCode(kind: keyof typeof CODE_DIGITS, code: number) {
  return String(code).padStart(CODE_DIGITS[kind], '0')
}

const date = new Intl.DateTimeFormat('pt-PT', { dateStyle: 'short' })

export function formatUnixDate(seconds: number) {
  return date.format(new Date(seconds * 1000))
}

// A YYYY-MM-DD date from the API, shown as dd/mm/yyyy without going through the time zone.
export function formatIsoDate(iso: string) {
  const [y, m, d] = iso.split('-')
  return `${d}/${m}/${y}`
}
