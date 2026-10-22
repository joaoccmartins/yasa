import type { AnchorHTMLAttributes } from 'react'
import { navigate } from '../lib/router'

/** A real link (open in a new tab, copy address) that navigates in place on a plain click. */
export function Link({ to, ...props }: { to: string } & AnchorHTMLAttributes<HTMLAnchorElement>) {
  return (
    <a
      href={to}
      {...props}
      onClick={(e) => {
        props.onClick?.(e)
        // Ctrl/Cmd/Shift/middle click keep the browser's own behaviour (new tab or window).
        if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
        e.preventDefault()
        navigate(to)
      }}
    />
  )
}
