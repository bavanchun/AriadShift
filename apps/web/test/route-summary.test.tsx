// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import { RouteSummary } from "@/components/route-summary";

describe("RouteSummary", () => {
  it("lists the route and its scores", () => {
    render(
      <RouteSummary
        route={{
          steps: [
            { from: "markdown", to: "ariad-ir+json", engine: "ariad-core" },
            { from: "ariad-ir+json", to: "html", engine: "ariad-core" },
          ],
          score: { fidelity: 1, editability: 0.981, estimated_duration_ms: 0 },
          measured: true,
          alternatives: [],
        }}
      />,
    );
    const route = screen.getByRole("list", { name: "Conversion route" });
    expect(within(route).getAllByRole("listitem")).toHaveLength(3);
    expect(route.textContent).toContain("Markdown");
    expect(route.textContent).toContain("HTML");
    expect(screen.getByText("100%")).toBeTruthy();
    expect(screen.getByText("98%")).toBeTruthy();
    expect(screen.queryByText(/not been benchmarked/)).toBeNull();
  });

  it("flags unmeasured routes", () => {
    render(
      <RouteSummary
        route={{
          steps: [{ from: "pdf", to: "ariad-ir+json", engine: "docling" }],
          score: { fidelity: null, editability: null, estimated_duration_ms: null },
          measured: false,
          alternatives: [],
        }}
      />,
    );
    expect(screen.getAllByText("not measured")).toHaveLength(3);
    expect(screen.getByText(/not been benchmarked/)).toBeTruthy();
  });
});
