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
  // Of each secção's count, how many were placed by age (no dated record that year): estimates.
  byAge: z.object({
    lobitos: z.number(),
    exploradores: z.number(),
    pioneiros: z.number(),
    caminheiros: z.number(),
  }),
})
export type YearRow = z.infer<typeof yearRowSchema>

// Quality criteria come from the API (src/criteria.rs), so new ones need no UI change. Each is a
// condition a secção meets or fails in a year; failing costs its level's points (red 3, yellow 2,
// green 1), and an agrupamento's points are the sum over its secções: more is worse.
// scope: 'members' has a number (members of the secção that break a member check) and fails when
// more than maxShare of the secção do; 'seccao' is about the secção as a whole (1 fails, 0 meets).
export const levelSchema = z.enum(['green', 'yellow', 'red'])
export type Level = z.infer<typeof levelSchema>
export const criterionSchema = z.object({
  key: z.string(),
  label: z.string(),
  hint: z.string(),
  level: levelSchema,
  points: z.number(),
  scope: z.enum(['members', 'seccao']),
  // members scope: the share (0-1) above which the secção fails; null for 'seccao'.
  maxShare: z.number().nullable(),
})
export type Criterion = z.infer<typeof criterionSchema>

// One secção's criteria in one ano escutista: flags[key] per criterion, as above.
export const criteriaSchema = z.object({
  year: z.number(),
  label: z.string(),
  members: z.number(),
  flags: z.record(z.string(), z.number()),
  // The keys of the criteria the secção fails that year, and the points they cost.
  failed: z.array(z.string()),
  points: z.number(),
})
export type Criteria = z.infer<typeof criteriaSchema>

// Each agrupamento the account holds whole, this year: its points (all secções' failed criteria)
// and which criterion each secção fails.
export const qualityRowSchema = z.object({
  code: z.number(),
  name: z.string().nullable(),
  members: z.number(),
  points: z.number(),
  failed: z.array(z.object({ tipo: z.number(), key: z.string() })),
})
export type QualityRow = z.infer<typeof qualityRowSchema>
export const qualityListSchema = z.object({ defs: z.array(criterionSchema), rows: z.array(qualityRowSchema) })

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
  // Former members with no date at all in SIIE (old records): not in any year.
  undated: z.number(),
  // Per secção (tipo 1 Lobitos ... 4 Caminheiros), the years it had members, oldest first.
  criteriaDefs: z.array(criterionSchema),
  criteria: z.array(z.object({ tipo: z.number(), years: z.array(criteriaSchema) })),
  // Unix seconds of the latest finished import; null = never imported.
  lastImportAt: z.number().nullable(),
})
export type Digest = z.infer<typeof digestSchema>

// One scout of a secção (by totem: names never reach YASA). Dates are YYYY-MM-DD.
export const scoutSchema = z.object({
  // Opaque id for the member page URL (never a NIN).
  ref: z.string(),
  // Whole years today; null without a birth month.
  age: z.number().nullable(),
  totem: z.string(),
  categoria: z.string().nullable(),
  // The date of each step in this secção: adesão, 1.ª-3.ª etapa, anilha de mérito (see ETAPAS).
  steps: z.array(z.string().nullable()),
  // The step they're at: the index of their latest date, 0 (adesão) when there's none.
  stage: z.number(),
  promessaOn: z.string().nullable(),
  // Earned in this secção, newest first.
  especialidades: z.array(z.object({ name: z.string(), on: z.string().nullable() })),
})
export type Scout = z.infer<typeof scoutSchema>

// An adult tied to the secção by an active chefe / chefe adjunto cargo there.
export const adultSchema = z.object({
  ref: z.string(),
  age: z.number().nullable(),
  totem: z.string(),
  // Dirigente, Aspirante a Dirigente, Noviço a Dirigente...
  categoria: z.string().nullable(),
  // e.g. "Chefe de Alcateia"
  cargos: z.string(),
  // Instrutor of the secção: the ones counted per sub-unidade (chefes and adjuntos aren't).
  instrutor: z.boolean(),
})
export type Adult = z.infer<typeof adultSchema>

export const seccaoSchema = z.object({
  tipo: z.number(),
  yearLabel: z.string(),
  // First day of this ano escutista: anything on or after it happened "this year".
  yearStart: z.string(),
  // Sub-unidades (bandos, patrulhas, equipas, tribos) as their sizes. estimated: SIIE doesn't
  // have them, so they're worked out from the number of elementos (4-8 each, 5-8 for lobitos,
  // as close to 6 as the numbers allow).
  subunidades: z.object({ sizes: z.array(z.number()), estimated: z.boolean() }),
  scouts: z.array(scoutSchema),
  adults: z.array(adultSchema),
  // This secção's criteria per year, oldest first.
  criteriaDefs: z.array(criterionSchema),
  criteria: z.array(criteriaSchema),
})
export type SeccaoDetail = z.infer<typeof seccaoSchema>

// One member's page: who they are and their history, year by year. Dates are YYYY-MM-DD.
const date = z.string().nullable()
export const bucketSchema = z.enum(['lobitos', 'exploradores', 'pioneiros', 'caminheiros', 'dirigentes', 'semRegisto'])
export const memberSchema = z.object({
  ref: z.string(),
  age: z.number().nullable(),
  info: z.object({
    totem: z.string(),
    categoria: z.string().nullable(),
    seccao: z.string().nullable(),
    situacao: z.string().nullable(),
    present: z.boolean(),
    admittedOn: date,
    promisedOn: date,
    // Per secção (tipo 1-4 in order): the date of each step (ETAPAS order), then the promessa.
    progresso: z.array(z.array(date)),
    especialidades: z.array(z.object({ tipo: z.number(), name: z.string(), on: date })),
    cargos: z.array(
      z.object({
        cargo: z.string().nullable(),
        // The secção it belongs to (tipo 1-4), from its sigla or descrição; null = none known.
        seccaoTipo: z.number().nullable(),
        agrupamento: z.number().nullable(),
        startedOn: date,
        endedOn: date,
        active: z.boolean().nullable(),
      }),
    ),
    cursos: z.array(z.object({ curso: z.string().nullable(), on: date })),
    // Per ano escutista (its first year): activities, nights, nights in the field.
    atividades: z.array(z.object({ year: z.number(), count: z.number(), noites: z.number(), noitesCampo: z.number() })),
  }),
  // The secção they were counted in each year, as in the digest; byAge = estimated by age.
  years: z.array(z.object({ year: z.number(), label: z.string(), bucket: bucketSchema, byAge: z.boolean() })),
})
export type Member = z.infer<typeof memberSchema>
