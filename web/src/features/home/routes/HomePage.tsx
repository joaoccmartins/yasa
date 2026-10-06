import type { Me } from '../../auth/schema'
import { EmptyState } from '../../../ui/EmptyState'
import { AccessSummary } from '../components/AccessSummary'

/** Dashboards land here once data sync exists. */
export function HomePage({ me }: { me: Me }) {
  return (
    <div className="page">
      <AccessSummary me={me} />
      {me.accessLevel === 'none' ? (
        <EmptyState
          title="O SIIE não lhe mostra nenhum agrupamento"
          body="O YASA mostra os agrupamentos que a sua conta SIIE consegue ver. Se isso mudar no SIIE, volte a iniciar sessão para atualizar."
        />
      ) : (
        <EmptyState
          title="Ainda sem dados"
          body="Quando o YASA sincronizar os elementos destes agrupamentos, os seus painéis vão aparecer aqui."
        />
      )}
    </div>
  )
}
