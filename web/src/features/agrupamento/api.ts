import { useQuery } from '@tanstack/react-query'
import { api } from '../../lib/api-client'
import { digestSchema } from './schema'

export const agrupamentoKeys = {
  digest: (code: number) => ['agrupamento', code, 'digest'] as const,
}

/** Members per ano escutista of one agrupamento the account holds whole. */
export function useDigest(code: number) {
  return useQuery({
    queryKey: agrupamentoKeys.digest(code),
    queryFn: async () => digestSchema.parse(await api(`/agrupamentos/${code}/digest`)),
  })
}
