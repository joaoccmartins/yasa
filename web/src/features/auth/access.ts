import type { AccessLevel } from './schema'

const LABELS: Record<AccessLevel, string> = {
  none: 'Sem acesso',
  agrupamento: 'Agrupamento',
  nucleo: 'Núcleo',
  regiao: 'Região',
  nacional: 'Nacional',
}

export function accessLevelLabel(level: AccessLevel) {
  return LABELS[level]
}
