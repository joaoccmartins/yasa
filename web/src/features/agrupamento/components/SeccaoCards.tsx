import type { CSSProperties } from 'react'
import type { Quality, YearRow } from '../schema'
import { seccaoOf, TIPOS, type Tipo } from '../series'

/** One card per secção with this year's count, the change on last year and how much of its
 * progresso is registered; opens the secção. */
export function SeccaoCards({
  years,
  quality,
  onOpen,
}: {
  years: YearRow[]
  quality: Quality[]
  onOpen: (tipo: Tipo) => void
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
        return (
          <button key={tipo} className="seccao-card" style={{ '--seccao': s.color } as CSSProperties} onClick={() => onOpen(tipo)}>
            <span className="seccao-card-name">{s.label}</span>
            <span className="tile-value">{now}</span>
            <span className="seccao-card-meta">
              {previous == null || delta == null
                ? `em ${current.label}`
                : delta === 0
                  ? `igual a ${previous.label}`
                  : `${delta > 0 ? '+' : ''}${delta} face a ${previous.label}`}
            </span>
            <QualityLines q={quality[tipo - 1]} />
            <span className="seccao-card-go">Ver progresso →</span>
          </button>
        )
      })}
    </div>
  )
}

/** "Adesão registada 3 de 21" and "Promessa registada 0 de 18", for this year's members. */
export function QualityLines({ q }: { q: Quality | undefined }) {
  if (!q || q.members === 0) return null
  return (
    <span className="quality-lines">
      <Meter label="Adesão registada" value={q.withAdesao} of={q.members} />
      {q.promessaDue > 0 && <Meter label="Promessa registada" value={q.withPromessa} of={q.promessaDue} />}
    </span>
  )
}

/** A labelled share as text plus a thin bar, e.g. "Adesão registada 3 de 21". */
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
