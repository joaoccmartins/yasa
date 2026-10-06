import { useState } from 'react'
import { AccessPage } from '../features/access/routes/AccessPage'
import { useLogout } from '../features/auth/api'
import type { Me } from '../features/auth/schema'
import { HomePage } from '../features/home/routes/HomePage'
import { Button } from '../ui/Button'

type View = 'home' | 'access'

/** Logged-in layout. Plain view state instead of a router until there are more pages. */
export function Shell({ me }: { me: Me }) {
  const logout = useLogout()
  const [view, setView] = useState<View>('home')
  // The server enforces admin-only routes; hiding the link just avoids a dead end.
  const isAdmin = me.role === 'admin'
  const current = view === 'access' && isAdmin ? 'access' : 'home'

  return (
    <>
      <header className="topbar">
        <nav className="nav" aria-label="Principal">
          <span className="brand">YASA</span>
          <button className="nav-link" aria-current={current === 'home' ? 'page' : undefined} onClick={() => setView('home')}>
            Painel
          </button>
          {isAdmin && (
            <button className="nav-link" aria-current={current === 'access' ? 'page' : undefined} onClick={() => setView('access')}>
              Acessos
            </button>
          )}
        </nav>
        <div className="who">
          <span>{me.label}</span>
          <Button onClick={() => logout.mutate()} loading={logout.isPending}>
            Sair
          </Button>
        </div>
      </header>
      <main className="main">{current === 'access' ? <AccessPage me={me} /> : <HomePage me={me} />}</main>
    </>
  )
}
