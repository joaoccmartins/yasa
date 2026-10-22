import type { CSSProperties } from 'react'
import { formatIsoDate } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { EmptyState } from '../../../ui/EmptyState'
import { useSeccao } from '../api'
import type { Scout, SeccaoDetail } from '../schema'
import { seccaoOf, type Tipo } from '../series'
import { CriteriaMatrix } from './CriteriaMatrix'
import { CriteriaLines } from './SeccaoCards'

// What each date in Scout.steps concludes, in API order.
const STEPS = ['Adesão concluída', '1.ª etapa concluída', '2.ª etapa concluída', '3.ª etapa concluída', 'Anilha de mérito']
// The board's columns, by Scout.stage: the stage a scout is in. Concluding one moves them to the
// next; nothing concluded yet = in Adesão. The anilha de mérito is a badge, not a stage.
const STAGES = ['Adesão', '1.ª etapa', '2.ª etapa', '3.ª etapa', 'Etapas concluídas']

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
  const stepsThisYear = d.scouts.flatMap((sc) => sc.steps.slice(0, 4)).filter(thisYear).length
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
          <Tile label="Etapas concluídas este ano" value={stepsThisYear} />
          <Tile label="Promessas este ano" value={promessas} />
          <Tile label="Especialidades este ano" value={especialidades.filter((e) => thisYear(e.on)).length} />
        </div>
      </div>
      <Board d={d} thisYear={thisYear} />
      <CriteriaMatrix years={d.criteria} />
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

/** One column per stage, each scout in the stage they're in now. */
function Board({ d, thisYear }: { d: SeccaoDetail; thisYear: (on: string | null) => boolean }) {
  const columns = STAGES.map((label, stage) => ({
    label,
    scouts: d.scouts.filter((sc) => sc.stage === stage),
  }))
  return (
    <section className="panel" aria-labelledby="board-title">
      <h2 id="board-title">Progresso</h2>
      <p className="hint">
        Cada elemento está na etapa em curso: quem ainda não concluiu nada está na Adesão, e ao concluir uma passa à
        seguinte. Os destacados mudaram de etapa este ano escutista.
      </p>
      <div className="board">
        {columns.map((c) => {
          const fresh = c.scouts.filter((sc) => sc.stage > 0 && thisYear(sc.steps[sc.stage - 1])).length
          return (
            <div key={c.label} className="board-column">
              <h3>
                {c.label} <span className="board-count">{c.scouts.length}</span>
              </h3>
              {/* Always there (blank when none), so the cards line up across columns. */}
              <p className="board-fresh">{fresh > 0 ? `${fresh} este ano` : '\u00a0'}</p>
              {c.scouts.map((sc, i) => (
                <ScoutCard key={i} sc={sc} thisYear={thisYear} />
              ))}
            </div>
          )
        })}
      </div>
    </section>
  )
}

/** A scout's card; opens to the dates of each step and their especialidades. */
function ScoutCard({ sc, thisYear }: { sc: Scout; thisYear: (on: string | null) => boolean }) {
  const fresh = sc.stage > 0 && thisYear(sc.steps[sc.stage - 1])
  const espThisYear = sc.especialidades.filter((e) => thisYear(e.on)).length
  return (
    <details className="scout" data-fresh={fresh || undefined}>
      <summary>
        <span className="scout-totem">{sc.totem}</span>
        {sc.categoria && <span className="scout-sub">{sc.categoria}</span>}
        <span className="scout-tags">
          {fresh && <span className="badge" data-tone="accent">Este ano</span>}
          {sc.promessaOn && <span className="badge">Promessa</span>}
          {sc.steps[4] && <span className="badge">Anilha de mérito</span>}
          {sc.especialidades.length > 0 && (
            <span className="badge">
              {sc.especialidades.length} esp.{espThisYear > 0 && ` (+${espThisYear})`}
            </span>
          )}
        </span>
      </summary>
      <dl className="scout-dates">
        {STEPS.map((label, i) => (
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
