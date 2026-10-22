import type { Me } from '../../auth/schema'
import { EmptyState } from '../../../ui/EmptyState'
import { AccessSummary } from '../components/AccessSummary'
import { ImportsNotice } from '../components/ImportsNotice'
import { QualityTable } from '../../agrupamento/components/QualityTable'

/** Access, imports in progress, and how well each agrupamento registers its progresso. */
export function HomePage({ me }: { me: Me }) {
  const { access } = me
  const hasRead = access.refreshedAt != null
  return (
    <div className="page">
      <AccessSummary access={access} />
      <ImportsNotice imports={me.imports} />
      {/* Before the first read, the summary's notice says what's happening; nothing to add. */}
      {hasRead &&
        (access.agrupamentos.length === 0 ? (
          <EmptyState
            title="O SIIE não lhe mostra nenhum agrupamento"
            body="O YASA mostra os agrupamentos que a sua conta SIIE consegue ver. Se isso mudar no SIIE, o acesso é atualizado no próximo início de sessão depois de expirar."
          />
        ) : access.agrupamentos.some((a) => a.seccao == null) ? (
          <QualityTable />
        ) : (
          <EmptyState
            title="Ainda sem dados"
            body="Quando o YASA sincronizar os elementos destes agrupamentos, os seus painéis vão aparecer aqui."
          />
        ))}
    </div>
  )
}
