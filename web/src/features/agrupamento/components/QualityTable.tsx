import { formatAgrupamento } from '../../../lib/format'
import { paths } from '../../../lib/router'
import { Button } from '../../../ui/Button'
import { Link } from '../../../ui/Link'
import { useQualityList } from '../api'
import { seccaoOf } from '../series'
import { CriteriaOverlay } from './Criteria'

/** Every agrupamento the account holds whole, ranked from worst to best by this year's quality
 * points, with the criteria each secção doesn't meet. */
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
  const { defs, rows } = list.data
  const levelOf = new Map(defs.map((k) => [k.key, k.level]))
  // Worst first: most points; agrupamentos with no members this year (not imported, or defunct)
  // at the end.
  const sorted = [...rows].sort(
    (a, b) => Number(b.members > 0) - Number(a.members > 0) || b.points - a.points || a.code - b.code,
  )
  return (
    <section className="page-section" aria-labelledby="quality-title">
      <div>
        <h2 id="quality-title">Critérios de qualidade</h2>
        <p className="hint">
          Por agrupamento, este ano escutista: cada critério que uma secção não cumpre custa pontos (vermelho 3, amarelo
          2). Do pior para o melhor.
        </p>
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Agrupamento</th>
              <th>Pontos</th>
              <th>Por cumprir</th>
              <th className="num">Elementos</th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((r) => (
              <tr key={r.code}>
                <td className="wrap">
                  <Link to={paths.agrupamento(r.code)}>{formatAgrupamento(r.code, r.name)}</Link>
                </td>
                <td>
                  {r.members > 0 ? (
                    <CriteriaOverlay
                      id={`criteria-${r.code}`}
                      title={`${formatAgrupamento(r.code, r.name)}: critérios por cumprir`}
                      defs={defs}
                      failed={r.failed.map((f) => ({ key: f.key, seccao: seccaoOf(f.tipo).label }))}
                      points={r.points}
                    />
                  ) : (
                    <span className="muted">Sem elementos</span>
                  )}
                </td>
                <td>
                  {/* How many of each level; the overlay (the points) lists them. */}
                  <span className="level-counts">
                    {r.members > 0 &&
                      (['red', 'yellow'] as const)
                        .map((level) => [level, r.failed.filter((f) => levelOf.get(f.key) === level).length] as const)
                        .filter(([, n]) => n > 0)
                        .map(([level, n]) => (
                          <span key={level} className="level" data-level={level}>
                            {n}{' '}
                            {level === 'red' ? (n === 1 ? 'vermelho' : 'vermelhos') : n === 1 ? 'amarelo' : 'amarelos'}
                          </span>
                        ))}
                  </span>
                </td>
                <td className="num">{r.members || '-'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
