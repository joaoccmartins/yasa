import type { CSSProperties } from 'react'
import { paths } from '../../../lib/router'
import { Link } from '../../../ui/Link'
import type { Criteria, YearRow } from '../schema'
import { CRITERIA, seccaoOf, TIPOS } from '../series'

/** One card per secção with this year's count, the change on last year and this year's quality
 * criteria; opens the secção. */
export function SeccaoCards({
  code,
  years,
  criteria,
}: {
  code: number
  years: YearRow[]
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
            <CriteriaLines c={c} />
            <span className="seccao-card-go">Ver progresso →</span>
          </Link>
        )
      })}
    </div>
  )
}

/** One year's criteria as "Progresso por registar 15 de 21" lines. */
export function CriteriaLines({ c }: { c: Criteria | undefined }) {
  if (!c || c.members === 0) return null
  return (
    <span className="quality-lines">
      {CRITERIA.map((k) => (
        <Meter key={k.key} label={k.label} value={c[k.key]} of={c.members} />
      ))}
    </span>
  )
}

/** A labelled share of members flagged, as text plus a thin bar (longer is worse). */
export function Meter({ label, value, of }: { label?: string; value: number; of: number }) {
  const pct = of === 0 ? 0 : Math.round((value / of) * 100)
  return (
    <span className="meter">
      <span className="meter-text">
        {label && `${label} `}
        <b>{value}</b> de {of} <span className="muted">({pct}%)</span>
      </span>
      <span className="meter-bar" aria-hidden>
        <span style={{ width: `${pct}%` }} />
      </span>
    </span>
  )
}
