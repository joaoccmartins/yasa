import type { LoginMethod } from './schema'

// Which identifier each SIIE login option takes. Shared by the login and invite forms.
export const LOGIN_METHODS: { value: LoginMethod; label: string; idLabel: string }[] = [
  { value: 'loja', label: 'Loja Escutista', idLabel: 'NIF' },
  { value: 'siie', label: 'SIIE', idLabel: 'NIN' },
]

export function idLabelFor(method: LoginMethod) {
  return LOGIN_METHODS.find((m) => m.value === method)!.idLabel
}
