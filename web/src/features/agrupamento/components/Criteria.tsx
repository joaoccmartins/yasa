import type { Criteria, Criterion, Level } from '../schema'

const LEVELS: Record<Level, string> = { green: 'Verde', yellow: 'Amarelo', red: 'Vermelho' }

/** A criterion's level as a coloured mark plus its name, so it never relies on colour alone. */
export function LevelBadge({ level }: { level: Level }) {
  return (
    <span className="level" data-level={level}>
      {LEVELS[level]}
    </span>
  )
}

/** "3 pontos" */
export function Points({ points }: { points: number }) {
  return points === 0 ? (
    <span className="points" data-zero>
      Cumpre todos os critérios
    </span>
  ) : (
    <span className="points">
      {points} {points === 1 ? 'ponto' : 'pontos'}
    </span>
  )
}

/** A share of members as text plus a thin bar, with a tick at the criterion's limit. The bar takes
 * the level's colour only when the limit is passed (the secção fails); grey otherwise. */
export function Meter({
  label,
  value,
  of,
  level,
  max,
}: {
  label?: string
  value: number
  of: number
  level: Level
  max?: number | null
}) {
  const pct = of === 0 ? 0 : Math.round((value / of) * 100)
  const failed = max != null ? value > max * of : value > 0
  return (
    <span className="meter" data-level={level} data-failed={failed || undefined}>
      <span className="meter-text">
        {label && `${label} `}
        <b>{value}</b> de {of} <span className="muted">({pct}%)</span>
        {failed && max != null && <span className="meter-fail"> acima de {Math.round(max * 100)}%</span>}
      </span>
      <span className="meter-bar" aria-hidden>
        <span style={{ width: `${pct}%` }} />
        {max != null && <i style={{ left: `${max * 100}%` }} />}
      </span>
    </span>
  )
}

/** One year's criteria for a secção: the points (unless `points` is false, when an overlay shows
 * them instead), a meter per member criterion, and the secção criteria it fails. */
export function CriteriaLines({
  defs,
  c,
  points = true,
}: {
  defs: Criterion[]
  c: Criteria | undefined
  points?: boolean
}) {
  if (!c || c.members === 0) return null
  const failedSeccao = defs.filter((k) => k.scope === 'seccao' && c.failed.includes(k.key))
  return (
    <span className="quality-lines">
      {points && <Points points={c.points} />}
      {defs
        .filter((k) => k.scope === 'members')
        .map((k) => (
          <Meter
            key={k.key}
            label={k.label}
            value={c.flags[k.key] ?? 0}
            of={c.members}
            level={k.level}
            max={k.maxShare}
          />
        ))}
      {failedSeccao.map((k) => (
        <span key={k.key} className="crit-flag" title={k.hint}>
          <LevelBadge level={k.level} /> {k.label}
        </span>
      ))}
    </span>
  )
}

const percent = (share: number) => `${Math.round(share * 100)}%`

/** The points as a button that opens an overlay listing the criteria not met (secção, level,
 * what it means, what it costs), and how every criterion is decided. Native popover: closes on
 * Escape or a click outside. */
export function CriteriaOverlay({
  id,
  title,
  defs,
  failed,
  points,
}: {
  id: string
  title: string
  defs: Criterion[]
  /** The criteria not met: their key, and the secção (tipo) when it's for a whole agrupamento. */
  failed: { key: string; seccao?: string }[]
  points: number
}) {
  const byKey = new Map(defs.map((k) => [k.key, k]))
  return (
    <>
      <button type="button" className="points-button" popoverTarget={id} data-zero={points === 0 || undefined}>
        {points === 0 ? 'Cumpre todos os critérios' : `${points} ${points === 1 ? 'ponto' : 'pontos'}`}
      </button>
      <div id={id} popover="auto" className="overlay" role="dialog" aria-labelledby={`${id}-title`}>
        <div className="overlay-head">
          <h3 id={`${id}-title`}>{title}</h3>
          <button type="button" className="btn" popoverTarget={id} popoverTargetAction="hide">
            Fechar
          </button>
        </div>
        {failed.length === 0 ? (
          <p className="hint">Cumpre todos os critérios este ano escutista.</p>
        ) : (
          <ul className="overlay-list">
            {failed.map((f, i) => {
              const k = byKey.get(f.key)
              if (!k) return null
              return (
                <li key={i}>
                  <span className="overlay-item-head">
                    <LevelBadge level={k.level} />
                    <b>{[f.seccao, k.label].filter(Boolean).join(': ')}</b>
                    <span className="overlay-points">+{k.points}</span>
                  </span>
                  <span className="sub">
                    {k.hint}
                    {k.maxShare != null && ` Não cumpre acima de ${percent(k.maxShare)} da secção.`}
                  </span>
                </li>
              )
            })}
          </ul>
        )}
        <details>
          <summary>Como são os critérios</summary>
          <p className="hint">Cada critério que uma secção não cumpre custa os pontos do seu nível: quanto mais, pior.</p>
          <ul className="overlay-list">
            {defs.map((k) => (
              <li key={k.key}>
                <span className="overlay-item-head">
                  <LevelBadge level={k.level} />
                  <b>{k.label}</b>
                  <span className="overlay-points">{k.points} pontos</span>
                </span>
                <span className="sub">
                  {k.hint}
                  {k.maxShare != null && ` Não cumpre acima de ${percent(k.maxShare)} da secção.`}
                </span>
              </li>
            ))}
          </ul>
        </details>
      </div>
    </>
  )
}
