import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../lib/api-client'
import { authKeys } from '../auth/api'
import { accountListSchema, type InviteInput, type Role, type Status } from './schema'

export const accessKeys = { accounts: ['access', 'accounts'] as const }

export function useAccounts() {
  return useQuery({
    queryKey: accessKeys.accounts,
    queryFn: async () => accountListSchema.parse(await api('/admin/accounts')),
  })
}

export function useInvite() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: InviteInput) =>
      api('/admin/accounts', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: accessKeys.accounts }),
  })
}

export function useUpdateAccount() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...patch }: { id: string; status?: Status; role?: Role }) =>
      api(`/admin/accounts/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: accessKeys.accounts })
      // Changing your own role or status changes what you can see.
      qc.invalidateQueries({ queryKey: authKeys.me })
    },
  })
}
