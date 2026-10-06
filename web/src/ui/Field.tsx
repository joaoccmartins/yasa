import { cloneElement, type ReactElement } from 'react'

type Props = {
  id: string
  label: string
  error?: string
  hint?: string
  children: ReactElement<Record<string, unknown>>
}

/** Owns the label / hint / error wiring so no screen has to remember it. */
export function Field({ id, label, error, hint, children }: Props) {
  const hintId = hint ? `${id}-hint` : undefined
  const errorId = error ? `${id}-error` : undefined
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {hint && (
        <p id={hintId} className="hint">
          {hint}
        </p>
      )}
      {cloneElement(children, {
        id,
        'aria-invalid': !!error,
        'aria-describedby': [hintId, errorId].filter(Boolean).join(' ') || undefined,
      })}
      {error && (
        <p id={errorId} role="alert" className="error">
          {error}
        </p>
      )}
    </div>
  )
}
