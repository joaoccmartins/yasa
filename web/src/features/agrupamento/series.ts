import type { YearRow } from './schema'

export type SeriesKey = Exclude<keyof YearRow, 'year' | 'label'>
export type Series = { key: SeriesKey; label: string; color: string; dashed?: boolean }

// Secções in the CNE's own colours (yellow, green, blue, red), checked with the data-viz palette
// validator in both themes. Sem registo is a neutral dashed line: missing data, not a group.
export const SECCOES: Series[] = [
  { key: 'lobitos', label: 'Lobitos', color: 'var(--series-lobitos)' },
  { key: 'exploradores', label: 'Exploradores', color: 'var(--series-exploradores)' },
  { key: 'pioneiros', label: 'Pioneiros', color: 'var(--series-pioneiros)' },
  { key: 'caminheiros', label: 'Caminheiros', color: 'var(--series-caminheiros)' },
  { key: 'dirigentes', label: 'Dirigentes', color: 'var(--series-dirigentes)' },
  { key: 'semRegisto', label: 'Sem registo', color: 'var(--ink-muted)', dashed: true },
]
export const TOTAL: Series[] = [{ key: 'total', label: 'Total', color: 'var(--accent)' }]
