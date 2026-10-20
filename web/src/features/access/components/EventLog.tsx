import { formatUnixTime } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { accessLevelLabel } from '../../auth/access'
import { accessLevelSchema } from '../../auth/schema'
import { useEvents } from '../api'
import type { AccountEvent } from '../schema'

const SKELETON_ROWS = 4

/** Who did what to which account, newest first. */
export function EventLog() {
  const events = useEvents()
  return (
    <section className="page-section" aria-labelledby="history-title">
      <div>
        <h2 id="history-title">Histórico</h2>
        <p className="hint">Os últimos 100 eventos: convites, entradas e alterações de acesso.</p>
      </div>
      {events.isError ? (
        <div className="empty">
          <h2>Não foi possível carregar o histórico</h2>
          <p>{events.error.message}</p>
          <Button onClick={() => events.refetch()}>Tentar novamente</Button>
        </div>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Quando</th>
                <th>Evento</th>
              </tr>
            </thead>
            <tbody>
              {events.isPending ? (
                Array.from({ length: SKELETON_ROWS }, (_, i) => (
                  <tr key={i} aria-hidden>
                    <td colSpan={2}>
                      <span className="skeleton" />
                    </td>
                  </tr>
                ))
              ) : events.data.length === 0 ? (
                <tr>
                  <td colSpan={2} className="muted">
                    Ainda não há eventos.
                  </td>
                </tr>
              ) : (
                events.data.map((e) => (
                  <tr key={e.id}>
                    <td className="when">{formatUnixTime(e.at)}</td>
                    <td className="wrap">{describe(e)}</td>
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

/** One sentence per event. `detail` is read defensively: older rows may lack a field. */
function describe(e: AccountEvent): string {
  const actor = e.actorLabel ?? 'Linha de comandos'
  const who = e.accountLabel
  const d = e.detail
  const roleName = (r: unknown) => (r === 'admin' ? 'administrador' : 'membro')
  const kind = typeof d.kind === 'string' ? d.kind.toUpperCase() : 'identificador'
  switch (e.action) {
    case 'invited':
      return `${actor} convidou ${who} como ${roleName(d.role)} (entra com ${kind}).`
    case 'login':
      switch (d.access) {
        case 'started':
          return `${who} entrou; o acesso começou a ser lido do SIIE.`
        case 'already_running':
          return `${who} entrou; o acesso já estava a ser lido do SIIE.`
        case 'fresh':
          return `${who} entrou (acesso dentro da validade).`
        default:
          // Logins from before access was read in the background carried the counts.
          return d.accessLevel ? `${who} entrou com ${levelAndCounts(d)}.` : `${who} entrou.`
      }
    case 'access_refreshed':
      return `O acesso de ${who} foi lido do SIIE: ${levelAndCounts(d)}.`
    case 'access_failed':
      return `Não foi possível ler o acesso de ${who} no SIIE.`
    case 'enabled':
      return `${actor} ativou ${who}.`
    case 'disabled':
      return `${actor} desativou ${who}.`
    case 'role_changed':
      return `${actor} tornou ${who} ${roleName(d.to)}.`
    case 'identifier_linked':
      return `O ${kind} de ${who} foi associado à conta (encontrado na grelha do SIIE).`
  }
}

/** "1 núcleo" / "3 núcleos"; "?" when an older event lacks the number. */
function count(n: unknown, singular: string, plural: string) {
  return typeof n === 'number' ? `${n} ${n === 1 ? singular : plural}` : `? ${plural}`
}

/** "acesso de nível núcleo: 53 agrupamentos, 1 núcleo, 1 região" from an event's detail. */
function levelAndCounts(d: Record<string, unknown>) {
  const level = accessLevelSchema.safeParse(d.accessLevel)
  const levelText = level.success ? accessLevelLabel(level.data).toLowerCase() : '?'
  const counts = [
    count(d.agrupamentos, 'agrupamento', 'agrupamentos'),
    count(d.nucleos, 'núcleo', 'núcleos'),
    count(d.regioes, 'região', 'regiões'),
  ].join(', ')
  return `acesso de nível ${levelText}: ${counts}`
}
