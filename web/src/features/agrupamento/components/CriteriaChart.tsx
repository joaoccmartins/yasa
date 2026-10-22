import { useLayoutEffect, useRef, type CSSProperties } from 'react'
import type { Criteria, Criterion } from '../schema'
import { LevelBadge } from './Criteria'

// Row heights: a member criterion's bars, a secção criterion's flag, the years.
const ROW = { members: '120px', seccao: '76px', years: '44px' }

/** A secção's quality criteria per ano escutista: one row per criterion (from the API), oldest
 * year on the left. Member criteria are bars (the share of the secção with the count on top, and a
 * line at the limit): grey while met, in the level's colour once failed. Secção criteria say
 * "falha" or "cumpre". The last 5 years fit the width (see .crit-grid); older ones are a scroll
 * away to the left. */
export function CriteriaChart({ defs, years }: { defs: Criterion[]; years: Criteria[] }) {
  const scroll = useRef<HTMLDivElement>(null)
  // Start scrolled to the newest year.
  useLayoutEffect(() => {
    if (scroll.current) scroll.current.scrollLeft = scroll.current.scrollWidth
  }, [years])
  const rows = { gridTemplateRows: [...defs.map((k) => ROW[k.scope]), ROW.years].join(' ') }
  return (
    <section className="panel" aria-labelledby="criteria-title">
      <h2 id="criteria-title">Critérios de qualidade</h2>
      <p className="hint">
        Por ano escutista. Nos critérios de elementos, a barra é a parte da secção em falta e a linha o limite: acima
        dele, a secção não cumpre o critério (a barra ganha a cor do nível). Nos da secção, diz se cumpriu. Cada critério
        por cumprir custa pontos: vermelho 3, amarelo 2. Os últimos 5 anos; deslize para a esquerda para ver os
        anteriores.
      </p>
      <div className="crit">
        <div className="crit-labels" style={rows}>
          {defs.map((k) => (
            <div key={k.key}>
              <b>{k.label}</b> <LevelBadge level={k.level} />
              <span className="sub">{k.hint}</span>
            </div>
          ))}
          <div className="sub">Ano · elementos</div>
        </div>
        <div className="crit-scroll" ref={scroll} tabIndex={0} role="region" aria-label="Critérios por ano escutista">
          <div className="crit-grid" style={{ ...rows, '--cols': years.length } as CSSProperties}>
            {defs.map((k) =>
              years.map((c) => {
                const v = c.flags[k.key] ?? 0
                if (k.scope === 'seccao') {
                  return (
                    <div key={`${k.key}-${c.year}`} className="crit-cell" title={`${c.label}: ${v ? 'falha' : 'cumpre'}`}>
                      {v > 0 ? (
                        <span className="crit-mark" data-level={k.level}>
                          falha
                        </span>
                      ) : (
                        <span className="crit-value" data-zero>
                          cumpre
                        </span>
                      )}
                    </div>
                  )
                }
                const pct = c.members === 0 ? 0 : Math.round((v / c.members) * 100)
                const failed = c.failed.includes(k.key)
                return (
                  <div
                    key={`${k.key}-${c.year}`}
                    className="crit-cell"
                    title={`${c.label}: ${v} de ${c.members} (${pct}%)${failed ? ', não cumpre' : ''}`}
                  >
                    <span className="crit-value" data-zero={v === 0 || undefined}>
                      {v}
                    </span>
                    {/* Room is left for the number on top: a fully flagged year fills 80%. */}
                    <span
                      className="crit-bar"
                      data-level={k.level}
                      data-failed={failed || undefined}
                      style={{ height: `${pct * 0.8}%` }}
                    />
                    {k.maxShare != null && <i className="crit-limit" style={{ bottom: `${k.maxShare * 80}%` }} />}
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
