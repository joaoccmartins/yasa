import { AccessPage } from '../features/access/routes/AccessPage'
import { AgrupamentoPage } from '../features/agrupamento/routes/AgrupamentoPage'
import { useLogout } from '../features/auth/api'
import type { Me } from '../features/auth/schema'
import { HomePage } from '../features/home/routes/HomePage'
import { paths, useRoute } from '../lib/router'
import { Button } from '../ui/Button'
import { Link } from '../ui/Link'

/** Logged-in layout. The URL picks the page (see lib/router.ts). */
export function Shell({ me }: { me: Me }) {
  const logout = useLogout()
  const route = useRoute()
  // The server enforces admin-only routes; hiding the link just avoids a dead end.
  const isAdmin = me.role === 'admin'
  // The digest needs a whole agrupamento: secção-only access has no member imports.
  const hasWhole = me.access.agrupamentos.some((a) => a.seccao == null)
  // A page the account can't use falls back to the Painel.
  const page =
    (route.name === 'access' && isAdmin) || (route.name === 'agrupamento' && hasWhole) ? route.name : 'home'
  const current = (p: string) => (page === p ? 'page' : undefined)

  return (
    <>
      <header className="topbar">
        <nav className="nav" aria-label="Principal">
          <span className="brand">YASA</span>
          <Link to={paths.home} className="nav-link" aria-current={current('home')}>
            Painel
          </Link>
          {hasWhole && (
            <Link to="/agrupamentos" className="nav-link" aria-current={current('agrupamento')}>
              Agrupamento
            </Link>
          )}
          {isAdmin && (
            <Link to={paths.access} className="nav-link" aria-current={current('access')}>
              Acessos
            </Link>
          )}
        </nav>
        <div className="who">
          <span>{me.label}</span>
          <Button onClick={() => logout.mutate()} loading={logout.isPending}>
            Sair
          </Button>
        </div>
      </header>
      <main className="main">
        {page === 'access' ? (
          <AccessPage me={me} />
        ) : page === 'agrupamento' && route.name === 'agrupamento' ? (
          <AgrupamentoPage me={me} route={route} />
        ) : (
          <HomePage me={me} />
        )}
      </main>
    </>
  )
}
