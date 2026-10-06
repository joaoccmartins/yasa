import { z } from 'zod'

// How far someone's SIIE access reaches, narrowest to widest (see siie::Visibility::level).
export const accessLevelSchema = z.enum(['none', 'agrupamento', 'nucleo', 'regiao', 'nacional'])
export type AccessLevel = z.infer<typeof accessLevelSchema>

export const meSchema = z.object({
  accountId: z.string(),
  label: z.string(),
  role: z.enum(['member', 'admin']),
  // As SIIE showed it at the last login.
  accessLevel: accessLevelSchema,
  // Codes of what the SIIE grid showed. The level is read from how widely these spread.
  agrupamentos: z.array(z.string()),
  nucleos: z.array(z.string()),
  regioes: z.array(z.string()),
})
export type Me = z.infer<typeof meSchema>

// Same choice as the SIIE login page: Loja Escutista takes the NIF, SIIE takes the NIN.
export type LoginMethod = 'loja' | 'siie'

export type LoginInput = {
  identifier: string
  password: string
  method: LoginMethod
}
