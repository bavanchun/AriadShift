// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import { loadMessages, scopeMessages } from "@/i18n/messages";

const mocks = vi.hoisted(() => ({
  search: "",
}));

vi.mock("next/navigation", () => ({
  useSearchParams: () => new URLSearchParams(mocks.search),
}));
vi.mock("@/components/converter/converter-app", () => ({
  ConverterApp: () => <div data-testid="converter-stub" />,
}));
vi.mock("@/components/app/tools-host", () => {
  return {
    default: (props: {
      onSelectTool: (id: string) => void;
      defaultPaletteOpen?: boolean;
      onOpenPalette?: () => void;
      onRunRecipe?: (steps: Array<{ toolId: string; params: Record<string, unknown> }>) => void;
    }) => (
      <div data-testid="tools-stub" data-palette={String(props.defaultPaletteOpen)}>
        <button type="button" onClick={() => props.onSelectTool("pdf.rotate")}>
          choose rotate
        </button>
        <button type="button" onClick={() => props.onSelectTool("pdf.merge")}>
          choose merge
        </button>
        <button
          type="button"
          onClick={() => props.onRunRecipe?.([{ toolId: "pdf.rotate", params: { angle: 90 } }])}
        >
          run palette recipe
        </button>
      </div>
    ),
  };
});
vi.mock("@/components/app/workspace-host", () => {
  return {
    default: (props: {
      requestedTool: { id: string; nonce: number } | null;
      onSaveRecipe?: (steps: Array<{ toolId: string; params: Record<string, unknown> }>) => void;
    }) => (
      <div data-testid="workspace-stub">
        {props.requestedTool ? `${props.requestedTool.id}#${props.requestedTool.nonce}` : "none"}
        <button
          type="button"
          onClick={() =>
            props.onSaveRecipe?.([
              { toolId: "pdf.rotate", params: { angle: 90 } },
              { toolId: "pdf.compress", params: {} },
            ])
          }
        >
          save recipe
        </button>
      </div>
    ),
  };
});
vi.mock("@/components/app/recipes-host", () => {
  return {
    default: (props: {
      draft: { steps: unknown[]; nonce: number } | null;
      fragment: string | null;
      onResolved: () => void;
    }) => (
      <div
        data-testid="recipes-stub"
        data-fragment={props.fragment ?? ""}
        data-steps={props.draft ? props.draft.steps.length : 0}
      >
        <button type="button" onClick={props.onResolved}>
          resolve link
        </button>
      </div>
    ),
  };
});

import { AppShell } from "@/components/app/app-shell";

function renderShell(search = "", hash = "") {
  mocks.search = search;
  window.history.replaceState(null, "", `/en/app${search}${hash}`);
  const messages = scopeMessages(loadMessages("en"), ["common", "web.app"]);
  return render(
    <NextIntlClientProvider locale="en" messages={messages} timeZone="UTC">
      <AppShell />
    </NextIntlClientProvider>,
  );
}

const panel = (testId: string) => screen.getByTestId(testId).closest("[role=tabpanel]");

afterEach(cleanup);

describe("AppShell", () => {
  it("lands on the converter and loads nothing else until it is needed", () => {
    renderShell();
    expect(screen.getByTestId("converter-stub")).toBeTruthy();
    expect(screen.queryByTestId("tools-stub")).toBeNull();
    expect(screen.queryByTestId("workspace-stub")).toBeNull();
    expect(screen.getByRole("tab", { name: "Convert" }).getAttribute("aria-selected")).toBe("true");
  });

  it("loads the hub on its tab and keeps the converter mounted", async () => {
    renderShell();
    fireEvent.click(screen.getByRole("tab", { name: "Tools" }));
    expect(await screen.findByTestId("tools-stub")).toBeTruthy();
    expect(new URL(window.location.href).searchParams.get("view")).toBe("tools");
    expect(screen.getByTestId("converter-stub")).toBeTruthy();
    expect(panel("tools-stub")?.hasAttribute("hidden")).toBe(false);
  });

  it("opens a chosen tool in the workspace and links it in the address bar", async () => {
    renderShell("?view=tools");
    fireEvent.click(await screen.findByRole("button", { name: "choose rotate" }));
    expect(await screen.findByText("pdf.rotate#1")).toBeTruthy();
    expect(screen.getByRole("tab", { name: "PDF workspace" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    const url = new URL(window.location.href);
    expect(url.searchParams.get("tool")).toBe("pdf.rotate");
    expect(url.searchParams.get("view")).toBe("workspace");

    // Choosing the same tool again is a new request, so a changed picker is reset.
    fireEvent.click(screen.getByRole("tab", { name: "Tools" }));
    fireEvent.click(await screen.findByRole("button", { name: "choose rotate" }));
    expect(await screen.findByText("pdf.rotate#2")).toBeTruthy();
  });

  it("opens straight into the workspace for a linked tool", async () => {
    renderShell("?tool=pdf.merge");
    expect(await screen.findByText("pdf.merge#0")).toBeTruthy();
    expect(screen.getByRole("tab", { name: "PDF workspace" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    expect(screen.queryByTestId("tools-stub")).toBeNull();
  });

  it("opens the palette with Ctrl+K before the hub has loaded", async () => {
    renderShell();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true }));
    });
    const hub = await screen.findByTestId("tools-stub");
    expect(hub.getAttribute("data-palette")).toBe("true");
  });

  it("ignores other keys and leaves the hub to its own shortcut once it exists", async () => {
    renderShell();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k" }));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "j", ctrlKey: true }));
    });
    expect(screen.queryByTestId("tools-stub")).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Tools" }));
    await screen.findByTestId("tools-stub");
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }));
    });
    expect(screen.getByTestId("tools-stub").getAttribute("data-palette")).toBe("false");
  });

  it("opens the recipe builder on its tab and keeps it in the address bar", async () => {
    renderShell();
    expect(screen.queryByTestId("recipes-stub")).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Recipes" }));
    expect(await screen.findByTestId("recipes-stub")).toBeTruthy();
    expect(new URL(window.location.href).searchParams.get("view")).toBe("recipes");
    expect(screen.getByTestId("recipes-stub").getAttribute("data-steps")).toBe("0");
  });

  it("hands a recipe from a #recipe= link to the confirmation gate and clears the hash after", async () => {
    renderShell("", "#recipe=eyJ2IjoxfQ");
    const stub = await screen.findByTestId("recipes-stub");
    expect(stub.getAttribute("data-fragment")).toBe("#recipe=eyJ2IjoxfQ");
    expect(screen.getByRole("tab", { name: "Recipes" }).getAttribute("aria-selected")).toBe("true");
    expect(window.location.hash).toBe("#recipe=eyJ2IjoxfQ");
    fireEvent.click(screen.getByRole("button", { name: "resolve link" }));
    await waitFor(() => expect(window.location.hash).toBe(""));
    expect(screen.getByTestId("recipes-stub").getAttribute("data-fragment")).toBe("");
  });

  it("opens the builder with the steps of the workspace's Save as recipe action", async () => {
    renderShell("?view=workspace");
    fireEvent.click(await screen.findByRole("button", { name: "save recipe" }));
    const stub = await screen.findByTestId("recipes-stub");
    expect(stub.getAttribute("data-steps")).toBe("2");
    expect(new URL(window.location.href).searchParams.get("view")).toBe("recipes");
  });

  it("opens the builder with the steps of a command palette recipe", async () => {
    renderShell("?view=tools");
    fireEvent.click(await screen.findByRole("button", { name: "run palette recipe" }));
    expect((await screen.findByTestId("recipes-stub")).getAttribute("data-steps")).toBe("1");
  });
});
