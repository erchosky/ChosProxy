import { describe, expect, it } from "vitest";
import type { GraphqlSchema } from "../../bindings/GraphqlSchema";
import { rootFields } from "./GraphqlPanel";

const field = (name: string, typeRef: string) => ({
  name,
  description: "",
  args: [],
  typeRef,
  deprecated: false,
});

const type = (name: string, fields: ReturnType<typeof field>[]) => ({
  kind: "OBJECT",
  name,
  fields,
  inputFields: [],
  enumValues: [],
  possibleTypes: [],
});

describe("rootFields", () => {
  it("lists queries then mutations", () => {
    const schema: GraphqlSchema = {
      queryType: "Q",
      mutationType: "M",
      subscriptionType: null,
      types: [type("M", [field("rename", "Item")]), type("Q", [field("items", "[Item]")])],
    };
    expect(rootFields(schema)).toEqual([
      { root: "query", name: "items", type: "[Item]" },
      { root: "mutation", name: "rename", type: "Item" },
    ]);
  });

  it("copes with a schema without mutations", () => {
    const schema: GraphqlSchema = {
      queryType: "Q",
      mutationType: null,
      subscriptionType: null,
      types: [type("Q", [])],
    };
    expect(rootFields(schema)).toEqual([]);
  });
});
