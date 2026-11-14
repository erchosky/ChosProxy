import { useEffect, useState } from "react";
import type { GraphqlSchema } from "../../bindings/GraphqlSchema";
import { Button } from "../../components/Button";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useFuzzerStore } from "../../state/fuzzer";
import { useRepeaterStore } from "../../state/repeater";

type Root = "query" | "mutation";

/** The root fields of a schema: queries, then mutations. */
export function rootFields(schema: GraphqlSchema): { root: Root; name: string; type: string }[] {
  const out: { root: Root; name: string; type: string }[] = [];
  const roots: [Root, string | null][] = [
    ["query", schema.queryType],
    ["mutation", schema.mutationType],
  ];
  for (const [root, typeName] of roots) {
    const t = schema.types.find((x) => x.name === typeName);
    for (const f of t?.fields ?? []) out.push({ root, name: f.name, type: f.typeRef });
  }
  return out;
}

/** Schema browser and operation editor of a GraphQL definition (M27). */
export function GraphqlPanel({ id }: { id: number }) {
  const [schema, setSchema] = useState<GraphqlSchema | null>(null);
  const [query, setQuery] = useState("");
  const [variables, setVariables] = useState("{}");
  const [typeName, setTypeName] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .graphqlSchema(id)
      .then(setSchema)
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, [id]);

  async function build(root: Root, field: string) {
    try {
      const op = await api.graphqlBuild(id, root, field);
      setQuery(op.query);
      setVariables(op.variables);
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }

  async function open(tool: "repeater" | "fuzzer") {
    try {
      const req = await api.graphqlRequest(id, query, variables);
      if (tool === "repeater") {
        const t = req.target;
        await useRepeaterStore
          .getState()
          .openRequest(`${t.scheme}://${t.host}:${t.port}/`, req.request);
        useAppStore.getState().setView("repeater");
      } else {
        await useFuzzerStore.getState().openRequest(req.target, req.request);
      }
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }

  if (error) return <p className="callout callout-danger">{error}</p>;
  if (!schema) return null;
  const fields = rootFields(schema);
  const shownType = schema.types.find((t) => t.name === typeName);

  return (
    <details className="graphql-panel" open>
      <summary>
        <strong>Schema</strong>{" "}
        <span className="muted">
          {schema.types.length} types · {fields.filter((f) => f.root === "query").length} queries ·{" "}
          {fields.filter((f) => f.root === "mutation").length} mutations
        </span>
      </summary>
      <div className="graphql-grid">
        <div className="graphql-fields" aria-label="Operations">
          {fields.map((f) => (
            <button
              key={`${f.root}.${f.name}`}
              type="button"
              className="graphql-field"
              title={`Write a ${f.root} for ${f.name}`}
              onClick={() => void build(f.root, f.name)}
            >
              <span className={f.root === "mutation" ? "api-method unsafe" : "api-method"}>
                {f.root === "mutation" ? "M" : "Q"}
              </span>{" "}
              <span className="mono">{f.name}</span> <span className="muted">{f.type}</span>
            </button>
          ))}
        </div>
        <div className="graphql-editor">
          <label className="field-stack">
            Operation
            <textarea
              className="mono"
              rows={10}
              spellCheck={false}
              value={query}
              placeholder="Pick a query or mutation on the left, or write one"
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          <label className="field-stack">
            Variables (JSON)
            <textarea
              className="mono"
              rows={4}
              spellCheck={false}
              value={variables}
              onChange={(e) => setVariables(e.target.value)}
            />
          </label>
          <div className="dialog-row">
            <Button size="sm" disabled={!query.trim()} onClick={() => void open("repeater")}>
              Open in Repeater
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!query.trim()}
              onClick={() => void open("fuzzer")}
            >
              Open in Fuzzer
            </Button>
          </div>
        </div>
      </div>
      <label className="inline-field">
        Type
        <select value={typeName} onChange={(e) => setTypeName(e.target.value)}>
          <option value="">(choose a type to see its fields)</option>
          {schema.types.map((t) => (
            <option key={t.name} value={t.name}>
              {t.name} ({t.kind.toLowerCase()})
            </option>
          ))}
        </select>
      </label>
      {shownType && (
        <ul className="graphql-type mono">
          {shownType.fields.map((f) => (
            <li key={f.name}>
              {f.name}
              {f.args.length > 0 &&
                `(${f.args.map((a) => `${a.name}: ${a.typeRef}`).join(", ")})`}: {f.typeRef}
              {f.deprecated && <span className="muted"> (deprecated)</span>}
            </li>
          ))}
          {shownType.inputFields.map((f) => (
            <li key={f.name}>
              {f.name}: {f.typeRef}
              {f.defaultValue !== null && ` = ${f.defaultValue}`}
            </li>
          ))}
          {shownType.enumValues.map((v) => (
            <li key={v}>{v}</li>
          ))}
          {shownType.possibleTypes.length > 0 && <li>= {shownType.possibleTypes.join(" | ")}</li>}
        </ul>
      )}
    </details>
  );
}
