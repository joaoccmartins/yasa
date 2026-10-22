import { formatAgrupamento } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { useQualityList } from '../api'
import type { QualityRow } from '../schema'
import { CRITERIA } from '../series'
import { Meter } from './SeccaoCards'

/** This year's quality criteria of every agrupamento the account holds whole, worst first, to spot
 * the ones not keeping SIIE up to date. */
export function QualityTable() {
  const list = useQualityList()
  if (list.isPending) {
    return (
      <div className="panel" aria-busy>
        <span className="skeleton" />
      </div>
    )
  }
  if (list.isError) {
    return (
      <div className="empty">
        <h2>Não foi possível carregar o progresso</h2>
        <p>{list.error.message}</p>
        <Button onClick={() => list.refetch()}>Tentar novamente</Button>
      </div>
    )
  }
  // Most flagged (both criteria, as a share of members) first; agrupamentos with no members this
  // year (not imported, or defunct) at the end.
  const share = (r: QualityRow) => (r.members === 0 ? -1 : (r.progresso + r.promessa) / r.members)
  const rows = [...list.data].sort((a, b) => share(b) - share(a) || a.code - b.code)
  return (
    <section className="page-section" aria-labelledby="quality-title">
      <div>
        <h2 id="quality-title">Critérios de qualidade</h2>
        <p className="hint">
          Por agrupamento, este ano escutista: quantos elementos das secções falham cada critério. Os piores primeiro.
        </p>
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Agrupamento</th>
              <th className="num">Elementos</th>
              {CRITERIA.map((k) => (
                <th key={k.key} title={k.hint}>
                  {k.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.code}>
                <td className="wrap">{formatAgrupamento(r.code, r.name)}</td>
                <td className="num">{r.members || '-'}</td>
                {CRITERIA.map((k) => (
                  <td key={k.key}>
                    {r.members > 0 ? <Meter value={r[k.key]} of={r.members} /> : <span className="muted">Sem elementos</span>}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
