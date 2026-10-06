import { useState } from 'react'
import { formatUnixTime } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { accessLevelLabel } from '../../auth/access'
import type { Me } from '../../auth/schema'
import { useAccounts, useUpdateAccount } from '../api'
import { InviteForm } from '../components/InviteForm'
import type { Account } from '../schema'

const SKELETON_ROWS = 3

/** Admins only: who can use YASA. Disable instead of delete, so it is always reversible. */
export function AccessPage({ me }: { me: Me }) {
  const accounts = useAccounts()
  const update = useUpdateAccount()
  const [inviting, setInviting] = useState(false)

  return (
    <section className="page">
      <div className="page-head">
        <div>
          <h1>Acessos</h1>
          <p>
            Só as pessoas convidadas podem entrar. Cada pessoa vê o que o SIIE lhe mostra, verificado de novo
            em cada início de sessão.
          </p>
        </div>
        {!inviting && (
          <Button variant="primary" onClick={() => setInviting(true)}>
            Convidar alguém
          </Button>
        )}
      </div>

      {inviting && <InviteForm onDone={() => setInviting(false)} />}

      {update.isError && (
        <p role="alert" className="error">
          {update.error.message}
        </p>
      )}

      {accounts.isError ? (
        <div className="empty">
          <h2>Não foi possível carregar as contas</h2>
          <p>{accounts.error.message}</p>
          <Button onClick={() => accounts.refetch()}>Tentar novamente</Button>
        </div>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Nome</th>
                <th>Entra com</th>
                <th>Perfil</th>
                <th>Estado</th>
                <th>Último acesso</th>
                <th>Nível</th>
                <th className="num">Agrupamentos</th>
                <th className="num">Núcleos</th>
                <th className="num">Regiões</th>
                <th aria-label="Ações" />
              </tr>
            </thead>
            <tbody>
              {accounts.isPending
                ? Array.from({ length: SKELETON_ROWS }, (_, i) => (
                    <tr key={i} aria-hidden>
                      <td colSpan={10}>
                        <span className="skeleton" />
                      </td>
                    </tr>
                  ))
                : accounts.data.map((a) => (
                    <AccountRow
                      key={a.id}
                      account={a}
                      isMe={a.id === me.accountId}
                      busy={update.isPending && update.variables?.id === a.id}
                      onUpdate={(patch) => update.mutate({ id: a.id, ...patch })}
                    />
                  ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

type RowProps = {
  account: Account
  isMe: boolean
  busy: boolean
  onUpdate: (patch: Partial<Pick<Account, 'role' | 'status'>>) => void
}

function AccountRow({ account: a, isMe, busy, onUpdate }: RowProps) {
  const methods = [a.hasNif && 'NIF', a.hasNin && 'NIN'].filter(Boolean).join(' + ')
  const active = a.status === 'active'
  const seen = a.lastLoginAt != null
  return (
    <tr data-status={a.status}>
      <td>
        {a.label}
        {isMe && <span className="tag">(eu)</span>}
      </td>
      <td>{methods}</td>
      <td>
        <span className="badge" data-tone={a.role === 'admin' ? 'accent' : undefined}>
          {a.role === 'admin' ? 'Administrador' : 'Membro'}
        </span>
      </td>
      <td>
        <span className="badge" data-tone={active ? undefined : 'danger'}>
          {active ? 'Ativa' : 'Desativada'}
        </span>
      </td>
      <td>{formatUnixTime(a.lastLoginAt, 'Nunca')}</td>
      {/* From SIIE, refreshed at each login; unknown until the first one. */}
      <td>{seen ? accessLevelLabel(a.accessLevel) : '-'}</td>
      <td className="num">{seen ? a.agrupamentos : '-'}</td>
      <td className="num">{seen ? a.nucleos : '-'}</td>
      <td className="num">{seen ? a.regioes : '-'}</td>
      <td>
        <div className="row-actions">
          <Button
            disabled={busy}
            onClick={() => onUpdate({ role: a.role === 'admin' ? 'member' : 'admin' })}
          >
            {a.role === 'admin' ? 'Retirar admin' : 'Tornar admin'}
          </Button>
          <Button disabled={busy} onClick={() => onUpdate({ status: active ? 'disabled' : 'active' })}>
            {active ? 'Desativar' : 'Ativar'}
          </Button>
        </div>
      </td>
    </tr>
  )
}
