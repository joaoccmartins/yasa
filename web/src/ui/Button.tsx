import type { ButtonHTMLAttributes } from 'react'

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary'
  loading?: boolean
}

/** Keeps its label while loading, so the width doesn't jump. */
export function Button({ variant = 'secondary', loading, className, ...rest }: Props) {
  return (
    <button
      className={['btn', className].filter(Boolean).join(' ')}
      data-variant={variant}
      aria-busy={loading || undefined}
      {...rest}
    />
  )
}
