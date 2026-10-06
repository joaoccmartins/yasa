import type { ReactNode } from 'react'

export function EmptyState({ title, body, action }: { title: string; body: string; action?: ReactNode }) {
  return (
    <section className="empty">
      <h2>{title}</h2>
      <p>{body}</p>
      {action}
    </section>
  )
}
