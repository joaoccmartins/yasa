import { useMe } from '../features/auth/api'
import { LoginPage } from '../features/auth/routes/LoginPage'
import { Button } from '../ui/Button'
import { Shell } from './Shell'

/** Auth gate: nothing but the login page until the backend confirms a session. */
export function App() {
  const me = useMe()

  if (me.isPending) {
    // Quiet splash instead of a spinner: this check is usually a few ms.
    return (
      <div className="center">
        <span className="brand">YASA</span>
      </div>
    )
  }

  if (me.isError) {
    return (
      <div className="center">
        <div className="empty">
          <h2>Não foi possível contactar o YASA</h2>
          <p>{me.error.message} Verifique a ligação e tente novamente.</p>
          <Button variant="primary" onClick={() => me.refetch()}>
            Tentar novamente
          </Button>
        </div>
      </div>
    )
  }

  return me.data ? <Shell me={me.data} /> : <LoginPage />
}
