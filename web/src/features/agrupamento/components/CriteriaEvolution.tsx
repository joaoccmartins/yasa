import { useLayoutEffect, useRef, type CSSProperties } from 'react'
import type { Criteria, Criterion } from '../schema'
import { seccaoOf } from '../series'
import { LevelBadge } from './Criteria'

// Row heights: the points bars, one row per criterion, the years.
const ROW = { points: '120px', criterion: '64px', years: '36px' }

/** The agrupamento's quality criteria over the years, all secções together. First its points per
 * ano escutista (red and yellow parts stacked), then one row per criterion with the secções that
 * failed it that year. The last 5 years fit the width; older ones are a scroll away to the left. */
export function CriteriaEvolution({
  defs,
  criteria,
}: {
  defs: Criterion[]
  criteria: { tipo: number; years: Criteria[] }[]
}) {
  const scroll = useRef<HTMLDivElement>(null)
  // Every year any secção has, oldest first.
  const years = [...new Set(criteria.flatMap((s) => s.years.map((y) => y.year)))].sort((a, b) => a - b)
  // Start scrolled to the newest year.
  useLayoutEffect(() => {
    if (scroll.current) scroll.current.scrollLeft = scroll.current.scrollWidth
  }, [years.length])
  if (years.length === 0) return null

  const levelOf = new Map(defs.map((k) => [k.key, k]))
  // Per year: each secção's criteria that year, and the points split by level.
  const columns = years.map((year) => {
    const seccoes = criteria.flatMap((s) => {
      const c = s.years.find((y) => y.year === year)
      return c ? [{ tipo: s.tipo, c }] : []
    })
    const pointsOf = (level: string) =>
      seccoes.reduce(
        (sum, { c }) =>
          sum +
          c.failed
            .filter((key) => levelOf.get(key)?.level === level)
            .reduce((p, key) => p + (levelOf.get(key)?.points ?? 0), 0),
        0,
      )
    const red = pointsOf('red')
    const yellow = pointsOf('yellow')
    return { year, label: seccoes[0]?.c.label ?? String(year), seccoes, red, yellow, total: red + yellow }
  })
  const max = Math.max(1, ...columns.map((c) => c.total))
  const rows = { gridTemplateRows: [ROW.points, ...defs.map(() => ROW.criterion), ROW.years].join(' ') }

  return (
    <section className="panel" aria-labelledby="evolution-title">
      <h2 id="evolution-title">Critérios ao longo dos anos</h2>
      <p className="hint">
        Os pontos do agrupamento em cada ano escutista (cada critério que uma secção não cumpre custa pontos: vermelho
        3, amarelo 2) e, por critério, as secções que não o cumpriram. Os últimos 5 anos; deslize para a esquerda para
        ver os anteriores.
      </p>
      <div className="crit">
        <div className="crit-labels" style={rows}>
          <div>
            <b>Pontos</b>
            <span className="sub">Quanto mais, pior.</span>
          </div>
          {defs.map((k) => (
            <div key={k.key} title={k.hint}>
              <b>{k.label}</b> <LevelBadge level={k.level} />
            </div>
          ))}
          <div className="sub">Ano</div>
        </div>
        <div className="crit-scroll" ref={scroll} tabIndex={0} role="region" aria-label="Critérios por ano escutista">
          <div className="crit-grid" style={{ ...rows, '--cols': columns.length } as CSSProperties}>
            {columns.map((c) => (
              <div key={`p-${c.year}`} className="crit-cell" title={`${c.label}: ${c.total} pontos`}>
                <span className="crit-value" data-zero={c.total === 0 || undefined}>
                  {c.total}
                </span>
                {/* Stacked: red points on the bottom, yellow on top; the tallest year fills 80%. */}
                <span className="crit-stack" style={{ height: `${(c.total / max) * 80}%` }}>
                  <span data-level="yellow" style={{ flexGrow: c.yellow }} />
                  <span data-level="red" style={{ flexGrow: c.red }} />
                </span>
              </div>
            ))}
            {defs.map((k) =>
              columns.map((c) => {
                const failing = c.seccoes.filter(({ c: sc }) => sc.failed.includes(k.key))
                return (
                  <div key={`${k.key}-${c.year}`} className="crit-cell crit-seccoes">
                    {failing.length === 0 ? (
                      <span className="crit-value" data-zero>
                        <span className="crit-long">cumpre</span>
                        <span className="crit-short" aria-label="cumpre">
                          ·
                        </span>
                      </span>
                    ) : (
                      failing.map(({ tipo }) => (
                        <span
                          key={tipo}
                          className="seccao-dot"
                          style={{ '--seccao': seccaoOf(tipo).color } as CSSProperties}
                          title={seccaoOf(tipo).label}
                        >
                          {/* The full name, or its initial on a phone where columns are narrow. */}
                          <span className="crit-long">{seccaoOf(tipo).label}</span>
                          <span className="crit-short">{seccaoOf(tipo).label[0]}</span>
                        </span>
                      ))
                    )}
                  </div>
                )
              }),
            )}
            {columns.map((c) => (
              <div key={`y-${c.year}`} className="crit-year">
                <b>
                  <span className="crit-century">{c.label.slice(0, 2)}</span>
                  {c.label.slice(2)}
                </b>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}
