import { useState, type ReactNode } from 'react'
import { formatCode, formatIsoDate, formatUnixTime } from '../../../lib/format'
import { Button } from '../../../ui/Button'
import { EmptyState } from '../../../ui/EmptyState'
import { Segmented } from '../../../ui/Segmented'
import type { Me } from '../../auth/schema'
import { useDigest } from '../api'
import { MembersChart } from '../components/MembersChart'
import { SeccaoCards } from '../components/SeccaoCards'
import { SeccaoView } from '../components/SeccaoView'
import type { Digest } from '../schema'
import { SECCOES, seccaoOf, TIPOS, type Tipo } from '../series'

type Mode = 'total' | 'seccoes'

/** One agrupamento's digest: its data and its members per ano escutista, since the first on record. */
export function AgrupamentoPage({ me }: { me: Me }) {
  // Only whole agrupamentos have imports, so only those have a digest.
  const whole = me.access.agrupamentos.filter((a) => a.seccao == null)
  const [picked, setPicked] = useState(whole[0]?.code)
  const code = whole.some((a) => a.code === picked) ? picked : whole[0]?.code

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
        {whole.length > 1 && (
          <label className="picker">
            <span>Agrupamento</span>
            <select value={code} onChange={(e) => setPicked(Number(e.target.value))}>
              {whole.map((a) => (
                <option key={a.code} value={a.code}>
                  {[formatCode('agrupamento', a.code), a.name].filter(Boolean).join(' · ')}
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
        <DigestView key={code} code={code} />
      )}
    </section>
  )
}

function DigestView({ code }: { code: number }) {
  const digest = useDigest(code)
  const [mode, setMode] = useState<Mode>('total')
  // 'resumo' = the whole agrupamento; a tipo = that secção's detail.
  const [view, setView] = useState<'resumo' | Tipo>('resumo')

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
            {(['resumo', ...TIPOS] as const).map((v) => (
              <button
                key={v}
                className="nav-link"
                aria-current={view === v ? 'page' : undefined}
                onClick={() => setView(v)}
              >
                {v === 'resumo' ? 'Resumo' : seccaoOf(v).label}
              </button>
            ))}
          </nav>
          {view === 'resumo' ? (
            <>
              <SeccaoCards years={d.years} onOpen={setView} />
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
                <YearsTable d={d} />
              </div>
            </>
          ) : (
            <SeccaoView key={view} code={code} tipo={view} />
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
          {a.name ?? `Agrupamento ${formatCode('agrupamento', a.code)}`}
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
