// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createContext, useContext } from "react";
import { AnnotatePane } from "@/components/workspace/editor-panes";
import type { ViewerBackend } from "@/viewer/types";

afterEach(cleanup);

const ProviderContext = createContext("outside-provider");

const source = { id: "doc-1", bytes: new Uint8Array([1]), name: "doc.pdf" } as never;

const data = {
  entry: {
    status: "ready" as const,
    value: {
      pages: [1, 2, 3].map((page) => ({ page, widthPt: 200, heightPt: 300 })),
      fields: [],
      annotations: [],
    },
  },
  retry: () => undefined,
};

function ContextPreview({ page }: { page: number }) {
  return <span data-testid="preview">{`${useContext(ProviderContext)}:${page}`}</span>;
}

/** A provider that, like the real one, hands `children` a fresh closure on every render. */
function contextBackend() {
  const renders = { count: 0 };
  const backend: ViewerBackend = {
    Viewer: () => null,
    PagePreviewProvider: ({ children }) => {
      renders.count += 1;
      return (
        <ProviderContext.Provider value="inside-provider">
          {children((page) => (
            <ContextPreview page={page} />
          ))}
        </ProviderContext.Provider>
      );
    },
  };
  return { backend, renders };
}

describe("AnnotatePane page previews", () => {
  it("keeps the page when the viewer backend becomes ready, and renders inside the provider", () => {
    const { backend, renders } = contextBackend();
    // happy path needs a resolvable wasm URL
    const props = {
      docKey: "doc-1",
      source,
      data,
      busy: false,
      location: { page: 1 },
      onLocationChange: () => undefined,
      onApply: () => undefined,
    };
    const { rerender } = render(<AnnotatePane {...props} backend={{ status: "loading" }} />);
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(screen.queryByTestId("preview")).toBeNull();

    act(() => {
      rerender(<AnnotatePane {...props} backend={{ status: "ready", backend }} />);
    });

    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(screen.getByTestId("preview").textContent).toBe("inside-provider:2");
    expect(renders.count).toBeLessThan(10);
  });
});
