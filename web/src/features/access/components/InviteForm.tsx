import { useState, type FormEvent } from 'react'
import { Button } from '../../../ui/Button'
import { Field } from '../../../ui/Field'
import { Segmented } from '../../../ui/Segmented'
import { idLabelFor, LOGIN_METHODS } from '../../auth/methods'
import type { LoginMethod } from '../../auth/schema'
import { useInvite } from '../api'

/** Adds someone to the invite list. Only a hash of the NIF/NIN is stored, never the number. */
export function InviteForm({ onDone }: { onDone: () => void }) {
  const invite = useInvite()
  const [method, setMethod] = useState<LoginMethod>('loja')
  const [identifier, setIdentifier] = useState('')
  const [label, setLabel] = useState('')
  const [admin, setAdmin] = useState(false)
  const [tried, setTried] = useState(false)

  const idLabel = idLabelFor(method)
  const idError = tried && !identifier.trim() ? `Indique o ${idLabel}.` : undefined
  const labelError = tried && !label.trim() ? 'Indique um nome para saber quem é.' : undefined

  function submit(e: FormEvent) {
    e.preventDefault()
    setTried(true)
    if (!identifier.trim() || !label.trim()) return
    invite.mutate(
      { method, identifier: identifier.trim(), label: label.trim(), role: admin ? 'admin' : 'member' },
      { onSuccess: onDone },
    )
  }

  return (
    <form className="panel invite" onSubmit={submit} noValidate autoComplete="off">
      <Segmented
        name="invite-method"
        legend="Entra com"
        options={LOGIN_METHODS}
        value={method}
        onChange={setMethod}
      />
      <Field id="invite-identifier" label={idLabel} error={idError} hint="Guardado apenas como hash.">
        <input
          type="text"
          inputMode="numeric"
          value={identifier}
          onChange={(e) => setIdentifier(e.target.value)}
        />
      </Field>
      <Field id="invite-label" label="Nome" error={labelError} hint={'Só para saber quem é, p. ex. "Joana, Agr. 123".'}>
        <input type="text" value={label} onChange={(e) => setLabel(e.target.value)} />
      </Field>
      <label className="check">
        <input type="checkbox" checked={admin} onChange={(e) => setAdmin(e.target.checked)} />
        Administrador (pode gerir quem tem acesso)
      </label>
      {invite.isError && (
        <p role="alert" className="error">
          {invite.error.message}
        </p>
      )}
      <div className="actions">
        <Button type="button" onClick={onDone}>
          Cancelar
        </Button>
        <Button type="submit" variant="primary" loading={invite.isPending}>
          Convidar
        </Button>
      </div>
    </form>
  )
}
