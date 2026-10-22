import type { CSSProperties } from 'react'
import type { Criteria } from '../schema'
import { CRITERIA } from '../series'

/** A secção's quality criteria per ano escutista, newest first: one row per criterion, each cell
 * the members flagged, tinted by their share (stronger is worse). */
export function CriteriaMatrix({ years }: { years: Criteria[] }) {
  const cols = [...years].reverse()
  return (
    <section className="panel" aria-labelledby="criteria-title">
      <h2 id="criteria-title">Critérios de qualidade</h2>
      <p className="hint">
        Por ano escutista, quantos dos elementos da secção falham cada critério. Quanto mais forte a cor, maior a parte
        da secção.
      </p>
      <div className="table-wrap">
        <table className="table criteria">
          <thead>
            <tr>
              <th>Critério</th>
              {cols.map((c) => (
                <th key={c.year} className="num">
                  {c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>Elementos</td>
              {cols.map((c) => (
                <td key={c.year} className="num">
                  {c.members}
                </td>
              ))}
            </tr>
            {CRITERIA.map((k) => (
              <tr key={k.key}>
                <td title={k.hint}>
                  {k.label}
                  <span className="sub">{k.hint}</span>
                </td>
                {cols.map((c) => {
                  const share = c.members === 0 ? 0 : c[k.key] / c.members
                  return (
                    <td
                      key={c.year}
                      className="num"
                      title={`${c[k.key]} de ${c.members}`}
                      // Up to a 45% tint of --danger for a fully flagged secção.
                      style={{ '--flag': `${Math.round(share * 45)}%` } as CSSProperties}
                      data-flag={c[k.key] > 0 || undefined}
                    >
                      {c[k.key]}
                    </td>
                  )
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
