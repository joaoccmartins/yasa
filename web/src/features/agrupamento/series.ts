import type { YearRow } from './schema'

export type SeriesKey = Exclude<keyof YearRow, 'year' | 'label'>
export type Series = { key: SeriesKey; label: string; color: string; dashed?: boolean }

// The four secções take the palette's four colours (see tokens.css). Dirigentes and Sem registo
// stay neutral; Sem registo is dashed: missing data, not a group.
export const SECCOES: Series[] = [
  { key: 'lobitos', label: 'Lobitos', color: 'var(--seccao-lobitos)' },
  { key: 'exploradores', label: 'Exploradores', color: 'var(--seccao-exploradores)' },
  { key: 'pioneiros', label: 'Pioneiros', color: 'var(--seccao-pioneiros)' },
  { key: 'caminheiros', label: 'Caminheiros', color: 'var(--seccao-caminheiros)' },
  { key: 'dirigentes', label: 'Dirigentes', color: 'var(--seccao-dirigentes)' },
  { key: 'semRegisto', label: 'Sem registo', color: 'var(--seccao-sem-registo)', dashed: true },
]
export const TOTAL: Series[] = [{ key: 'total', label: 'Total', color: 'var(--accent)' }]

// The secções that have a detail view: tipo as the API takes it (1 Lobitos ... 4 Caminheiros).
export const TIPOS = [1, 2, 3, 4] as const
export type Tipo = (typeof TIPOS)[number]
export const seccaoOf = (tipo: Tipo) => SECCOES[tipo - 1]
