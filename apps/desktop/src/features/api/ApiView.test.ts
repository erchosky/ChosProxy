import { describe, expect, it } from "vitest";
import type { ApiEndpoint } from "../../bindings/ApiEndpoint";
import { filterEndpoints, isReadOnly } from "./ApiView";

const endpoint = (method: string, path: string, name: string, group = ""): ApiEndpoint => ({
  method,
  url: `https://api.test${path}`,
  path,
  name,
  group,
  params: [],
  headers: [],
  contentType: null,
  body: null,
  readOnly: null,
});

describe("filterEndpoints", () => {
  const list = [
    endpoint("GET", "/users", "listUsers", "users"),
    endpoint("DELETE", "/users/{id}", "deleteUser", "users"),
    endpoint("POST", "/orders", "createOrder", "orders"),
  ];

  it("keeps everything without a filter", () => {
    expect(filterEndpoints(list, "  ")).toEqual([0, 1, 2]);
  });

  it("matches method, path, name and group, ignoring case", () => {
    expect(filterEndpoints(list, "delete")).toEqual([1]);
    expect(filterEndpoints(list, "USERS")).toEqual([0, 1]);
    expect(filterEndpoints(list, "order")).toEqual([2]);
    expect(filterEndpoints(list, "nothing")).toEqual([]);
  });
});

describe("isReadOnly", () => {
  it("follows the method unless the endpoint says otherwise", () => {
    expect(isReadOnly(endpoint("get", "/a", "a"))).toBe(true);
    expect(isReadOnly(endpoint("POST", "/a", "a"))).toBe(false);
    expect(isReadOnly({ ...endpoint("POST", "/graphql", "items"), readOnly: true })).toBe(true);
  });
});
