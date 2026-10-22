import { useState, type ReactNode } from 'react'
import { agrupamentoName, formatAgrupamento, formatCode, formatIsoDate, formatUnixTime } from '../../../lib/format'
import { navigate, paths, type Route } from '../../../lib/router'
import { Button } from '../../../ui/Button'
import { EmptyState } from '../../../ui/EmptyState'
import { Link } from '../../../ui/Link'
import { Segmented } from '../../../ui/Segmented'
import type { Me } from '../../auth/schema'
import { useDigest } from '../api'
import { MembersChart } from '../components/MembersChart'
import { MemberView } from '../components/MemberView'
import { SeccaoCards } from '../components/SeccaoCards'
import { SeccaoView } from '../components/SeccaoView'
import type { Digest } from '../schema'
import { isAged, SECCOES, seccaoOf, tipoOfSlug, TIPOS } from '../series'

type Mode = 'total' | 'seccoes'
type AgrupamentoRoute = Extract<Route, { name: 'agrupamento' }>

/** One agrupamento's digest, a secção of it, or one of its members, as the URL says. */
export function AgrupamentoPage({ me, route }: { me: Me; route: AgrupamentoRoute }) {
  // Only whole agrupamentos have imports, so only those have a digest.
  const whole = me.access.agrupamentos.filter((a) => a.seccao == null)
  // /agrupamentos alone (or one not held whole) opens the first one.
  const code = whole.some((a) => a.code === route.code) ? route.code : (whole[0]?.code ?? null)

  return (
    <section className="page">
      <div className="page-head">
        <div>
          <h1>Agrupamento</h1>
          <p>
            Elementos por ano escutista (outubro a setembro), desde o primeiro registo. Cada elemento conta na secção
            em que estava no fim do ano.
          </p>
        </div>
        {whole.length > 1 && code != null && (
          <label className="picker">
            <span>Agrupamento</span>
            <select value={code} onChange={(e) => navigate(paths.agrupamento(Number(e.target.value)))}>
              {whole.map((a) => (
                <option key={a.code} value={a.code}>
                  {formatAgrupamento(a.code, a.name)}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {code == null ? (
        <EmptyState
          title="Sem agrupamentos inteiros"
          body="O resumo só aparece para agrupamentos que a sua conta SIIE vê por inteiro. Com acesso a uma só secção, os elementos não são importados."
        />
      ) : (
        <DigestView key={code} code={code} route={route} />
      )}
    </section>
  )
}

function DigestView({ code, route }: { code: number; route: AgrupamentoRoute }) {
  const digest = useDigest(code)
  const [mode, setMode] = useState<Mode>('total')
  const tipo = route.seccao ? tipoOfSlug(route.seccao) : null

  if (digest.isPending) {
    return (
      <div className="panel" aria-busy>
        <span className="skeleton" />
      </div>
    )
  }
  if (digest.isError) {
    return (
      <EmptyState
        title="Não foi possível carregar o resumo"
        body={digest.error.message}
        action={<Button onClick={() => digest.refetch()}>Tentar novamente</Button>}
      />
    )
  }
  const d = digest.data
  return (
    <>
      <Header d={d} />
      {d.years.length === 0 ? (
        <EmptyState
          title="Ainda sem dados"
          body="Os elementos deste agrupamento ainda não foram importados. A importação corre em segundo plano depois de um início de sessão."
        />
      ) : (
        <>
          <nav className="tabs" aria-label="Secções">
            <Link to={paths.agrupamento(code)} className="nav-link" aria-current={!tipo && !route.member ? 'page' : undefined}>
              Resumo
            </Link>
            {TIPOS.map((t) => (
              <Link
                key={t}
                to={paths.seccao(code, seccaoOf(t).key)}
                className="nav-link"
                aria-current={tipo === t ? 'page' : undefined}
              >
                {seccaoOf(t).label}
              </Link>
            ))}
          </nav>
          {route.member ? (
            <MemberView key={route.member} code={code} memberRef={route.member} />
          ) : tipo ? (
            <SeccaoView key={tipo} code={code} tipo={tipo} />
          ) : (
            <>
              <SeccaoCards code={code} years={d.years} defs={d.criteriaDefs} criteria={d.criteria} />
              <div className="panel digest">
                <Segmented
                  name="digest-mode"
                  legend="Mostrar"
                  value={mode}
                  onChange={setMode}
                  options={[
                    { value: 'total', label: 'Total' },
                    { value: 'seccoes', label: 'Por secção' },
                  ]}
                />
                <MembersChart rows={d.years} mode={mode} />
                {d.undated > 0 && (
                  <p className="hint">
                    {d.undated === 1
                      ? '1 antigo elemento não tem nenhuma data no SIIE (admissão, adesão, promessa ou cargo) e fica fora do gráfico.'
                      : `${d.undated} antigos elementos não têm nenhuma data no SIIE (admissão, adesão, promessa ou cargo) e ficam fora do gráfico.`}
                  </p>
                )}
                <YearsTable d={d} />
              </div>
            </>
          )}
        </>
      )}
    </>
  )
}

/** The agrupamento's own data, the current year's count and how fresh it is. */
function Header({ d }: { d: Digest }) {
  const a = d.agrupamento
  const current = d.years.at(-1)
  const first = d.years[0]
  const unit = (kind: 'nucleo' | 'regiao', label: string, code: number | null, name: string | null) =>
    code != null && [`${label} ${formatCode(kind, code)}`, name].filter(Boolean).join(' ')
  // Only what SIIE has: empty fields are left out.
  const facts: [string, ReactNode][] = [
    ['Paróquia', a.paroquia],
    ['Patrono', a.patrono],
    ['Localidade', [a.cp4, a.localidade].filter(Boolean).join(' ')],
    ['Iniciado em', a.iniciadoOn && formatIsoDate(a.iniciadoOn)],
    ['Filiado em', a.filiadoOn && formatIsoDate(a.filiadoOn)],
    ['Email', a.email && <a key="email" href={`mailto:${a.email}`}>{a.email}</a>],
    ['Email de notificações', a.emailnotif !== a.email && a.emailnotif],
    ['Telefone', a.telefone],
    ['Site', a.site && <a key="site" href={a.site.startsWith('http') ? a.site : `https://${a.site}`}>{a.site}</a>],
  ]
  return (
    <div className="panel summary">
      <div>
        <h2>
          {agrupamentoName(a.name) ?? `Agrupamento ${formatCode('agrupamento', a.code)}`}
          {a.estado && (
            <span className="badge tag" data-tone={a.estado === 'Ativo' ? 'accent' : 'danger'}>
              {a.estado}
            </span>
          )}
          {a.tipo && <span className="badge tag">{a.tipo}</span>}
        </h2>
        <p className="hint">
          {[
            `Agrupamento ${formatCode('agrupamento', a.code)}`,
            unit('nucleo', 'Núcleo', a.nucleo, a.nucleoName),
            unit('regiao', 'Região', a.regiao, a.regiaoName),
          ]
            .filter(Boolean)
            .join(' · ')}
        </p>
      </div>
      {facts.some(([, v]) => v) && (
        <dl className="facts">
          {facts
            .filter(([, v]) => v)
            .map(([label, v]) => (
              <div key={label}>
                <dt>{label}</dt>
                <dd>{v}</dd>
              </div>
            ))}
        </dl>
      )}
      {current && (
        <div className="tiles">
          <Tile label={`Elementos em ${current.label}`} value={current.total} />
          <Tile label="Primeiro registo" value={first.label} />
          <Tile label="Última importação" value={formatUnixTime(d.lastImportAt, 'Nunca')} small />
        </div>
      )}
    </div>
  )
}

function Tile({ label, value, small }: { label: string; value: string | number; small?: boolean }) {
  return (
    <div className="tile">
      <span className="tile-label">{label}</span>
      <span className="tile-value" data-size={small ? 'small' : undefined}>
        {value}
      </span>
    </div>
  )
}

/** The same numbers as the chart, newest year first. */
function YearsTable({ d }: { d: Digest }) {
  return (
    <details>
      <summary>Ver a tabela</summary>
      <p className="hint">Entre parênteses: quantos estão na secção por estimativa de idade, sem registo de progresso nesse ano.</p>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Ano escutista</th>
              {SECCOES.map((s) => (
                <th key={s.key} className="num">
                  {s.label}
                </th>
              ))}
              <th className="num">Total</th>
            </tr>
          </thead>
          <tbody>
            {[...d.years].reverse().map((r) => (
              <tr key={r.year}>
                <td>{r.label}</td>
                {SECCOES.map((s) => (
                  <td key={s.key} className="num">
                    {r[s.key]}
                    {isAged(s.key) && r.byAge[s.key] > 0 && <span className="aged"> ({r.byAge[s.key]})</span>}
                  </td>
                ))}
                <td className="num">
                  <b>{r.total}</b>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </details>
  )
}
