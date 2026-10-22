// Every screen has its own URL, so it can be bookmarked, shared and opened in a new tab:
//   /                                   Painel
//   /acessos                            Acessos (admins)
//   /agrupamentos/0028                  an agrupamento's Resumo
//   /agrupamentos/0028/lobitos          one secção (lobitos, exploradores, pioneiros, caminheiros)
//   /agrupamentos/0028/elementos/<ref>  one member (<ref> is opaque: never a NIN)
// Plain History API instead of a router library: a handful of routes doesn't need one.
import { useSyncExternalStore } from 'react'

export type Route =
  | { name: 'home' }
  | { name: 'access' }
  | { name: 'agrupamento'; code: number | null; seccao: string | null; member: string | null }

export function parseRoute(pathname: string): Route {
  const [first, code, third, fourth] = pathname.split('/').filter(Boolean)
  if (first === 'acessos') return { name: 'access' }
  if (first === 'agrupamentos') {
    const n = Number(code)
    return {
      name: 'agrupamento',
      code: code && Number.isInteger(n) ? n : null,
      seccao: third && third !== 'elementos' ? third : null,
      member: third === 'elementos' ? (fourth ?? null) : null,
    }
  }
  return { name: 'home' }
}

const pad = (code: number) => String(code).padStart(4, '0')

export const paths = {
  home: '/',
  access: '/acessos',
  agrupamento: (code: number) => `/agrupamentos/${pad(code)}`,
  seccao: (code: number, slug: string) => `/agrupamentos/${pad(code)}/${slug}`,
  member: (code: number, ref: string) => `/agrupamentos/${pad(code)}/elementos/${ref}`,
}

// Navigation fires popstate itself, so one listener covers links and the back button.
function subscribe(onChange: () => void) {
  window.addEventListener('popstate', onChange)
  return () => window.removeEventListener('popstate', onChange)
}

/** The current route; re-renders on navigation and on back / forward. */
export function useRoute(): Route {
  const pathname = useSyncExternalStore(subscribe, () => window.location.pathname)
  return parseRoute(pathname)
}

export function navigate(to: string) {
  if (to === window.location.pathname) return
  window.history.pushState(null, '', to)
  window.dispatchEvent(new PopStateEvent('popstate'))
  window.scrollTo(0, 0)
}
