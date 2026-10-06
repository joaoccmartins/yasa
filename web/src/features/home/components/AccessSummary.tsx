import { accessLevelLabel } from '../../auth/access'
import type { Me } from '../../auth/schema'

/** What SIIE showed at the last login: the level and what it was read from. */
export function AccessSummary({ me }: { me: Me }) {
  const lists = [
    { label: 'Regiões', codes: me.regioes },
    { label: 'Núcleos', codes: me.nucleos },
    { label: 'Agrupamentos', codes: me.agrupamentos },
  ]
  return (
    <section className="panel summary" aria-labelledby="summary-title">
      <h2 id="summary-title">O seu acesso no SIIE</h2>
      <div className="tiles">
        <Tile label="Nível" value={accessLevelLabel(me.accessLevel)} />
        <Tile label="Agrupamentos" value={me.agrupamentos.length} />
        <Tile label="Núcleos" value={me.nucleos.length} />
        <Tile label="Regiões" value={me.regioes.length} />
      </div>
      <p className="hint">
        Lido no último início de sessão a partir da grelha de elementos do SIIE. O nível resulta
        de quantas regiões, núcleos e agrupamentos aparecem nela.
      </p>
      {lists.map(
        (l) =>
          l.codes.length > 0 && (
            <details key={l.label}>
              <summary>
                {l.label} ({l.codes.length})
              </summary>
              <ul className="codes">
                {l.codes.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
            </details>
          ),
      )}
    </section>
  )
}

function Tile({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="tile">
      <span className="tile-label">{label}</span>
      <span className="tile-value">{value}</span>
    </div>
  )
}
