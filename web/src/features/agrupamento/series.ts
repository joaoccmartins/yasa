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
export const seccaoOf = (tipo: Tipo) => SECCOES[tipo - 1]

// The secções whose counts can include age estimates (YearRow.byAge).
export type AgedKey = keyof YearRow['byAge']
export const isAged = (key: SeriesKey): key is AgedKey =>
  key === 'lobitos' || key === 'exploradores' || key === 'pioneiros' || key === 'caminheiros'

// Quality criteria (see Criteria in schema.ts): flags of how well an agrupamento keeps SIIE up
// to date. Higher is worse.
export const CRITERIA = [
  {
    key: 'progresso',
    label: 'Progresso por registar',
    hint: 'Fizeram a promessa, mas não têm a adesão à secção concluída no SIIE.',
  },
  {
    key: 'promessa',
    label: 'Promessa por registar',
    hint: 'Concluíram a adesão há mais de um ano escutista, ou já não são aspirantes, mas não têm a promessa no SIIE.',
  },
] as const

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
