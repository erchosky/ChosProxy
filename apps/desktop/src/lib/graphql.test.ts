import { describe, expect, it } from "vitest";
import { bodyOf, replaceBody } from "./httpText";
import { formatGraphql, graphqlParts, withGraphql } from "./graphql";

describe("graphqlParts", () => {
  it("reads a single operation and a batch", () => {
    const one = graphqlParts(
      '{"query":"query A($id: ID!) { item(id: $id) { id name } }","variables":{"id":"1"},"operationName":"A"}',
    );
    expect(one?.operationName).toBe("A");
    expect(one?.variables).toBe('{\n  "id": "1"\n}');
    expect(one?.query).toBe("query A($id: ID!) {\n  item(id: $id) {\n    id name\n  }\n}");
    expect(graphqlParts('[{"query":"{ a }"},{"query":"{ b }"}]')?.batch).toBe(2);
  });

  it("ignores bodies that are not GraphQL", () => {
    expect(graphqlParts(null)).toBeNull();
    expect(graphqlParts("a=1")).toBeNull();
    expect(graphqlParts('{"query":"select 1"}')).toBeNull();
    expect(graphqlParts('{"q":"{ a }"}')).toBeNull();
  });
});

describe("formatGraphql", () => {
  it("keeps strings intact", () => {
    expect(formatGraphql('{ a(s: "x { y }") { b } }')).toBe(
      '{\n  a(s: "x { y }") {\n    b\n  }\n}',
    );
  });
});

describe("editing a GraphQL request", () => {
  const body =
    '{"operationName":"Q","query":"query Q { a }","variables":{"id":1},"extensions":{"x":1}}';
  const request = `POST /graphql HTTP/1.1\r\nHost: lab\r\nContent-Length: ${body.length}\r\n\r\n${body}`;

  it("writes the query and variables back, keeping the rest", () => {
    const next = withGraphql(bodyOf(request), "query Q { b }", '{ "id": 2 }');
    expect(next).not.toBeNull();
    if (!next || "error" in next) throw new Error("no body");
    expect(JSON.parse(next.body)).toEqual({
      operationName: "Q",
      query: "query Q { b }",
      variables: { id: 2 },
      extensions: { x: 1 },
    });
    const message = replaceBody(request, next.body);
    expect(message).toContain(`Content-Length: ${next.body.length}\r\n\r\n`);
    expect(bodyOf(message ?? "")).toBe(next.body);
  });

  it("keeps the other operations of a batch", () => {
    const batch = '[{"query":"{ a }"},{"query":"{ z }"}]';
    const next = withGraphql(batch, "{ b }", "");
    if (!next || "error" in next) throw new Error("no body");
    expect(JSON.parse(next.body)).toEqual([{ query: "{ b }", variables: {} }, { query: "{ z }" }]);
  });

  it("refuses variables that are not a JSON object, and other bodies", () => {
    expect(withGraphql(body, "{ a }", "{nope")).toEqual({
      error: "The variables are not valid JSON.",
    });
    expect(withGraphql(body, "{ a }", "[1]")).toEqual({
      error: "The variables must be a JSON object.",
    });
    expect(withGraphql('{"a":1}', "{ a }", "{}")).toBeNull();
    expect(replaceBody("GET / HTTP/1.1", "x")).toBeNull();
    expect(replaceBody("POST / HTTP/1.1\nTransfer-Encoding: chunked\n\n0", "x")).toBeNull();
    expect(replaceBody("POST / HTTP/1.1\nHost: a\n\nold", "né")).toBe(
      "POST / HTTP/1.1\nHost: a\nContent-Length: 3\n\nné",
    );
  });
});
