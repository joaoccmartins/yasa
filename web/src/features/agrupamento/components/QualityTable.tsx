import { formatAgrupamento } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { useQualityList } from '../api'
import type { QualityRow } from '../schema'
import { Meter } from './SeccaoCards'

/** Progresso quality of every agrupamento the account holds whole, least registered first, to spot
 * the ones not filling it in. */
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
  // Least adesão registered first; agrupamentos with no active members (not imported, or defunct) at the end.
  const share = (r: QualityRow) => (r.members === 0 ? 2 : r.withAdesao / r.members)
  const rows = [...list.data].sort((a, b) => share(a) - share(b) || a.code - b.code)
  return (
    <section className="page-section" aria-labelledby="quality-title">
      <div>
        <h2 id="quality-title">Progresso registado</h2>
        <p className="hint">
          Por agrupamento, este ano escutista: quantos elementos das secções têm a adesão à sua secção registada no
          SIIE, e quantos dos que já não são aspirantes têm a promessa registada.
        </p>
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Agrupamento</th>
              <th className="num">Elementos</th>
              <th>Adesão</th>
              <th>Promessa</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.code}>
                <td className="wrap">{formatAgrupamento(r.code, r.name)}</td>
                <td className="num">{r.members || '-'}</td>
                <td>{r.members > 0 ? <Meter value={r.withAdesao} of={r.members} /> : <span className="muted">Sem elementos ativos</span>}</td>
                <td>{r.promessaDue > 0 ? <Meter value={r.withPromessa} of={r.promessaDue} /> : '-'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
