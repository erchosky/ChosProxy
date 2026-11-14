import { describe, expect, it } from "vitest";
import { curlExample } from "./AutomationSection";

describe("curlExample", () => {
  it("names the configured port and reads the token from the environment", () => {
    expect(curlExample({ config: { enabled: true, port: 9000 } })).toBe(
      'curl -H "Authorization: Bearer $CHOSPROXY_API_TOKEN" http://127.0.0.1:9000/v1/status',
    );
  });
});
