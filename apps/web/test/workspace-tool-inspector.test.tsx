// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ParamsForm, validateParams } from "@/components/tools/params-form";
import { HandoffPanel } from "@/components/workspace/handoff-panel";
import { ToolInspector } from "@/components/workspace/tool-inspector";
import { renderWithToasts, tool, workspaceFile } from "./workspace-fixtures";

afterEach(cleanup);
const files = [workspaceFile("a.pdf"), workspaceFile("b.pdf")];

describe("<ToolInspector />", () => {
  it("shows the tool, its runtime and tier, and runs a browser tool on this device", () => {
    const onRun = vi.fn();
    renderWithToasts(
      <ToolInspector
        tool={tool("pdf.rotate")}
        files={files}
        pageCount={4}
        onRun={onRun}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText("Rotate")).toBeTruthy();
    expect(document.querySelector('[data-slot="runtime-badge"]')?.textContent).toBe("Browser");
    expect(document.querySelector('[data-slot="tier-badge"]')?.textContent).toBe("Free");
    const run = screen.getByRole("button", {
      name: "Run tool on this device",
    }) as HTMLButtonElement;
    // The angle is required and has no default.
    expect(run.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "90" } });
    expect(run.disabled).toBe(false);
    fireEvent.click(run);
    expect(onRun).toHaveBeenCalledWith({ angle: 90, pages: "all" });
    expect(screen.queryByText("Run on your computer")).toBeNull();
  });

  it("needs a file before it can run", () => {
    renderWithToasts(
      <ToolInspector tool={tool("pdf.rotate")} files={[]} onRun={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(
      (screen.getByRole("button", { name: "Run tool on this device" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(screen.getByText("Add a file first.")).toBeTruthy();
  });

  it("shows progress and a cancel button while a run is in flight", () => {
    const onCancel = vi.fn();
    renderWithToasts(
      <ToolInspector
        tool={tool("pdf.rotate")}
        files={files}
        busy
        progress="Rotating page 2"
        onRun={vi.fn()}
        onCancel={onCancel}
      />,
    );
    expect(screen.getByText("Rotating page 2")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
  });

  it("lets a browser tool be run on the computer instead, with the exact command", async () => {
    const copyText = vi.fn().mockResolvedValue(undefined);
    renderWithToasts(
      <ToolInspector
        tool={tool("pdf.rotate")}
        files={files}
        locale="vi"
        copyText={copyText}
        onRun={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "180" } });
    fireEvent.click(screen.getByRole("button", { name: "Run on your computer instead" }));
    expect(document.querySelector('[data-slot="runtime-badge"]')?.textContent).toBe("Desktop");
    expect(screen.getByText("Run on your computer")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Run tool on this device" })).toBeNull();
    const shown = document.querySelector("code")?.textContent ?? "";
    expect(shown).toContain("ashift tool 'pdf.rotate' 'a.pdf' 'b.pdf' --params '");
    expect(shown).toContain('"angle":180');
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(copyText).toHaveBeenCalledTimes(1));
    expect(copyText.mock.calls[0]?.[0]).toContain("ashift tool 'pdf.rotate' 'a.pdf' 'b.pdf'");
    expect((await screen.findByText("Command copied")).textContent).toBe("Command copied");
    expect(screen.getByRole("link", { name: "Get AriadShift" }).getAttribute("href")).toBe(
      "/vi/download",
    );
    expect(screen.getByRole("link", { name: "Open in the desktop app" }).getAttribute("href")).toBe(
      "ariadshift://tool/pdf.rotate",
    );
    fireEvent.click(screen.getByRole("button", { name: "Back to this device" }));
    expect(screen.getByRole("button", { name: "Run tool on this device" })).toBeTruthy();
  });

  it("is honest for a desktop-only tool: the command, never a run button", () => {
    renderWithToasts(
      <ToolInspector tool={tool("pdf.encrypt")} files={files} onRun={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(document.querySelector('[data-slot="runtime-badge"]')?.textContent).toBe("Desktop");
    expect(screen.getByText("Run on your computer")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: /Run tool on this device|Run in Cloud/ }),
    ).toBeNull();
  });

  it("offers Run in Cloud only with a cloud runtime, a configured API and a handler", () => {
    const cloudOnly = {
      ...tool("pdf.encrypt"),
      runtimes: ["cloud", "local"] as ReturnType<typeof tool>["runtimes"],
    };
    const onRunCloud = vi.fn();
    renderWithToasts(
      <ToolInspector
        tool={cloudOnly}
        files={files}
        cloudApiUrl="https://api.test"
        onRunCloud={onRunCloud}
        onRun={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(document.querySelector('[data-slot="runtime-badge"]')?.textContent).toBe("Cloud");
    expect(screen.getByText(/uploaded for it/)).toBeTruthy();
    // pdf.encrypt has a required password; satisfy the form before running.
    for (const input of document.querySelectorAll("input[type=password]"))
      fireEvent.change(input, { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Run in Cloud" }));
    expect(onRunCloud).toHaveBeenCalledTimes(1);
  });

  it("never shows Run in Cloud without a configured API URL or without a handler", () => {
    const cloudOnly = {
      ...tool("pdf.encrypt"),
      runtimes: ["cloud", "local"] as ReturnType<typeof tool>["runtimes"],
    };
    const first = renderWithToasts(
      <ToolInspector
        tool={cloudOnly}
        files={files}
        onRunCloud={vi.fn()}
        onRun={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Run in Cloud" })).toBeNull();
    expect(screen.getByText("Run on your computer")).toBeTruthy();
    first.unmount();
    renderWithToasts(
      <ToolInspector
        tool={cloudOnly}
        files={files}
        cloudApiUrl="https://api.test"
        onRun={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Run in Cloud" })).toBeNull();
    expect(screen.getByText("Run on your computer")).toBeTruthy();
  });

  it("prompts for a tool when none is chosen", () => {
    renderWithToasts(
      <ToolInspector tool={null} files={files} onRun={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(screen.getByText("Choose a tool to see its options.")).toBeTruthy();
  });
});

describe("<HandoffPanel />", () => {
  const encrypt = tool("pdf.encrypt");
  const secretKey = encrypt.secret_params[0] ?? "user_password";

  it("masks secret values in the command until they are revealed", () => {
    renderWithToasts(
      <HandoffPanel tool={encrypt} params={{ [secretKey]: "hunter2" }} inputNames={["a.pdf"]} />,
    );
    expect(document.querySelector("code")?.textContent).not.toContain("hunter2");
    expect(document.querySelector("code")?.textContent).toContain("***");
    fireEvent.click(screen.getByRole("button", { name: "Show secret values" }));
    expect(document.querySelector("code")?.textContent).toContain("hunter2");
    fireEvent.click(screen.getByRole("button", { name: "Hide secret values" }));
    expect(document.querySelector("code")?.textContent).not.toContain("hunter2");
  });

  it("keeps the deep link route-only and quotes dangerous file names", () => {
    renderWithToasts(
      <HandoffPanel
        tool={encrypt}
        params={{ [secretKey]: "hunter2" }}
        inputNames={["it's -x.pdf"]}
      />,
    );
    expect(screen.getByRole("link", { name: "Open in the desktop app" }).getAttribute("href")).toBe(
      "ariadshift://tool/pdf.encrypt",
    );
    expect(document.querySelector("code")?.textContent).toContain(`'it'"'"'s -x.pdf'`);
  });

  it("tells the user when the clipboard is unavailable", async () => {
    renderWithToasts(
      <HandoffPanel
        tool={encrypt}
        params={{}}
        inputNames={["a.pdf"]}
        copyText={vi.fn().mockRejectedValue(new Error("denied"))}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    expect(await screen.findByText("Could not copy. Select the command and copy it.")).toBeTruthy();
  });
});

describe("params form pattern safety", () => {
  const schema = (pattern: string) => ({
    type: "object",
    properties: { code: { type: "string", pattern } },
  });

  it("does not throw on a schema pattern the browser cannot compile", () => {
    expect(() => validateParams(schema("([unclosed"), { code: "abc" })).not.toThrow();
    expect(validateParams(schema("([unclosed"), { code: "abc" }).isValid).toBe(true);
  });

  it("still enforces a valid pattern", () => {
    expect(validateParams(schema("^[A-Z]{3}$"), { code: "abc" }).errors.code).toBe(
      "Enter a value in the required format.",
    );
    expect(validateParams(schema("^[A-Z]{3}$"), { code: "ABC" }).isValid).toBe(true);
  });

  it("renders a form for a malformed pattern instead of crashing", () => {
    const onChange = vi.fn();
    render(
      <ParamsForm
        tool={{ id: "test.pattern", params_schema: schema("([unclosed"), secret_params: [] }}
        initialParams={{ code: "x" }}
        onChange={onChange}
      />,
    );
    expect(screen.getByLabelText("code")).toBeTruthy();
    expect(onChange).toHaveBeenLastCalledWith({ code: "x" }, { isValid: true, errors: {} });
  });
});
