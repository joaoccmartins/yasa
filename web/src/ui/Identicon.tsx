// The palette's four colours (tokens.css): a member's identicon takes one of them.
const COLORS = ['var(--color-apricot)', 'var(--color-benzol)', 'var(--color-lyons)', 'var(--color-sienna)']

/** FNV-1a: 32 well-mixed bits from any string. */
function hash(text: string) {
  let h = 0x811c9dc5
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}

/** A member's identicon: a 5 x 5 pattern, mirrored left to right, in one of the palette colours.
 * Drawn from the member's opaque ref, so the same member always gets the same picture, and it
 * reveals nothing about them. */
export function Identicon({ seed, size = 32 }: { seed: string; size?: number }) {
  const h = hash(seed)
  // Bits 0-14: the 15 cells of the left three columns (column 2 is the middle); bits 15-16: colour.
  const cells = h & 0x7fff || 0x2a95
  const color = COLORS[(h >>> 15) & 3]
  const rects = []
  for (let row = 0; row < 5; row++) {
    for (let col = 0; col < 3; col++) {
      if (!((cells >>> (row * 3 + col)) & 1)) continue
      rects.push(<rect key={`${row}-${col}`} x={col} y={row} width="1" height="1" />)
      if (col < 2) rects.push(<rect key={`${row}-${4 - col}`} x={4 - col} y={row} width="1" height="1" />)
    }
  }
  return (
    <svg className="identicon" width={size} height={size} viewBox="-0.75 -0.75 6.5 6.5" aria-hidden>
      <rect x="-0.75" y="-0.75" width="6.5" height="6.5" rx="1.2" fill="var(--surface)" />
      <g fill={color}>{rects}</g>
    </svg>
  )
}
