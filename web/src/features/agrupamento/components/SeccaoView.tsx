import type { CSSProperties } from 'react'
import { paths } from '../../../lib/router'
import { Button } from '../../../ui/Button'
import { EmptyState } from '../../../ui/EmptyState'
import { Identicon } from '../../../ui/Identicon'
import { Link } from '../../../ui/Link'
import { useSeccao } from '../api'
import type { Adult, Scout, SeccaoDetail } from '../schema'
import { ETAPAS, seccaoOf, STEP_KINDS, SUBUNIDADES, type Tipo } from '../series'
import { CriteriaChart } from './CriteriaChart'
import { CriteriaLines, CriteriaOverlay } from './Criteria'

const oneDecimal = new Intl.NumberFormat('pt-PT', { maximumFractionDigits: 1 })

/** Mean of the known values, to one decimal; "-" when there are none. */
function mean(values: (number | null)[]) {
  const known = values.filter((v): v is number => v != null)
  return known.length === 0 ? '-' : oneDecimal.format(known.reduce((a, b) => a + b, 0) / known.length)
}

/** One secção this ano escutista: its dirigentes, who is at which progression step, its quality
 * criteria and especialidades. */
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
    return (
      <EmptyState
        title={`Sem ${s.label.toLowerCase()} em ${d.yearLabel}`}
        body="Nenhum elemento ativo está nesta secção este ano escutista."
      />
    )
  }
  // "This year" = on or after the ano escutista's first day. ISO dates compare as text.
  const thisYear = (on: string | null) => on != null && on >= d.yearStart
  const stepsThisYear = d.scouts.flatMap((sc) => sc.steps).filter(thisYear).length
  const now = d.criteria.find((c) => c.label === d.yearLabel)
  const promessas = d.scouts.filter((sc) => thisYear(sc.promessaOn)).length
  const especialidades = d.scouts.flatMap((sc) => sc.especialidades)
  return (
    <div className="seccao-view" style={{ '--seccao': s.color } as CSSProperties}>
      <Adults code={code} d={d} tipo={tipo} />
      <div className="panel summary">
        <h2>
          {s.label} em {d.yearLabel}
        </h2>
        {now && (
          <CriteriaOverlay
            id={`criteria-seccao-${tipo}`}
            title={`${s.label} em ${d.yearLabel}: critérios por cumprir`}
            defs={d.criteriaDefs}
            failed={now.failed.map((key) => ({ key }))}
            points={now.points}
          />
        )}
        <CriteriaLines defs={d.criteriaDefs} c={now} points={false} />
        <div className="tiles">
          <Tile label="Etapas alcançadas este ano" value={stepsThisYear} />
          <Tile label="Promessas este ano" value={promessas} />
          <Tile label="Especialidades este ano" value={especialidades.filter((e) => thisYear(e.on)).length} />
        </div>
      </div>
      <Board code={code} d={d} names={ETAPAS[tipo]} thisYear={thisYear} />
      <CriteriaChart defs={d.criteriaDefs} years={d.criteria} />
      <EspecialidadesTable scouts={d.scouts} thisYear={thisYear} />
    </div>
  )
}

/** The adults with a cargo in the secção, on top, with the ratios. Instrutores are counted per
 * sub-unidade; the chefe and chefe adjunto lead the whole unidade, so they aren't. */
function Adults({ code, d, tipo }: { code: number; d: SeccaoDetail; tipo: Tipo }) {
  const ratio = d.adults.length === 0 ? '-' : oneDecimal.format(d.scouts.length / d.adults.length)
  const instrutores = d.adults.filter((a) => a.instrutor).length
  const subs = d.subunidades.sizes
  const perSub = subs.length === 0 ? '-' : oneDecimal.format(instrutores / subs.length)
  // "6 + 6 + 5", or "2 × 6" when they're all the same size.
  const split = subs.length > 1 && subs.every((n) => n === subs[0]) ? `${subs.length} × ${subs[0]}` : subs.join(' + ')
  return (
    <section className="panel summary" aria-labelledby="adults-title">
      <div>
        <h2 id="adults-title">Equipa de animação</h2>
        <p className="hint">
          Dirigentes, aspirantes e noviços a dirigente com um cargo ativo nesta secção: chefe, chefe adjunto, ou
          instrutor quando a descrição do cargo no SIIE diz a secção. O SIIE não tem as sub-unidades: são estimadas pelo
          número de elementos (4 a 8 cada, 5 a 8 nos lobitos, tão perto de 6 quanto possível).
        </p>
      </div>
      <div className="tiles">
        <Tile label="Elementos" value={d.scouts.length} />
        <Tile label="Dirigentes" value={d.adults.length} />
        <Tile label="Instrutores" value={instrutores} />
        <Tile
          label={`Sub-unidades${d.subunidades.estimated ? ' (estimadas)' : ''}`}
          value={subs.length}
          note={subs.length > 0 ? `${SUBUNIDADES[tipo]}: ${split}` : undefined}
        />
        <Tile label="Instrutores por sub-unidade" value={perSub} />
        <Tile label="Elementos por dirigente" value={ratio} />
        <Tile label="Idade média dos elementos" value={mean(d.scouts.map((sc) => sc.age))} />
        <Tile label="Idade média dos dirigentes" value={mean(d.adults.map((a) => a.age))} />
      </div>
      {d.adults.length === 0 ? (
        <p className="hint">Nenhum adulto tem cargo nesta secção no SIIE.</p>
      ) : (
        <div className="adults">
          {d.adults.map((a, i) => (
            <AdultCard key={i} code={code} a={a} />
          ))}
        </div>
      )}
    </section>
  )
}

function AdultCard({ code, a }: { code: number; a: Adult }) {
  return (
    <Link to={paths.member(code, a.ref)} className="scout">
      <Identicon seed={a.ref} />
      <span className="scout-body">
        <span className="scout-totem">{a.totem}</span>
        <span className="scout-sub">{[a.cargos, a.categoria].filter(Boolean).join(' · ')}</span>
        {a.age != null && <span className="scout-sub">{a.age} anos</span>}
      </span>
    </Link>
  )
}

function Tile({ label, value, note }: { label: string; value: number | string; note?: string }) {
  return (
    <div className="tile">
      <span className="tile-label">{label}</span>
      <span className="tile-value">{value}</span>
      {note && <span className="tile-label">{note}</span>}
    </div>
  )
}

/** One column per step, as SIIE's progresso distribution: each scout at the step of their latest
 * date. Scouts with no date at all are in the first (adesão) one, which SIIE leaves out. */
function Board({
  code,
  d,
  names,
  thisYear,
}: {
  code: number
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
        Cada elemento está na etapa da sua data mais recente no SIIE, como na distribuição do progresso do SIIE. Quem
        não tem nenhuma data está na adesão (o SIIE não os conta). Os destacados chegaram à etapa este ano escutista.
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
                <ScoutCard key={i} code={code} sc={sc} thisYear={thisYear} />
              ))}
            </div>
          )
        })}
      </div>
    </section>
  )
}

/** A scout's card; links to their page, with their history year by year. */
function ScoutCard({ code, sc, thisYear }: { code: number; sc: Scout; thisYear: (on: string | null) => boolean }) {
  const fresh = thisYear(sc.steps[sc.stage])
  const espThisYear = sc.especialidades.filter((e) => thisYear(e.on)).length
  // No step dated at all: in adesão only because nothing is registered.
  const undated = sc.steps.every((on) => on == null)
  return (
    <Link to={paths.member(code, sc.ref)} className="scout" data-fresh={fresh || undefined}>
      <Identicon seed={sc.ref} />
      <span className="scout-body">
        <span className="scout-totem">{sc.totem}</span>
        <span className="scout-sub">
          {[sc.categoria, sc.age != null && `${sc.age} anos`].filter(Boolean).join(' · ')}
        </span>
        <span className="scout-tags">
          {fresh && (
            <span className="badge" data-tone="accent">
              Este ano
            </span>
          )}
          {undated && <span className="badge">Sem datas</span>}
          {sc.promessaOn && <span className="badge">Promessa</span>}
          {sc.especialidades.length > 0 && (
            <span className="badge">
              {sc.especialidades.length} esp.
              {espThisYear > 0 && ` (+${espThisYear})`}
            </span>
          )}
        </span>
      </span>
    </Link>
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
