import { z } from 'zod'

// How far someone's SIIE access reaches, narrowest to widest (see siie::Visibility::level).
export const accessLevelSchema = z.enum(['none', 'seccao', 'agrupamento', 'nucleo', 'regiao', 'nacional'])
export type AccessLevel = z.infer<typeof accessLevelSchema>

// never: not read yet · running: being read now · failed: last read failed
// expired: older than the validity period (re-read at next login) · ready: in force
export const accessStatusSchema = z.enum(['never', 'running', 'failed', 'expired', 'ready'])
export type AccessStatus = z.infer<typeof accessStatusSchema>

// tipo: 1 Alcateia, 2 Expedição, 3 Comunidade, 4 Clã; numero tells apart two of one tipo.
export const seccaoSchema = z.object({ tipo: z.number(), numero: z.number() })
export type Seccao = z.infer<typeof seccaoSchema>

export const grantSchema = z.object({
  code: z.number(),
  name: z.string().nullable(),
  nucleo: z.number().nullable(),
  regiao: z.number().nullable(),
  // null = the whole agrupamento
  seccao: seccaoSchema.nullable(),
})
export type GrantedAgrupamento = z.infer<typeof grantSchema>

export const accessSchema = z.object({
  level: accessLevelSchema,
  status: accessStatusSchema,
  // Unix seconds.
  refreshedAt: z.number().nullable(),
  validUntil: z.number().nullable(),
  error: z.string().nullable(),
  agrupamentos: z.array(grantSchema),
  nucleos: z.array(z.number()),
  regioes: z.array(z.number()),
})
export type Access = z.infer<typeof accessSchema>

export const meSchema = z.object({
  accountId: z.string(),
  label: z.string(),
  role: z.enum(['member', 'admin']),
  access: accessSchema,
})
export type Me = z.infer<typeof meSchema>

// Same choice as the SIIE login page: Loja Escutista takes the NIF, SIIE takes the NIN.
export type LoginMethod = 'loja' | 'siie'

export type LoginInput = {
  identifier: string
  password: string
  method: LoginMethod
}
