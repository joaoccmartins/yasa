import type { CSSProperties } from 'react'
import { paths } from '../../../lib/router'
import { Link } from '../../../ui/Link'
import type { Criteria, Criterion, YearRow } from '../schema'
import { seccaoOf, TIPOS } from '../series'
import { CriteriaLines } from './Criteria'

/** One card per secção with this year's count, the change on last year and this year's quality
 * criteria; opens the secção. */
export function SeccaoCards({
  code,
  years,
  defs,
  criteria,
}: {
  code: number
  years: YearRow[]
  defs: Criterion[]
  criteria: { tipo: number; years: Criteria[] }[]
}) {
  const current = years.at(-1)
  const previous = years.at(-2)
  if (!current) return null
  return (
    <div className="seccao-cards">
      {TIPOS.map((tipo) => {
        const s = seccaoOf(tipo)
        const now = current[s.key]
        // Change on the previous ano escutista; none for the first year on record.
        const delta = previous ? now - previous[s.key] : null
        const c = criteria.find((c) => c.tipo === tipo)?.years.find((y) => y.year === current.year)
        return (
          <Link key={tipo} to={paths.seccao(code, s.key)} className="seccao-card" style={{ '--seccao': s.color } as CSSProperties}>
            <span className="seccao-card-name">{s.label}</span>
            <span className="tile-value">{now}</span>
            <span className="seccao-card-meta">
              {previous == null || delta == null
                ? `em ${current.label}`
                : delta === 0
                  ? `igual a ${previous.label}`
                  : `${delta > 0 ? '+' : ''}${delta} face a ${previous.label}`}
            </span>
            <CriteriaLines defs={defs} c={c} />
            <span className="seccao-card-go">Ver progresso →</span>
          </Link>
        )
      })}
    </div>
  )
}
