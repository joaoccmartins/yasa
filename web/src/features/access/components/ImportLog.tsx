import { formatCode, formatUnixTime } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { useImports } from '../api'
import type { ImportLogRow } from '../schema'

const SKELETON_ROWS = 4

const STATUS: Record<ImportLogRow['status'], string> = {
  queued: 'Em fila',
  running: 'A decorrer',
  done: 'Concluída',
  failed: 'Falhou',
}

/** Member imports, newest first: which agrupamento, whose login started it, and what it found. */
export function ImportLog() {
  const imports = useImports()
  return (
    <section className="page-section" aria-labelledby="imports-log-title">
      <div>
        <h2 id="imports-log-title">Importações de elementos</h2>
        <p className="hint">
          As últimas 100, uma por agrupamento. Só contagens: elementos vistos, alterações e saídas.
        </p>
      </div>
      {imports.isError ? (
        <div className="empty">
          <h2>Não foi possível carregar as importações</h2>
          <p>{imports.error.message}</p>
          <Button onClick={() => imports.refetch()}>Tentar novamente</Button>
        </div>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Quando</th>
                <th>Agrupamento</th>
                <th>Iniciada por</th>
                <th>Estado</th>
              </tr>
            </thead>
            <tbody>
              {imports.isPending ? (
                Array.from({ length: SKELETON_ROWS }, (_, i) => (
                  <tr key={i} aria-hidden>
                    <td colSpan={4}>
                      <span className="skeleton" />
                    </td>
                  </tr>
                ))
              ) : imports.data.length === 0 ? (
                <tr>
                  <td colSpan={4} className="muted">
                    Ainda não há importações.
                  </td>
                </tr>
              ) : (
                imports.data.map((i) => (
                  <tr key={i.id}>
                    <td className="when">{formatUnixTime(i.finishedAt ?? i.queuedAt)}</td>
                    <td className="wrap">
                      {[formatCode('agrupamento', i.agrupamento), i.agrupamentoName]
                        .filter(Boolean)
                        .join(' · ')}
                    </td>
                    <td>{i.accountLabel ?? 'Conta apagada'}</td>
                    <td className="wrap">{outcome(i)}</td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

/** "Concluída: 243 elementos, 3 alterações, 1 saída", or the error. */
function outcome(i: ImportLogRow) {
  switch (i.status) {
    case 'done':
      return `${STATUS.done}: ${i.membersSeen ?? 0} elementos, ${i.changes ?? 0} alterações, ${i.exits ?? 0} saídas`
    case 'failed':
      return `${STATUS.failed}: ${i.error ?? 'erro desconhecido.'}`
    default:
      return STATUS[i.status]
  }
}
