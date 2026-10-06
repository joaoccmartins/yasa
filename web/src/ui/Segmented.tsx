type Option<T extends string> = { value: T; label: string }

type Props<T extends string> = {
  name: string
  legend: string
  options: Option<T>[]
  value: T
  onChange: (value: T) => void
}

/** A radio group drawn as a two-or-more way switch. Real radios, so keyboard and labels work. */
export function Segmented<T extends string>({ name, legend, options, value, onChange }: Props<T>) {
  return (
    <fieldset className="field">
      <legend>{legend}</legend>
      <div className="segmented">
        {options.map((o) => (
          <label key={o.value}>
            <input
              type="radio"
              name={name}
              value={o.value}
              checked={value === o.value}
              onChange={() => onChange(o.value)}
            />
            {o.label}
          </label>
        ))}
      </div>
    </fieldset>
  )
}
