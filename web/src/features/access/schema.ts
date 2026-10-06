import { z } from 'zod'
import { accessLevelSchema, type LoginMethod } from '../auth/schema'

export const accountSchema = z.object({
  id: z.string(),
  label: z.string(),
  role: z.enum(['member', 'admin']),
  status: z.enum(['active', 'disabled']),
  hasNif: z.boolean(),
  hasNin: z.boolean(),
  // Unix seconds.
  createdAt: z.number(),
  lastLoginAt: z.number().nullable(),
  accessLevel: accessLevelSchema,
  // Counts only; the lists themselves are in /auth/me for the account's own session.
  agrupamentos: z.number(),
  nucleos: z.number(),
  regioes: z.number(),
})
export const accountListSchema = z.array(accountSchema)
export type Account = z.infer<typeof accountSchema>
export type Role = Account['role']
export type Status = Account['status']

export type InviteInput = {
  method: LoginMethod
  identifier: string
  label: string
  role: Role
}
