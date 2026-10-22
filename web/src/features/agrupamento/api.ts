import { useQuery } from '@tanstack/react-query'
import { api } from '../../lib/api-client'
import { digestSchema, seccaoSchema } from './schema'

export const agrupamentoKeys = {
  digest: (code: number) => ['agrupamento', code, 'digest'] as const,
  seccao: (code: number, tipo: number) => ['agrupamento', code, 'seccao', tipo] as const,
}

/** Members per ano escutista of one agrupamento the account holds whole. */
export function useDigest(code: number) {
  return useQuery({
    queryKey: agrupamentoKeys.digest(code),
    queryFn: async () => digestSchema.parse(await api(`/agrupamentos/${code}/digest`)),
  })
}

/** The scouts in one secção this ano escutista, with their progression and especialidades. */
export function useSeccao(code: number, tipo: number) {
  return useQuery({
    queryKey: agrupamentoKeys.seccao(code, tipo),
    queryFn: async () => seccaoSchema.parse(await api(`/agrupamentos/${code}/seccoes/${tipo}`)),
  })
}
