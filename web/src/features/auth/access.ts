import type { AccessLevel, Seccao } from './schema'

const LEVELS: Record<AccessLevel, string> = {
  none: 'Sem acesso',
  seccao: 'Secção',
  agrupamento: 'Agrupamento',
  nucleo: 'Núcleo',
  regiao: 'Região',
  nacional: 'Nacional',
}

export function accessLevelLabel(level: AccessLevel) {
  return LEVELS[level]
}

const SECCAO_TIPOS: Record<number, string> = {
  1: 'Alcateia (Lobitos)',
  2: 'Expedição (Exploradores)',
  3: 'Comunidade (Pioneiros)',
  4: 'Clã (Caminheiros)',
}

/** "Alcateia (Lobitos)", or "Alcateia (Lobitos) 2" for the rare second one of a tipo. */
export function seccaoLabel(s: Seccao) {
  const name = SECCAO_TIPOS[s.tipo] ?? `Secção ${s.tipo}`
  return s.numero > 1 ? `${name} ${s.numero}` : name
}
