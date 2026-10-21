import { z } from 'zod'
import {
  accessLevelSchema,
  accessStatusSchema,
  importStatusSchema,
  type LoginMethod,
} from '../auth/schema'

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
  accessStatus: accessStatusSchema,
  // Unix seconds; null until the first successful read.
  accessValidUntil: z.number().nullable(),
  // What the grants cover. The lists themselves are in /auth/me, for the account's own session.
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

export const eventSchema = z.object({
  id: z.number(),
  // Unix seconds.
  at: z.number(),
  action: z.enum([
    'invited',
    'login',
    'enabled',
    'disabled',
    'role_changed',
    'identifier_linked',
    'access_refreshed',
    'access_failed',
  ]),
  // Small facts per action (role names, counts); read defensively in the event log.
  detail: z.record(z.string(), z.unknown()),
  accountId: z.string(),
  accountLabel: z.string(),
  // null = command line or the system.
  actorLabel: z.string().nullable(),
})
export const eventListSchema = z.array(eventSchema)
export type AccountEvent = z.infer<typeof eventSchema>

// One member import for the Access page history. Counts only, never members.
export const importLogSchema = z.object({
  id: z.number(),
  agrupamento: z.number(),
  agrupamentoName: z.string().nullable(),
  // Whose login queued it; null = that account was deleted.
  accountLabel: z.string().nullable(),
  status: importStatusSchema,
  // Unix seconds.
  queuedAt: z.number(),
  finishedAt: z.number().nullable(),
  membersSeen: z.number().nullable(),
  changes: z.number().nullable(),
  exits: z.number().nullable(),
  error: z.string().nullable(),
  warning: z.string().nullable(),
})
export const importLogListSchema = z.array(importLogSchema)
export type ImportLogRow = z.infer<typeof importLogSchema>
