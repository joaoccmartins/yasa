import type { CSSProperties } from 'react'
import type { YearRow } from '../schema'
import { seccaoOf, TIPOS, type Tipo } from '../series'

/** One card per secção with this year's count and the change on last year; opens the secção. */
export function SeccaoCards({ years, onOpen }: { years: YearRow[]; onOpen: (tipo: Tipo) => void }) {
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
            <span className="seccao-card-go">Ver progresso →</span>
          </button>
        )
      })}
    </div>
  )
}
