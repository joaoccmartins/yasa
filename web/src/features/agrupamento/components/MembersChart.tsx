import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type RefObject } from 'react'
import type { YearRow } from '../schema'
import { isAged, SECCOES, TOTAL } from '../series'

const HEIGHT = 280
const MARGIN = { top: 12, bottom: 28, left: 40 }
// Room on the right for the direct labels at the end of each line.
const RIGHT = { total: 28, seccoes: 104 }
const X_LABEL_PX = 60
const Y_TICKS = 5
// Direct labels closer than this are pushed apart.
const LABEL_GAP = 13

/** Members per ano escutista as lines: one for the total, or one per secção. */
export function MembersChart({ rows, mode }: { rows: YearRow[]; mode: 'total' | 'seccoes' }) {
  const wrap = useRef<HTMLDivElement>(null)
  const width = useWidth(wrap)
  const [active, setActive] = useState<number | null>(null)
  const series = mode === 'total' ? TOTAL : SECCOES

  const n = rows.length
  const left = MARGIN.left
  const right = width - RIGHT[mode]
  const top = MARGIN.top
  const bottom = HEIGHT - MARGIN.bottom
  const step = n > 1 ? (right - left) / (n - 1) : 0
  const x = (i: number) => (n > 1 ? left + i * step : (left + right) / 2)
  const axis = niceAxis(Math.max(1, ...rows.flatMap((r) => series.map((s) => r[s.key]))))
  const y = (v: number) => bottom - (v / axis.top) * (bottom - top)

  // Every k-th year gets an x label so they never overlap.
  const every = Math.max(1, Math.ceil((n * X_LABEL_PX) / Math.max(1, right - left)))

  const pick = (e: PointerEvent<SVGRectElement>) => {
    // The hit area starts half a step before the first year, so each year owns one step around it.
    const px = e.clientX - e.currentTarget.getBoundingClientRect().left - step / 2
    setActive(n > 1 ? Math.min(n - 1, Math.max(0, Math.round(px / step))) : 0)
  }
  const key = (e: KeyboardEvent<SVGSVGElement>) => {
    const d = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0
    if (d === 0) return
    e.preventDefault()
    setActive((a) => Math.min(n - 1, Math.max(0, (a ?? n - 1) + d)))
  }

  const last = rows[n - 1]
  const row = active == null ? null : rows[active]
  return (
    <div className="chart" ref={wrap}>
      {mode === 'seccoes' && (
        <ul className="chart-legend">
          {series.map((s) => (
            <li key={s.key}>
              <svg width="16" height="8" aria-hidden>
                <line x1="0" y1="4" x2="16" y2="4" stroke={s.color} strokeWidth="2" strokeDasharray={s.dashed ? '4 3' : undefined} />
              </svg>
              {s.label}
            </li>
          ))}
          <li>
            <svg width="10" height="10" aria-hidden>
              <circle cx="5" cy="5" r="3.5" fill="none" stroke="var(--ink-muted)" strokeWidth="1.5" />
            </svg>
            Inclui estimativas por idade
          </li>
        </ul>
      )}
      {width > 0 && n > 0 && (
        <svg
          width={width}
          height={HEIGHT}
          role="img"
          tabIndex={0}
          aria-label={`Elementos por ano escutista, de ${rows[0].label} a ${last.label}. Use as setas para ver cada ano; os números estão também na tabela abaixo.`}
          onKeyDown={key}
          onFocus={() => setActive((a) => a ?? n - 1)}
          onBlur={() => setActive(null)}
        >
          {axis.ticks.map((t) => (
            <g key={t}>
              <line x1={left} x2={right} y1={y(t)} y2={y(t)} stroke="var(--line)" />
              <text x={left - 8} y={y(t)} dy="0.32em" textAnchor="end" className="chart-axis">
                {t}
              </text>
            </g>
          ))}
          {rows.map((r, i) =>
            // The latest year always gets its label, so a regular one too close to it is skipped.
            (i % every === 0 && n - 1 - i >= every) || i === n - 1 ? (
              <text key={r.year} x={x(i)} y={HEIGHT - 8} textAnchor="middle" className="chart-axis">
                {r.label}
              </text>
            ) : null,
          )}
          {series.map((s) => (
            <path
              key={s.key}
              d={rows.map((r, i) => `${i ? 'L' : 'M'}${x(i)},${y(r[s.key])}`).join('')}
              fill="none"
              stroke={s.color}
              strokeWidth="2"
              strokeLinejoin="round"
              strokeLinecap="round"
              strokeDasharray={s.dashed ? '4 3' : undefined}
            />
          ))}
          {/* Hollow markers: years where part of a secção's count was placed by age, not by a record. */}
          {mode === 'seccoes' &&
            series.filter((s) => isAged(s.key)).flatMap((s) =>
              rows.map((r, i) =>
                isAged(s.key) && r.byAge[s.key] > 0 ? (
                  <circle
                    key={`${s.key}-${r.year}`}
                    cx={x(i)}
                    cy={y(r[s.key])}
                    r="3.5"
                    fill="var(--surface-raised)"
                    stroke={s.color}
                    strokeWidth="1.5"
                  />
                ) : null,
              ),
            )}
          {mode === 'seccoes' &&
            endLabels(series.map((s) => ({ s, at: y(last[s.key]) }))).map(({ s, at }) => (
              <text key={s.key} x={x(n - 1) + 8} y={at} dy="0.32em" className="chart-label">
                {s.label}
              </text>
            ))}
          {active != null && (
            <g aria-hidden>
              <line x1={x(active)} x2={x(active)} y1={top} y2={bottom} stroke="var(--ink-muted)" />
              {series.map((s) => (
                <circle
                  key={s.key}
                  cx={x(active)}
                  cy={y(rows[active][s.key])}
                  r="4"
                  fill={s.color}
                  stroke="var(--surface-raised)"
                  strokeWidth="2"
                />
              ))}
            </g>
          )}
          <rect
            x={left - step / 2}
            y={top}
            width={right - left + step}
            height={bottom - top}
            fill="transparent"
            onPointerMove={(e) => pick(e)}
            onPointerLeave={() => setActive(null)}
          />
        </svg>
      )}
      {row && active != null && (
        <div
          className="chart-tip"
          style={{ left: x(active), transform: x(active) > width / 2 ? 'translateX(calc(-100% - 12px))' : 'translateX(12px)' }}
        >
          <strong>{row.label}</strong>
          {(mode === 'total' ? TOTAL : [...SECCOES, ...TOTAL]).map((s) => (
            <div key={s.key} className="chart-tip-row">
              <svg width="12" height="8" aria-hidden>
                <line x1="0" y1="4" x2="12" y2="4" stroke={s.color} strokeWidth="2" strokeDasharray={s.dashed ? '3 2' : undefined} />
              </svg>
              <b>{row[s.key]}</b> <span>{s.label}</span>
              {isAged(s.key) && row.byAge[s.key] > 0 && <span>({row.byAge[s.key]} por idade)</span>}
            </div>
          ))}
          {mode === 'total' && aged(row) > 0 && (
            <div className="chart-tip-note">{aged(row)} com secção estimada por idade</div>
          )}
        </div>
      )}
    </div>
  )
}

/** How many of a year's members had their secção placed by age. */
function aged(r: YearRow) {
  return r.byAge.lobitos + r.byAge.exploradores + r.byAge.pioneiros + r.byAge.caminheiros
}

/** The element's width, kept up to date as it resizes. */
function useWidth(ref: RefObject<HTMLElement | null>) {
  const [width, setWidth] = useState(0)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setWidth(Math.floor(e.contentRect.width)))
    ro.observe(el)
    return () => ro.disconnect()
  }, [ref])
  return width
}

/** The y axis: a round tick (1, 2 or 5 times a power of ten, never below 1 since people are
 * counted whole) giving at most Y_TICKS steps, and the first tick at or above `max` as the top. */
function niceAxis(max: number) {
  const raw = max / Y_TICKS
  const pow = 10 ** Math.max(0, Math.floor(Math.log10(raw)))
  const tick = [1, 2, 5, 10].map((m) => m * pow).find((t) => t >= raw) ?? raw
  const top = Math.ceil(max / tick) * tick
  return { top, ticks: Array.from({ length: top / tick + 1 }, (_, i) => i * tick) }
}

/** Pushes the end-of-line labels apart (top to bottom) so none overlap. */
function endLabels<T extends { at: number }>(labels: T[]) {
  const sorted = [...labels].sort((a, b) => a.at - b.at)
  for (let i = 1; i < sorted.length; i++) {
    sorted[i].at = Math.max(sorted[i].at, sorted[i - 1].at + LABEL_GAP)
  }
  return sorted
}
