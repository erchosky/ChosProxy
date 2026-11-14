import { useMemo } from "react";
import {
  applyInspection,
  type Header,
  type Inspection,
  inspectRequest,
  inspectResponse,
  MAX_FIELDS,
  type Param,
} from "../../lib/inspector";
import { IconButton } from "../Button";
import { EmptyState } from "../States";

interface InspectorPanelProps {
  /** The request in text form. */
  text: string;
  /** When set, edits rewrite the text and are reported here. */
  onChange?(text: string): void;
}

/** Query parameters, form body, cookies and headers of a request, or the
 * headers and cookies a response sets. */
export function InspectorPanel({ text, onChange }: InspectorPanelProps) {
  const response = text.startsWith("HTTP/");
  return response ? (
    <ResponseInspector text={text} />
  ) : (
    <RequestInspector text={text} onChange={onChange} />
  );
}

function ResponseInspector({ text }: { text: string }) {
  const inspection = useMemo(() => inspectResponse(text), [text]);
  if (!inspection) return <EmptyState title="Not an HTTP response" />;
  return (
    <div className="inspector" aria-label="Inspector">
      <section className="inspector-section">
        <header>
          <strong>Cookies set</strong>
          <span className="muted">{inspection.cookies.length}</span>
        </header>
        {inspection.cookies.length > 0 && (
          <div className="inspector-rows" role="table" aria-label="Cookies set">
            {inspection.cookies.map((c, i) => (
              <div className="inspector-row readonly cookie-row" role="row" key={i}>
                <span className="mono selectable">{c.name}</span>
                <span className="mono selectable">{c.value}</span>
                <span className="muted selectable">{c.attributes}</span>
              </div>
            ))}
          </div>
        )}
      </section>
      <Section title="Headers" rows={inspection.headers} editable={false} onChange={() => {}} />
    </div>
  );
}

function RequestInspector({ text, onChange }: InspectorPanelProps) {
  const inspection = useMemo(() => inspectRequest(text), [text]);
  if (!inspection) return <EmptyState title="Not an HTTP request" />;
  const editable = onChange !== undefined;
  const jsonBody =
    inspection.bodyType !== null &&
    (inspection.bodyType.endsWith("/json") || inspection.bodyType.endsWith("+json"));
  const rangedBody = inspection.bodyFields?.some((f) => f.range !== undefined) ?? false;
  const update = (patch: Partial<Inspection>) =>
    onChange?.(applyInspection(text, { ...inspection, ...patch }));

  return (
    <div className="inspector" aria-label="Inspector">
      <Section
        title="Query parameters"
        rows={inspection.query}
        editable={editable}
        onChange={(query) => update({ query })}
      />
      {inspection.form !== null ? (
        <Section
          title="Body parameters"
          rows={inspection.form}
          editable={editable}
          onChange={(form) => update({ form })}
        />
      ) : inspection.bodyFields !== null ? (
        <>
          <Section
            title={`Body fields (${inspection.bodyType ?? "body"})`}
            rows={inspection.bodyFields}
            editable={(jsonBody || rangedBody) && editable}
            valuesOnly
            onChange={(bodyFields) => update({ bodyFields })}
          />
          {(editable || inspection.bodyFields.length >= MAX_FIELDS) && (
            <p className="muted inspector-note">
              {inspection.bodyFields.length >= MAX_FIELDS && `First ${MAX_FIELDS} fields. `}
              {editable &&
                (jsonBody
                  ? "Editing a value rewrites the JSON body compactly; add or remove fields in the text editor."
                  : rangedBody
                    ? "Editing a value changes only that value (XML is escaped); file contents and structure are edited in the text editor."
                    : "Edit these in the text editor.")}
            </p>
          )}
        </>
      ) : (
        inspection.bodyType && (
          <p className="muted inspector-note">
            Body: {inspection.bodyType} (edit it in the text editor)
          </p>
        )
      )}
      <Section
        title="Cookies"
        rows={inspection.cookies}
        editable={editable}
        onChange={(cookies) => update({ cookies })}
      />
      <Section
        title="Headers"
        rows={inspection.headers}
        editable={editable}
        onChange={(headers) => update({ headers })}
      />
    </div>
  );
}

interface SectionProps<T extends Param | Header> {
  title: string;
  rows: T[];
  editable: boolean;
  /** Only values can change (no renaming, adding or removing). */
  valuesOnly?: boolean;
  onChange(rows: T[]): void;
}

function Section<T extends Param | Header>({
  title,
  rows,
  editable,
  valuesOnly = false,
  onChange,
}: SectionProps<T>) {
  const set = (i: number, patch: Partial<Header>) =>
    onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <section className="inspector-section">
      <header>
        <strong>{title}</strong>
        <span className="muted">{rows.length}</span>
        <span className="spacer" />
        {editable && !valuesOnly && (
          <IconButton
            icon="plus"
            label={`Add to ${title.toLowerCase()}`}
            onClick={() => onChange([...rows, { name: "name", value: "" } as T])}
          />
        )}
      </header>
      {rows.length > 0 && (
        <div className="inspector-rows" role="table" aria-label={title}>
          {rows.map((r, i) =>
            editable ? (
              // Rows have no identity besides their position.
              <div className="inspector-row" role="row" key={i}>
                <input
                  className="mono"
                  aria-label={`${title} ${i + 1} name`}
                  value={r.name}
                  readOnly={valuesOnly}
                  onChange={(e) => set(i, { name: e.target.value })}
                />
                <input
                  className="mono"
                  aria-label={`${title} ${i + 1} value`}
                  value={r.value}
                  onChange={(e) => set(i, { value: e.target.value })}
                />
                {!valuesOnly && (
                  <IconButton
                    icon="trash"
                    label={`Remove ${r.name}`}
                    onClick={() => onChange(rows.filter((_, j) => j !== i))}
                  />
                )}
              </div>
            ) : (
              <div className="inspector-row readonly" role="row" key={i}>
                <span className="mono selectable">{r.name}</span>
                <span className="mono selectable">{r.value}</span>
              </div>
            ),
          )}
        </div>
      )}
    </section>
  );
}
