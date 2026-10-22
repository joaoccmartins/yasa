import type { YearRow } from './schema'

export type SeriesKey = Exclude<keyof YearRow, 'year' | 'label' | 'byAge'>
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
export const seccaoOf = (tipo: number) => SECCOES[tipo - 1]

// The secções whose counts can include age estimates (YearRow.byAge).
export type AgedKey = keyof YearRow['byAge']
export const isAged = (key: SeriesKey): key is AgedKey =>
  key === 'lobitos' || key === 'exploradores' || key === 'pioneiros' || key === 'caminheiros'


// The progresso steps of each secção as SIIE names them, in Scout.steps order: adesão, 1.ª-3.ª
// etapa, anilha de mérito. A scout is at the step of their latest date (Scout.stage).
export const ETAPAS: Record<Tipo, readonly string[]> = {
  1: ['Pata-tenra', 'Lobo Valente', 'Lobo Cortês', 'Lobo Amigo', 'Anilha de Mérito'],
  2: ['Apelo', 'Aliança', 'Rumo', 'Descoberta', 'Anilha de Mérito'],
  3: ['Desprendimento', 'Conhecimento', 'Vontade', 'Construção', 'Anilha de Mérito'],
  4: ['Caminho', 'Comunidade', 'Serviço', 'Partida', 'Anilha de Mérito'],
}
// What each step is, the same in every secção.
export const STEP_KINDS = ['Adesão', '1.ª etapa', '2.ª etapa', '3.ª etapa', null] as const

// The secção of a URL slug (its series key, e.g. 'lobitos'), or null if it isn't one.
export const tipoOfSlug = (slug: string): Tipo | null => TIPOS.find((t) => seccaoOf(t).key === slug) ?? null

// The ano escutista a YYYY-MM-DD date falls in (as the backend's scout_year): October onwards
// starts a new one, so 2024 = 2024/25.
export function scoutYear(iso: string) {
  const year = Number(iso.slice(0, 4))
  return Number(iso.slice(5, 7)) >= 10 ? year : year - 1
}

// What each secção calls its sub-unidades.
export const SUBUNIDADES: Record<Tipo, string> = { 1: 'Bandos', 2: 'Patrulhas', 3: 'Equipas', 4: 'Tribos' }

// "2024/25", as the backend's year_label.
export const yearLabel = (year: number) => `${year}/${String((year + 1) % 100).padStart(2, '0')}`

// How many complete years the forecast line is fitted on, the fewest it needs, and how far ahead
// it goes.
export const FIT_YEARS = 6
const MIN_FIT_YEARS = 3
const FORECAST_YEARS = 3

// TODO(forecast): replace with a cohort flow model: age the current members forward, with pooled
// passagem/saída rates per secção and age from every agrupamento the account holds, plus this
// agrupamento's average entrants. A straight line can't see a big lobitos cohort moving up.
/** The next FORECAST_YEARS after the current one, from a least squares line per secção through
 * the last FIT_YEARS complete years. Empty when there are fewer than MIN_FIT_YEARS of them. */
export function forecast(rows: YearRow[]): YearRow[] {
  // The current year (the last row) is still filling up: censo and passagens happen in autumn.
  const fit = rows.slice(0, -1).slice(-FIT_YEARS)
  if (fit.length < MIN_FIT_YEARS) return []
  const current = rows[rows.length - 1].year

  // Ordinary least squares, count = a + b * year:
  // b = cov(year, count) / var(year), a = mean(count) - b * mean(year).
  const mean = (v: number[]) => v.reduce((s, x) => s + x, 0) / v.length
  const years = fit.map((r) => r.year)
  const my = mean(years)
  const varYear = mean(years.map((y) => (y - my) ** 2))
  const line = (key: SeriesKey) => {
    const counts = fit.map((r) => r[key])
    const mc = mean(counts)
    const b = mean(years.map((y, i) => (y - my) * (counts[i] - mc))) / varYear
    return (year: number) => mc + b * (year - my)
  }
  const lines = SECCOES.map((s) => [s.key, line(s.key)] as const)

  return Array.from({ length: FORECAST_YEARS }, (_, i) => {
    const year = current + 1 + i
    const row: YearRow = {
      year,
      label: yearLabel(year),
      total: 0,
      lobitos: 0,
      exploradores: 0,
      pioneiros: 0,
      caminheiros: 0,
      dirigentes: 0,
      semRegisto: 0,
      byAge: { lobitos: 0, exploradores: 0, pioneiros: 0, caminheiros: 0 },
    }
    // Whole people, never fewer than none. The total is the sum of the rounded secções, so they
    // still add up as in the digest.
    for (const [key, at] of lines) {
      row[key] = Math.max(0, Math.round(at(year)))
      row.total += row[key]
    }
    return row
  })
}
