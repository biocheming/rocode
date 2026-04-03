interface StructuredDataViewProps {
  value: unknown;
  emptyLabel?: string;
  onNavigateKeyValue?: (key: string, value: string) => void;
}

function valueTypeLabel(value: unknown) {
  if (Array.isArray(value)) return `Array(${value.length})`;
  if (value === null) return "null";
  return typeof value;
}

function PrimitiveValue({ value }: { value: unknown }) {
  if (typeof value === "string") {
    return <pre className="whitespace-pre-wrap break-words text-sm">{value}</pre>;
  }
  return <code className="rounded bg-muted px-1.5 py-0.5 text-sm font-mono">{String(value)}</code>;
}

export function StructuredDataView({
  value,
  emptyLabel = "No structured data.",
  onNavigateKeyValue,
}: StructuredDataViewProps) {
  if (value === null || value === undefined) {
    return <p className="text-sm text-muted-foreground italic">{emptyLabel}</p>;
  }

  if (typeof value !== "object") {
    return (
      <div className="grid gap-2.5">
        <PrimitiveValue value={value} />
      </div>
    );
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return <p className="text-sm text-muted-foreground italic">{emptyLabel}</p>;
    }

    return (
      <div className="grid gap-2.5">
        {value.map((entry, index) => (
          <details key={`array-entry-${index}`} className="rounded-xl border border-border bg-card/70 p-2.5" open={index < 2}>
            <summary>
              [{index}] <span>{valueTypeLabel(entry)}</span>
            </summary>
            <StructuredDataView value={entry} emptyLabel="Empty item." />
          </details>
        ))}
      </div>
    );
  }

  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length === 0) {
    return <p className="text-sm text-muted-foreground italic">{emptyLabel}</p>;
  }

  const scalarEntries = entries.filter(([, entry]) => entry === null || typeof entry !== "object");
  const nestedEntries = entries.filter(([, entry]) => entry !== null && typeof entry === "object");

  return (
    <div className="grid gap-2.5">
      {scalarEntries.length ? (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
          {scalarEntries.map(([key, entry]) => (
            <div key={key}>
              <dt>{key}</dt>
              <dd>
                  <PrimitiveValue value={entry} />
                  {onNavigateKeyValue && typeof entry === "string" ? (
                    <button
                      className="text-xs text-primary underline underline-offset-2 hover:text-primary/80 transition-colors"
                      type="button"
                      onClick={() => onNavigateKeyValue(key, entry)}
                    >
                      Open
                    </button>
                  ) : null}
                </dd>
              </div>
          ))}
        </dl>
      ) : null}
      {nestedEntries.map(([key, entry]) => (
        <details key={key} className="rounded-xl border border-border bg-card/70 p-2.5" open>
          <summary>
            {key} <span>{valueTypeLabel(entry)}</span>
          </summary>
          <StructuredDataView value={entry} emptyLabel={`No data in ${key}.`} />
        </details>
      ))}
    </div>
  );
}
