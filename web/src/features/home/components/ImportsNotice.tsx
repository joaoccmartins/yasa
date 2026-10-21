import { formatCode, formatUnixTime } from '../../../lib/format'
import type { ImportStatus, MemberImport } from '../../auth/schema'

const STATUS: Record<ImportStatus, string> = {
  queued: 'em fila',
  running: 'a decorrer',
  done: 'concluída',
  failed: 'falhou',
}

/**
 * Member imports of the agrupamentos this account sees, one agrupamento at a time. Shown to
 * everyone who sees the agrupamento, not only to whoever's login started the import, since the
 * data is changing for all of them.
 */
export function ImportsNotice({ imports }: { imports: MemberImport[] }) {
  if (imports.length === 0) return null
  const active = imports.filter((i) => i.status === 'queued' || i.status === 'running')
  const failed = imports.filter((i) => i.status === 'failed')
  const done = imports.filter((i) => i.status === 'done')
  // Failures usually share one cause: show each distinct message once.
  const errors = [...new Set(failed.map((i) => i.error ?? 'erro desconhecido.'))]
  // Totals over the finished imports: elementos seen, field changes, exits.
  const sum = (key: 'members' | 'changes' | 'exits') =>
    imports.reduce((n, i) => n + (i.status === 'done' ? (i[key] ?? 0) : 0), 0)
  return (
    <section className="panel" aria-labelledby="imports-title">
      <h2 id="imports-title">Importação de elementos</h2>
      {active.length > 0 ? (
        <p className="notice" role="status">
          A importar os elementos do SIIE, um agrupamento de cada vez:{' '}
          {imports.length - active.length} de {imports.length} concluídos. Até terminar, os dados
          destes agrupamentos podem estar incompletos. Esta página atualiza-se sozinha.
        </p>
      ) : (
        done.length > 0 && (
          <p className="hint" role="status">
            Importação concluída: {sum('members')} elementos, {sum('changes')} alterações,{' '}
            {sum('exits')} saídas.
          </p>
        )
      )}
      {failed.length > 0 && (
        <div className="notice" data-tone="danger" role="status">
          <p>
            {failed.length === 1 ? '1 agrupamento falhou' : `${failed.length} agrupamentos falharam`}
            . Será repetido no próximo início de sessão.
          </p>
          <ul>
            {errors.map((e) => (
              <li key={e}>{e}</li>
            ))}
          </ul>
        </div>
      )}
      <details>
        <summary>Agrupamentos ({imports.length})</summary>
        <ul className="codes">
          {imports.map((i) => (
            <li key={i.agrupamento}>{describe(i)}</li>
          ))}
        </ul>
      </details>
    </section>
  )
}

/** "0455 · Nome · concluída 21/10/26 10:02: 243 elementos, 3 alterações, 1 saída". */
function describe(i: MemberImport) {
  const head = [formatCode('agrupamento', i.agrupamento), i.name, STATUS[i.status]]
    .filter(Boolean)
    .join(' · ')
  switch (i.status) {
    case 'done':
      return `${head} ${formatUnixTime(i.finishedAt)}: ${i.members ?? 0} elementos, ${i.changes ?? 0} alterações, ${i.exits ?? 0} saídas`
    case 'failed':
      return `${head}: ${i.error ?? 'erro desconhecido.'}`
    default:
      return head
  }
}
