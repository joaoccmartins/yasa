import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../lib/api-client'
import { authKeys } from '../auth/api'
import { accountListSchema, eventListSchema, type InviteInput, type Role, type Status } from './schema'

export const accessKeys = {
  all: ['access'] as const,
  accounts: ['access', 'accounts'] as const,
  events: ['access', 'events'] as const,
}

export function useAccounts() {
  return useQuery({
    queryKey: accessKeys.accounts,
    queryFn: async () => accountListSchema.parse(await api('/admin/accounts')),
  })
}

export function useEvents() {
  return useQuery({
    queryKey: accessKeys.events,
    queryFn: async () => eventListSchema.parse(await api('/admin/events')),
  })
}

export function useInvite() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: InviteInput) =>
      api('/admin/accounts', { method: 'POST', body: JSON.stringify(input) }),
    // Accounts and their history both change.
    onSuccess: () => qc.invalidateQueries({ queryKey: accessKeys.all }),
  })
}

export function useUpdateAccount() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...patch }: { id: string; status?: Status; role?: Role }) =>
      api(`/admin/accounts/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: accessKeys.all })
      // Changing your own role or status changes what you can see.
      qc.invalidateQueries({ queryKey: authKeys.me })
    },
  })
}
