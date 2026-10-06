import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../lib/api-client'
import { meSchema, type LoginInput, type Me } from './schema'

export const authKeys = { me: ['auth', 'me'] as const }

/** The logged-in user, or null when logged out (401 is an answer here, not an error). */
export function useMe() {
  return useQuery({
    queryKey: authKeys.me,
    queryFn: async (): Promise<Me | null> => {
      try {
        return meSchema.parse(await api('/auth/me'))
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) return null
        throw e
      }
    },
    retry: false,
    staleTime: Infinity,
  })
}

export function useLogin() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: LoginInput) =>
      api('/auth/login', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: authKeys.me }),
  })
}

export function useLogout() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => api('/auth/logout', { method: 'POST' }),
    // Drop every cached query so no data from this session survives in memory,
    // then refetch "me", which now answers null and shows the login page.
    onSuccess: () => qc.resetQueries(),
  })
}
