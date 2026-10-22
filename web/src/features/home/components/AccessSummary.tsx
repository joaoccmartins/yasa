import { formatAgrupamento, formatCode, formatUnixDate, formatUnixTime } from '../../../lib/format'
import { accessLevelLabel, seccaoLabel } from '../../auth/access'
import type { Access } from '../../auth/schema'

/** The person's access as last read from SIIE: status, level, and what it covers. */
export function AccessSummary({ access }: { access: Access }) {
  // Nothing read yet: the tiles would only show zeros, so they wait for the first read.
  const hasRead = access.refreshedAt != null
  return (
    <section className="panel summary" aria-labelledby="summary-title">
      <h2 id="summary-title">O seu acesso no SIIE</h2>
      <StatusNotice access={access} />
      {hasRead ? (
        <>
          <div className="tiles">
            <Tile label="Nível" value={accessLevelLabel(access.level)} />
            <Tile label="Agrupamentos" value={access.agrupamentos.length} />
            <Tile label="Núcleos" value={access.nucleos.length} />
            <Tile label="Regiões" value={access.regioes.length} />
          </div>
          <CodeLists access={access} />
        </>
      ) : (
        access.status === 'running' && (
          <div className="tiles" aria-hidden>
            {Array.from({ length: 4 }, (_, i) => (
              <div key={i} className="tile">
                <span className="skeleton" />
                <span className="skeleton tall" />
              </div>
            ))}
          </div>
        )
      )}
    </section>
  )
}

/** One line on where the access stands. role="status" so screen readers hear the change. */
function StatusNotice({ access }: { access: Access }) {
  const read = access.refreshedAt != null
  switch (access.status) {
    case 'running':
      return (
        <p className="notice" role="status">
          {read
            ? 'A atualizar o seu acesso a partir do SIIE… Até terminar, vale o acesso anterior.'
            : 'O seu acesso está a ser processado a partir do SIIE. Pode demorar um pouco; esta página atualiza-se sozinha.'}
        </p>
      )
    case 'failed':
      return (
        <p className="notice" data-tone="danger" role="status">
          Não foi possível ler o seu acesso no SIIE: {access.error ?? 'erro desconhecido.'} Volte a
          iniciar sessão para tentar novamente.
          {read && ' Até lá, vale o acesso anterior.'}
        </p>
      )
    case 'expired':
      return (
        <p className="hint">
          Lido em {formatUnixTime(access.refreshedAt)}. Expirou e vai ser atualizado no próximo
          início de sessão.
        </p>
      )
    case 'ready':
      return (
        <p className="hint">
          Lido em {formatUnixTime(access.refreshedAt)}, válido até{' '}
          {access.validUntil != null ? formatUnixDate(access.validUntil) : '-'}. O nível resulta de
          quantas regiões, núcleos e agrupamentos aparecem na grelha do SIIE.
        </p>
      )
    case 'never':
      return (
        <p className="hint">O seu acesso vai ser lido do SIIE no próximo início de sessão.</p>
      )
  }
}

function CodeLists({ access }: { access: Access }) {
  const lists = [
    { label: 'Regiões', items: access.regioes.map((r) => formatCode('regiao', r)) },
    { label: 'Núcleos', items: access.nucleos.map((n) => formatCode('nucleo', n)) },
    {
      label: 'Agrupamentos',
      items: access.agrupamentos.map((a) =>
        [formatAgrupamento(a.code, a.name), a.seccao && seccaoLabel(a.seccao)]
          .filter(Boolean)
          .join(' · '),
      ),
    },
  ]
  return lists.map(
    (l) =>
      l.items.length > 0 && (
        <details key={l.label}>
          <summary>
            {l.label} ({l.items.length})
          </summary>
          <ul className="codes">
            {l.items.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </details>
      ),
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
