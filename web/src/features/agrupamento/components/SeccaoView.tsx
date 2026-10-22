import type { CSSProperties } from 'react'
import { formatIsoDate } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { EmptyState } from '../../../ui/EmptyState'
import { useSeccao } from '../api'
import type { Scout, SeccaoDetail } from '../schema'
import { ETAPAS, seccaoOf, STEP_KINDS, type Tipo } from '../series'
import { CriteriaChart } from './CriteriaChart'
import { CriteriaLines } from './SeccaoCards'

/** One secção this ano escutista: who is at which progression step, and its especialidades. */
export function SeccaoView({ code, tipo }: { code: number; tipo: Tipo }) {
  const seccao = useSeccao(code, tipo)
  const s = seccaoOf(tipo)

  if (seccao.isPending) {
    return (
      <div className="panel" aria-busy>
        <span className="skeleton" />
      </div>
    )
  }
  if (seccao.isError) {
    return (
      <EmptyState
        title="Não foi possível carregar a secção"
        body={seccao.error.message}
        action={<Button onClick={() => seccao.refetch()}>Tentar novamente</Button>}
      />
    )
  }
  const d = seccao.data
  if (d.scouts.length === 0) {
    return <EmptyState title={`Sem ${s.label.toLowerCase()} em ${d.yearLabel}`} body="Nenhum elemento ativo está nesta secção este ano escutista." />
  }
  // "This year" = on or after the ano escutista's first day. ISO dates compare as text.
  const thisYear = (on: string | null) => on != null && on >= d.yearStart
  const stepsThisYear = d.scouts.flatMap((sc) => sc.steps).filter(thisYear).length
  const now = d.criteria.find((c) => c.label === d.yearLabel)
  const promessas = d.scouts.filter((sc) => thisYear(sc.promessaOn)).length
  const especialidades = d.scouts.flatMap((sc) => sc.especialidades)
  return (
    <div className="seccao-view" style={{ '--seccao': s.color } as CSSProperties}>
      <div className="panel summary">
        <h2>
          {s.label} em {d.yearLabel}
        </h2>
        <CriteriaLines c={now} />
        <div className="tiles">
          <Tile label="Elementos" value={d.scouts.length} />
          <Tile label="Etapas alcançadas este ano" value={stepsThisYear} />
          <Tile label="Promessas este ano" value={promessas} />
          <Tile label="Especialidades este ano" value={especialidades.filter((e) => thisYear(e.on)).length} />
        </div>
      </div>
      <Board d={d} names={ETAPAS[tipo]} thisYear={thisYear} />
      <CriteriaChart years={d.criteria} />
      <EspecialidadesTable scouts={d.scouts} thisYear={thisYear} />
    </div>
  )
}

function Tile({ label, value }: { label: string; value: number }) {
  return (
    <div className="tile">
      <span className="tile-label">{label}</span>
      <span className="tile-value">{value}</span>
    </div>
  )
}

/** One column per step, as SIIE's progresso distribution: each scout at the step of their latest
 * date. Scouts with no date at all are in the first (adesão) one, which SIIE leaves out. */
function Board({
  d,
  names,
  thisYear,
}: {
  d: SeccaoDetail
  names: readonly string[]
  thisYear: (on: string | null) => boolean
}) {
  const columns = names.map((label, stage) => ({
    label,
    kind: STEP_KINDS[stage],
    scouts: d.scouts.filter((sc) => sc.stage === stage),
  }))
  return (
    <section className="panel" aria-labelledby="board-title">
      <h2 id="board-title">Progresso</h2>
      <p className="hint">
        Cada elemento está na etapa da sua data mais recente no SIIE, como na distribuição do progresso do SIIE.
        Quem não tem nenhuma data está na adesão (o SIIE não os conta). Os destacados chegaram à etapa este ano
        escutista.
      </p>
      <div className="board">
        {columns.map((c) => {
          const fresh = c.scouts.filter((sc) => thisYear(sc.steps[sc.stage])).length
          return (
            <div key={c.label} className="board-column">
              <h3>
                <span>
                  {c.label}
                  {c.kind && <span className="scout-sub"> {c.kind}</span>}
                </span>
                <span className="board-count">{c.scouts.length}</span>
              </h3>
              {/* Always there (blank when none), so the cards line up across columns. */}
              <p className="board-fresh">{fresh > 0 ? `${fresh} este ano` : '\u00a0'}</p>
              {c.scouts.map((sc, i) => (
                <ScoutCard key={i} sc={sc} names={names} thisYear={thisYear} />
              ))}
            </div>
          )
        })}
      </div>
    </section>
  )
}

/** A scout's card; opens to the date of each step and their especialidades. */
function ScoutCard({
  sc,
  names,
  thisYear,
}: {
  sc: Scout
  names: readonly string[]
  thisYear: (on: string | null) => boolean
}) {
  const fresh = thisYear(sc.steps[sc.stage])
  // No step dated at all: in adesão only because nothing is registered.
  const undated = sc.steps.every((on) => on == null)
  const espThisYear = sc.especialidades.filter((e) => thisYear(e.on)).length
  return (
    <details className="scout" data-fresh={fresh || undefined}>
      <summary>
        <span className="scout-totem">{sc.totem}</span>
        {sc.categoria && <span className="scout-sub">{sc.categoria}</span>}
        <span className="scout-tags">
          {fresh && <span className="badge" data-tone="accent">Este ano</span>}
          {undated && <span className="badge">Sem datas</span>}
          {sc.promessaOn && <span className="badge">Promessa</span>}
          {sc.especialidades.length > 0 && (
            <span className="badge">
              {sc.especialidades.length} esp.{espThisYear > 0 && ` (+${espThisYear})`}
            </span>
          )}
        </span>
      </summary>
      <dl className="scout-dates">
        {names.map((label, i) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{sc.steps[i] ? formatIsoDate(sc.steps[i]!) : '-'}</dd>
          </div>
        ))}
        <div>
          <dt>Promessa</dt>
          <dd>{sc.promessaOn ? formatIsoDate(sc.promessaOn) : '-'}</dd>
        </div>
      </dl>
      {sc.especialidades.length > 0 && (
        <ul className="scout-esp">
          {sc.especialidades.map((e) => (
            <li key={e.name}>
              {e.name}
              {e.on && <span className="scout-sub"> {formatIsoDate(e.on)}</span>}
            </li>
          ))}
        </ul>
      )}
    </details>
  )
}

/** Especialidades earned in this secção by its current scouts: how many hold each, and this year. */
function EspecialidadesTable({ scouts, thisYear }: { scouts: Scout[]; thisYear: (on: string | null) => boolean }) {
  const counts = new Map<string, { total: number; year: number }>()
  for (const e of scouts.flatMap((sc) => sc.especialidades)) {
    const c = counts.get(e.name) ?? { total: 0, year: 0 }
    c.total += 1
    if (thisYear(e.on)) c.year += 1
    counts.set(e.name, c)
  }
  // Most held first, then by name.
  const rows = [...counts].sort((a, b) => b[1].total - a[1].total || a[0].localeCompare(b[0], 'pt'))
  return (
    <section className="panel" aria-labelledby="esp-title">
      <h2 id="esp-title">Especialidades</h2>
      {rows.length === 0 ? (
        <p className="hint">Nenhum elemento desta secção tem especialidades registadas aqui.</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Especialidade</th>
                <th className="num">Elementos</th>
                <th className="num">Este ano</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(([name, c]) => (
                <tr key={name}>
                  <td className="wrap">{name}</td>
                  <td className="num">{c.total}</td>
                  <td className="num">{c.year || '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}
