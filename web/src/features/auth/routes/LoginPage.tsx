import { useState, type FormEvent } from 'react'
import { Button } from '../../../ui/Button'
import { Field } from '../../../ui/Field'
import { Segmented } from '../../../ui/Segmented'
import { useLogin } from '../api'
import { idLabelFor, LOGIN_METHODS } from '../methods'
import type { LoginMethod } from '../schema'

/** The only screen a logged-out user sees. Credentials go to SIIE once and are not stored. */
export function LoginPage() {
  const login = useLogin()
  const [method, setMethod] = useState<LoginMethod>('loja')
  const [identifier, setIdentifier] = useState('')
  const [password, setPassword] = useState('')
  const [tried, setTried] = useState(false)

  const idLabel = idLabelFor(method)
  // Field errors only after the first submit attempt, never while typing.
  const idError = tried && !identifier.trim() ? `Indique o seu ${idLabel}.` : undefined
  const pwError = tried && !password ? 'Indique a sua palavra-passe.' : undefined

  function submit(e: FormEvent) {
    e.preventDefault()
    setTried(true)
    if (!identifier.trim() || !password) return
    login.mutate({ identifier: identifier.trim(), password, method })
  }

  return (
    <main className="login">
      <div className="login-card">
        <header>
          <span className="brand">YASA</span>
          <p>Inicie sessão com a sua conta SIIE para ver os dados a que esta lhe dá acesso.</p>
        </header>

        <form onSubmit={submit} noValidate autoComplete="off">
          <Segmented
            name="method"
            legend="Entrar com"
            options={LOGIN_METHODS}
            value={method}
            onChange={setMethod}
          />

          <Field id="identifier" label={idLabel} error={idError}>
            <input
              type="text"
              inputMode="numeric"
              value={identifier}
              onChange={(e) => setIdentifier(e.target.value)}
            />
          </Field>

          <Field id="password" label="Palavra-passe" error={pwError}>
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>

          {login.isError && (
            <p role="alert" className="error">
              {login.error.message}
            </p>
          )}

          <Button type="submit" variant="primary" loading={login.isPending}>
            {login.isPending ? 'A verificar o seu acesso no SIIE…' : 'Entrar'}
          </Button>
        </form>
      </div>
    </main>
  )
}
