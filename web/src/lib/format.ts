// One fixed date format everywhere (Portuguese style, 24h), so lists scan and compare cleanly.
const dateTime = new Intl.DateTimeFormat('pt-PT', { dateStyle: 'short', timeStyle: 'short' })

export function formatUnixTime(seconds: number | null, fallback = '-') {
  return seconds == null ? fallback : dateTime.format(new Date(seconds * 1000))
}
