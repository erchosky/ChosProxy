import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RENDER_CSP, RenderView, renderDocument } from "./RenderView";

describe("RenderView", () => {
  it("sandboxes the frame completely and injects a blocking CSP", () => {
    const { container } = render(
      <RenderView title="Response" html="<script>alert(1)</script><p>x</p>" />,
    );
    const frame = container.querySelector("iframe");
    expect(frame?.getAttribute("sandbox")).toBe("");
    expect(frame?.getAttribute("srcdoc")).toContain(RENDER_CSP);
    expect(RENDER_CSP).toContain("default-src 'none'");
    expect(renderDocument("<p>x</p>").indexOf("Content-Security-Policy")).toBeLessThan(
      renderDocument("<p>x</p>").indexOf("<p>x</p>"),
    );
  });

  it("refuses very large documents", () => {
    const { container } = render(<RenderView title="R" html={"x".repeat(2 * 1024 * 1024 + 1)} />);
    expect(container.querySelector("iframe")).toBeNull();
  });
});
