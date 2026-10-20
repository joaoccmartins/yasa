// The one place that talks HTTP. Components go through feature api.ts hooks, never fetch().

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

export async function api(path: string, init?: RequestInit): Promise<unknown> {
  const res = await fetch(`/api${path}`, {
    ...init,
    // Session cookie is same-origin (Vercel rewrite in prod, Vite proxy in dev).
    credentials: 'same-origin',
    headers: { 'content-type': 'application/json', ...init?.headers },
  })
  if (!res.ok) {
    // Backend errors are {"error": "..."}. Anything else (e.g. a platform error page) gets the
    // status code, since HTTP/2 responses carry no status text.
    const body = await res.json().catch(() => ({}))
    throw new ApiError(res.status, body.error ?? (res.statusText || `Erro HTTP ${res.status}.`))
  }
  return res.status === 204 ? undefined : res.json()
}
