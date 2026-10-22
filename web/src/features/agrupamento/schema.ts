import { z } from 'zod'

// One ano escutista (October to September). year 2024 = label "2024/25".
// The secções always add up to total: everyone counts once per year, where they were on Sep 30.
export const yearRowSchema = z.object({
  year: z.number(),
  label: z.string(),
  total: z.number(),
  lobitos: z.number(),
  exploradores: z.number(),
  pioneiros: z.number(),
  caminheiros: z.number(),
  dirigentes: z.number(),
  semRegisto: z.number(),
})
export type YearRow = z.infer<typeof yearRowSchema>

export const digestSchema = z.object({
  // From the SIIE agrupamentos list; null until an import has read it. Dates are YYYY-MM-DD.
  agrupamento: z.object({
    code: z.number(),
    name: z.string().nullable(),
    nucleo: z.number().nullable(),
    nucleoName: z.string().nullable(),
    regiao: z.number().nullable(),
    regiaoName: z.string().nullable(),
    // 'Ativo' / 'Extinto'
    estado: z.string().nullable(),
    // 'Terrestre', 'Marítimo', 'Aéreo'
    tipo: z.string().nullable(),
    paroquia: z.string().nullable(),
    patrono: z.string().nullable(),
    email: z.string().nullable(),
    emailnotif: z.string().nullable(),
    telefone: z.string().nullable(),
    site: z.string().nullable(),
    cp4: z.string().nullable(),
    localidade: z.string().nullable(),
    iniciadoOn: z.string().nullable(),
    filiadoOn: z.string().nullable(),
  }),
  // Oldest first, up to the current year. Empty before the first import.
  years: z.array(yearRowSchema),
  // Unix seconds of the latest finished import; null = never imported.
  lastImportAt: z.number().nullable(),
})
export type Digest = z.infer<typeof digestSchema>

// One scout of a secção (by totem: names never reach YASA). Dates are YYYY-MM-DD.
export const scoutSchema = z.object({
  totem: z.string(),
  categoria: z.string().nullable(),
  // The date of each step in this secção, in STEPS order; null = not reached.
  steps: z.array(z.string().nullable()),
  // 0 = no adesão yet; n = the furthest step reached is STEPS[n - 1].
  stage: z.number(),
  promessaOn: z.string().nullable(),
  // Earned in this secção, newest first.
  especialidades: z.array(z.object({ name: z.string(), on: z.string().nullable() })),
})
export type Scout = z.infer<typeof scoutSchema>

export const seccaoSchema = z.object({
  tipo: z.number(),
  yearLabel: z.string(),
  // First day of this ano escutista: anything on or after it happened "this year".
  yearStart: z.string(),
  scouts: z.array(scoutSchema),
})
export type SeccaoDetail = z.infer<typeof seccaoSchema>
