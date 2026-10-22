import type { CSSProperties } from 'react'
import { ApiError } from '../../../lib/api-client'
import { formatCode, formatIsoDate } from '../../../lib/format'
import { paths } from '../../../lib/router'
import { Button } from '../../../ui/Button'
import { EmptyState } from '../../../ui/EmptyState'
import { Identicon } from '../../../ui/Identicon'
import { Link } from '../../../ui/Link'
import { useMember } from '../api'
import type { Member } from '../schema'
import { ETAPAS, scoutYear, SECCOES, seccaoOf, TIPOS } from '../series'

/** One member: who they are, their progresso in each secção, and their history year by year. */
export function MemberView({ code, memberRef }: { code: number; memberRef: string }) {
  const q = useMember(code, memberRef)
  if (q.isPending) {
    return (
      <div className="panel" aria-busy>
        <span className="skeleton" />
      </div>
    )
  }
  if (q.isError) {
    const missing = q.error instanceof ApiError && q.error.status === 404
    return (
      <EmptyState
        title={missing ? 'Elemento não encontrado' : 'Não foi possível carregar o elemento'}
        body={
          missing
            ? 'Este endereço não corresponde a nenhum elemento deste agrupamento.'
            : q.error.message
        }
        action={missing ? undefined : <Button onClick={() => q.refetch()}>Tentar novamente</Button>}
      />
    )
  }
  const m = q.data
  const i = m.info
  // Where they're counted this year (their last year on record).
  const now = SECCOES.find((s) => s.key === m.years.at(-1)?.bucket)
  const nowTipo = TIPOS.find((t) => seccaoOf(t).key === now?.key)
  const noitesCampo = i.atividades.reduce((sum, a) => sum + a.noitesCampo, 0)
  return (
    <div className="seccao-view" style={{ '--seccao': now?.color ?? 'var(--accent)' } as CSSProperties}>
      <div className="panel summary">
        <div>
          {nowTipo && (
            <Link to={paths.seccao(code, seccaoOf(nowTipo).key)} className="back">
              ← {seccaoOf(nowTipo).label}
            </Link>
          )}
          <h2 className="member-name">
            <Identicon seed={m.ref} size={48} />
            {i.totem}
          </h2>
          <p className="hint">
            {[
              i.categoria,
              i.situacao,
              m.age != null && `${m.age} anos`,
              !i.present && 'já não aparece no agrupamento',
            ]
              .filter(Boolean)
              .join(' · ')}
          </p>
        </div>
        <div className="tiles">
          <Tile label="Admissão" value={i.admittedOn ? formatIsoDate(i.admittedOn) : '-'} />
          <Tile label="Promessa" value={i.promisedOn ? formatIsoDate(i.promisedOn) : '-'} />
          <Tile label="Especialidades" value={String(i.especialidades.length)} />
          <Tile label="Noites de campo" value={String(noitesCampo)} />
        </div>
      </div>
      <Progresso m={m} />
      <Percurso code={code} m={m} />
      <Timeline m={m} />
    </div>
  )
}

function Tile({ label, value }: { label: string; value: string }) {
  return (
    <div className="tile">
      <span className="tile-label">{label}</span>
      <span className="tile-value" data-size="small">
        {value}
      </span>
    </div>
  )
}

/** Each secção they have any progresso in: its steps (SIIE's names) with their dates. */
function Progresso({ m }: { m: Member }) {
  const rows = TIPOS.map((t) => ({ t, dates: m.info.progresso[t - 1] ?? [] })).filter((r) =>
    r.dates.some((d) => d != null),
  )
  return (
    <section className="panel" aria-labelledby="member-progresso">
      <h2 id="member-progresso">Progresso</h2>
      {rows.length === 0 ? (
        <p className="hint">Nenhuma etapa nem promessa registada no SIIE.</p>
      ) : (
        <div className="member-progresso">
          {rows.map(({ t, dates }) => (
            <div key={t} className="member-seccao" style={{ '--seccao': seccaoOf(t).color } as CSSProperties}>
              <h3>{seccaoOf(t).label}</h3>
              <ol className="steps">
                {/* The steps, then the promessa (the last date in the row). */}
                {[...ETAPAS[t], 'Promessa'].map((name, k) => (
                  <li key={name} data-done={dates[k] != null || undefined}>
                    <span>{name}</span>
                    <span className="scout-sub">{dates[k] ? formatIsoDate(dates[k]!) : 'sem data'}</span>
                  </li>
                ))}
              </ol>
            </div>
          ))}
        </div>
      )}
    </section>
  )
}

/** "2024/25" */
const yearLabel = (y: number) => `${y}/${String((y + 1) % 100).padStart(2, '0')}`

/** "Instrutor (Exploradores)": a cargo with the secção it belongs to, when known. */
const cargoLabel = (c: Member['info']['cargos'][number]) =>
  [c.cargo, c.seccaoTipo && `(${seccaoOf(c.seccaoTipo).label})`].filter(Boolean).join(' ')

/** Their cargos over the years, with the secção of each: not progresso as such, but the path of a
 * dirigente through the secções (instrutor in one, chefe in another...). */
function Percurso({ code, m }: { code: number; m: Member }) {
  const cargos = [...m.info.cargos].sort((a, b) => (a.startedOn ?? '').localeCompare(b.startedOn ?? ''))
  if (cargos.length === 0) return null
  return (
    <section className="panel" aria-labelledby="member-percurso">
      <h2 id="member-percurso">Percurso de dirigente</h2>
      <div className="table-wrap">
        <table className="table timeline">
          <thead>
            <tr>
              <th>Cargo</th>
              <th>Secção</th>
              <th>Anos escutistas</th>
              <th>Agrupamento</th>
            </tr>
          </thead>
          <tbody>
            {cargos.map((c, k) => {
              const s = c.seccaoTipo ? seccaoOf(c.seccaoTipo) : null
              const from = c.startedOn ? yearLabel(scoutYear(c.startedOn)) : '?'
              const to = c.endedOn ? yearLabel(scoutYear(c.endedOn)) : c.active ? 'hoje' : '?'
              return (
                <tr key={k}>
                  <td className="wrap">{c.cargo}</td>
                  <td>
                    {s ? (
                      <span className="seccao-dot" style={{ '--seccao': s.color } as CSSProperties}>
                        {s.label}
                      </span>
                    ) : (
                      <span className="muted">-</span>
                    )}
                  </td>
                  <td>{from === to ? from : `${from} a ${to}`}</td>
                  <td>
                    {c.agrupamento == null ? (
                      <span className="muted">Fora de agrupamento</span>
                    ) : c.agrupamento === code ? (
                      'Este'
                    ) : (
                      formatCode('agrupamento', c.agrupamento)
                    )}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
    </section>
  )
}

/** Their history, newest year first: secção, steps reached, especialidades, cargos, cursos and
 * atividades in each ano escutista. */
function Timeline({ m }: { m: Member }) {
  const i = m.info
  // Every year with something on record, the years they were counted in included.
  const years = new Set<number>(m.years.map((y) => y.year))
  const dated = (on: string | null) => (on ? scoutYear(on) : null)
  i.progresso.flat().forEach((on) => on && years.add(scoutYear(on)))
  i.especialidades.forEach((e) => e.on && years.add(scoutYear(e.on)))
  i.cursos.forEach((c) => c.on && years.add(scoutYear(c.on)))
  i.atividades.forEach((a) => years.add(a.year))
  // Every year a cargo covers, up to its end (or this year while it lasts).
  const thisYear = m.years.at(-1)?.year
  i.cargos.forEach((c) => {
    if (!c.startedOn) return
    const last = c.endedOn ? scoutYear(c.endedOn) : (thisYear ?? scoutYear(c.startedOn))
    for (let y = scoutYear(c.startedOn); y <= last; y++) years.add(y)
  })
  const sorted = [...years].sort((a, b) => b - a)

  const steps = (y: number) =>
    TIPOS.flatMap((t) =>
      [...ETAPAS[t], 'Promessa'].filter((_, k) => dated(i.progresso[t - 1]?.[k] ?? null) === y),
    )
  // A cargo counts in every year it overlaps (October 1 to September 30).
  const cargosIn = (y: number) =>
    i.cargos.filter(
      (c) => c.startedOn && c.startedOn <= `${y + 1}-09-30` && (!c.endedOn || c.endedOn >= `${y}-10-01`),
    )
  const undated = [
    ...i.especialidades.filter((e) => !e.on).map((e) => e.name),
    ...i.cursos.filter((c) => !c.on).map((c) => c.curso),
  ].filter(Boolean)

  return (
    <section className="panel" aria-labelledby="member-timeline">
      <h2 id="member-timeline">Ano a ano</h2>
      <div className="table-wrap">
        <table className="table timeline">
          <thead>
            <tr>
              <th>Ano escutista</th>
              <th>Secção</th>
              <th>Progresso</th>
              <th>Especialidades</th>
              <th>Cargos</th>
              <th>Cursos</th>
              <th>Atividades</th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((y) => {
              const counted = m.years.find((r) => r.year === y)
              const s = counted && SECCOES.find((s) => s.key === counted.bucket)
              const a = i.atividades.find((a) => a.year === y)
              return (
                <tr key={y}>
                  <td>{yearLabel(y)}</td>
                  <td>
                    {s ? (
                      <span className="seccao-dot" style={{ '--seccao': s.color } as CSSProperties}>
                        {s.label}
                        {counted.byAge && <span className="scout-sub"> (por idade)</span>}
                        {/* A dirigente's year: the secções their cargos were in. */}
                        {counted.bucket === 'dirigentes' &&
                          [...new Set(cargosIn(y).flatMap((c) => (c.seccaoTipo ? [c.seccaoTipo] : [])))].map((t) => (
                            <span key={t} className="scout-sub">
                              {' '}
                              · {seccaoOf(t).label}
                            </span>
                          ))}
                      </span>
                    ) : (
                      <span className="muted">-</span>
                    )}
                  </td>
                  <td className="wrap">{steps(y).join(', ')}</td>
                  <td className="wrap">
                    {i.especialidades
                      .filter((e) => dated(e.on) === y)
                      .map((e) => e.name)
                      .join(', ')}
                  </td>
                  <td className="wrap">{cargosIn(y).map(cargoLabel).join(', ')}</td>
                  <td className="wrap">
                    {i.cursos
                      .filter((c) => dated(c.on) === y)
                      .map((c) => c.curso)
                      .join(', ')}
                  </td>
                  <td className="wrap">
                    {a && `${a.count} (${a.noites} noites, ${a.noitesCampo} de campo)`}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      {undated.length > 0 && <p className="hint">Sem data no SIIE: {undated.join(', ')}.</p>}
    </section>
  )
}
