import { useLayoutEffect, useRef, type CSSProperties } from 'react'
import type { Criteria } from '../schema'
import { CRITERIA } from '../series'

/** A secção's quality criteria per ano escutista as bars: one row per criterion, each bar the share
 * of the secção flagged that year, with the count on top. The last 5 years fit the width (see
 * .crit-grid); older ones are a scroll away to the left. */
export function CriteriaChart({ years }: { years: Criteria[] }) {
  const scroll = useRef<HTMLDivElement>(null)
  // Oldest to newest, left to right: start scrolled to the newest.
  useLayoutEffect(() => {
    if (scroll.current) scroll.current.scrollLeft = scroll.current.scrollWidth
  }, [years])
  return (
    <section className="panel" aria-labelledby="criteria-title">
      <h2 id="criteria-title">Critérios de qualidade</h2>
      <p className="hint">
        Por ano escutista, a parte dos elementos da secção que falha cada critério (a barra) e quantos são (o número).
        Os últimos 5 anos; deslize para a esquerda para ver os anteriores.
      </p>
      <div className="crit">
        <div className="crit-labels">
          {CRITERIA.map((k) => (
            <div key={k.key}>
              <b>{k.label}</b>
              <span className="sub">{k.hint}</span>
            </div>
          ))}
          <div className="sub">Ano · elementos</div>
        </div>
        <div className="crit-scroll" ref={scroll} tabIndex={0} role="region" aria-label="Critérios por ano escutista">
          <div className="crit-grid" style={{ '--cols': years.length } as CSSProperties}>
            {CRITERIA.map((k) =>
              years.map((c) => {
                const pct = c.members === 0 ? 0 : Math.round((c[k.key] / c.members) * 100)
                return (
                  <div key={`${k.key}-${c.year}`} className="crit-cell" title={`${c.label}: ${c[k.key]} de ${c.members} (${pct}%)`}>
                    <span className="crit-value" data-zero={c[k.key] === 0 || undefined}>
                      {c[k.key]}
                    </span>
                    {/* Room is left for the number on top: a fully flagged year fills 80%. */}
                    <span className="crit-bar" style={{ height: `${pct * 0.8}%` }} />
                  </div>
                )
              }),
            )}
            {years.map((c) => (
              <div key={c.year} className="crit-year">
                {/* "2025/26", or "25/26" on a phone, where the columns are narrow. */}
                <b>
                  <span className="crit-century">{c.label.slice(0, 2)}</span>
                  {c.label.slice(2)}
                </b>
                <span className="sub">{c.members}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}
