// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { type BatchItem, BatchQueuePanel } from "@/components/batch";
import { RecipeBuilder, RecipeConfirmDialog } from "@/components/recipes";
import {
  type AriadRecipe,
  decodeRecipeFragment,
  encodeRecipeFragment,
  hasPromptSecrets,
  importRecipeJson,
  MAX_RECIPE_BYTES,
  MAX_RECIPE_STEPS,
  parseRecipe,
  previewOutputName,
  RecipeFormatError,
  serializeRecipe,
  validateNamingTemplate,
  validateStepCondition,
} from "@/lib/recipe";
import { tool } from "./workspace-fixtures";

afterEach(cleanup);

const rotate = { tool: "pdf.rotate", params: { angle: 90, pages: "all" } };
const base: AriadRecipe = {
  version: "ariad-recipe/1",
  steps: [
    { tool: "pdf.merge", params: { outline: "per-file" } },
    rotate,
    { convert: { to: "docx", profile: "editable" } },
  ],
};
const withSecret: AriadRecipe = {
  version: "ariad-recipe/1",
  steps: [
    { tool: "pdf.encrypt", params: { owner_password: "hunter2", user_password: "open-sesame" } },
    { tool: "pdf.decrypt", params: { password: "hunter2" }, if: "is_encrypted" },
  ],
  naming: "{stem}-secured",
};

function codeOf(run: () => unknown): string | undefined {
  try {
    run();
  } catch (error) {
    return error instanceof RecipeFormatError ? error.code : "other";
  }
  return undefined;
}

describe("ariad-recipe/1 serialisation", () => {
  it("emits the Rust field order with sorted params and no cleartext secret", () => {
    const json = serializeRecipe(withSecret, { tools: [tool("pdf.encrypt"), tool("pdf.decrypt")] });
    expect(json).not.toContain("hunter2");
    expect(json).not.toContain("open-sesame");
    const parsed = JSON.parse(json);
    expect(Object.keys(parsed)).toEqual(["version", "steps", "naming"]);
    expect(parsed.steps[0]).toEqual({
      tool: "pdf.encrypt",
      params: { owner_password: { $secret: "prompt" }, user_password: { $secret: "prompt" } },
    });
    expect(Object.keys(parsed.steps[1])).toEqual(["tool", "params", "if"]);
    expect(parsed.steps[1].params.password).toEqual({ $secret: "prompt" });
  });

  it("always writes params, and writes convert steps like the Rust enum", () => {
    const parsed = JSON.parse(
      serializeRecipe({
        version: "ariad-recipe/1",
        steps: [{ tool: "pdf.merge" }, { convert: { to: "docx" }, on_error: "skip" }],
      }),
    );
    expect(parsed.steps).toEqual([
      { tool: "pdf.merge", params: {} },
      { convert: { to: "docx" }, on_error: "skip" },
    ]);
  });

  it("redacts secrets nested in params and fields declared by the registry", () => {
    const json = serializeRecipe(
      {
        version: "ariad-recipe/1",
        steps: [{ tool: "ai.ask", params: { question: "q", nested: { api_key: "sk-live" } } }],
      },
      { tools: [tool("ai.ask")] },
    );
    expect(json).not.toContain("sk-live");
    expect(JSON.parse(json).steps[0].params.nested.api_key).toEqual({ $secret: "prompt" });
  });

  it("lists the prompt secrets by step index and parameter name", () => {
    const redacted = parseRecipe(JSON.parse(serializeRecipe(withSecret)));
    expect(hasPromptSecrets(redacted)).toEqual([
      "0:owner_password",
      "0:user_password",
      "1:password",
    ]);
    expect(hasPromptSecrets(base)).toEqual([]);
  });
});

describe("recipe URL fragments", () => {
  it("round-trips a recipe through #recipe=<base64url> and tags it untrusted", () => {
    const fragment = encodeRecipeFragment(base);
    expect(fragment).toMatch(/^#recipe=[A-Za-z0-9_-]+$/);
    const decoded = decodeRecipeFragment(fragment);
    expect(decoded.origin).toBe("untrusted");
    expect(decoded.recipe).toEqual(base);
    // The hash, the key and the bare payload are all accepted.
    expect(decodeRecipeFragment(fragment.slice(1)).recipe).toEqual(base);
    expect(decodeRecipeFragment(fragment.slice("#recipe=".length)).recipe).toEqual(base);
  });

  it("keeps non-ASCII text intact", () => {
    const recipe: AriadRecipe = {
      version: "ariad-recipe/1",
      steps: [{ tool: "pdf.watermark", params: { text: "Bản nháp — Ünïcode ✓" } }],
    };
    expect(decodeRecipeFragment(encodeRecipeFragment(recipe)).recipe).toEqual(recipe);
  });

  it("never puts a secret in the fragment", () => {
    const fragment = encodeRecipeFragment(withSecret, { tools: [tool("pdf.encrypt")] });
    const text = atob(fragment.slice("#recipe=".length).replaceAll("-", "+").replaceAll("_", "/"));
    expect(text).not.toContain("hunter2");
    expect(text).toContain('{"$secret":"prompt"}');
    const decoded = decodeRecipeFragment(fragment);
    expect(hasPromptSecrets(decoded.recipe)).toContain("0:owner_password");
  });

  it("rejects a wrong version, an empty or oversized recipe and bad encodings", () => {
    const wrap = (value: unknown) =>
      `#recipe=${btoa(JSON.stringify(value)).replaceAll("+", "-").replaceAll("/", "_")}`;
    expect(
      codeOf(() => decodeRecipeFragment(wrap({ version: "ariad-recipe/2", steps: [rotate] }))),
    ).toBe("invalid_version");
    expect(codeOf(() => decodeRecipeFragment(wrap({ version: "ariad-recipe/1", steps: [] })))).toBe(
      "empty_steps",
    );
    const many = Array.from({ length: MAX_RECIPE_STEPS + 1 }, () => ({ tool: "pdf.merge" }));
    expect(
      codeOf(() => decodeRecipeFragment(wrap({ version: "ariad-recipe/1", steps: many }))),
    ).toBe("too_many_steps");
    expect(codeOf(() => decodeRecipeFragment(`#recipe=${"A".repeat(MAX_RECIPE_BYTES * 2)}`))).toBe(
      "too_large",
    );
    const bigParams = {
      version: "ariad-recipe/1",
      steps: [{ tool: "pdf.merge", params: { x: "y".repeat(MAX_RECIPE_BYTES) } }],
    };
    expect(codeOf(() => importRecipeJson(JSON.stringify(bigParams)))).toBe("too_large");
    expect(codeOf(() => encodeRecipeFragment(bigParams as unknown as AriadRecipe))).toBe(
      "too_large",
    );
    expect(codeOf(() => decodeRecipeFragment("#recipe=!!!"))).toBe("invalid_encoding");
    expect(codeOf(() => decodeRecipeFragment(`#recipe=${btoa("not json")}`))).toBe("invalid_json");
  });

  it("applies the untrusted-recipe rules of the host", () => {
    const wrap = (extra: object) => ({ version: "ariad-recipe/1", steps: [rotate], ...extra });
    expect(codeOf(() => parseRecipe(wrap({ watch: "incoming" })))).toBe("forbidden");
    expect(codeOf(() => parseRecipe(wrap({ steps: [{ tool: "web.to_pdf", params: {} }] })))).toBe(
      "forbidden",
    );
    expect(codeOf(() => parseRecipe(wrap({ extra: 1 })))).toBe("invalid_json");
    expect(codeOf(() => parseRecipe(wrap({ steps: [{ tool: "pdf.merge", surprise: 1 }] })))).toBe(
      "invalid_step",
    );
    expect(
      codeOf(() => parseRecipe(wrap({ steps: [{ tool: "pdf.merge", if: "colour == 1" }] }))),
    ).toBe("invalid_condition");
    expect(codeOf(() => parseRecipe(wrap({ naming: "../x" })))).toBe("invalid_naming");
    // A trusted recipe may keep `watch`; it is the web surface's own.
    expect(codeOf(() => parseRecipe(wrap({ watch: "incoming" }), "trusted"))).toBeUndefined();
  });

  it("refuses a literal secret or a handle where only a prompt reference is allowed", () => {
    const step = (password: unknown) => ({
      version: "ariad-recipe/1",
      steps: [{ tool: "pdf.decrypt", params: { password } }],
    });
    expect(codeOf(() => parseRecipe(step("hunter2")))).toBe("literal_secret");
    expect(codeOf(() => parseRecipe(step({ $secret: "sec_01J" })))).toBe("literal_secret");
    expect(codeOf(() => parseRecipe(step({ $secret: "prompt" })))).toBeUndefined();
  });
});

describe("recipe grammars", () => {
  it("validates naming templates like the host", () => {
    for (const ok of ["{stem}", "{stem}-{n}", "Report {date} {title}.v2"])
      expect(validateNamingTemplate(ok)).toBeNull();
    for (const bad of ["", ".hidden", "a..b", "{stem", "stem}", "{ext}", "a/b", "é"])
      expect(validateNamingTemplate(bad)).not.toBeNull();
  });

  it("validates step conditions like the host", () => {
    for (const ok of [
      "page_count > 1",
      "size_bytes <= 1048576",
      "needs_ocr == true",
      "!is_encrypted",
      "has_signatures",
      "format == 'pdf'",
    ])
      expect(validateStepCondition(ok), ok).toBeNull();
    for (const bad of ["page_count > one", "needs_ocr > true", "format == pdf", "colour == 1", ""])
      expect(validateStepCondition(bad), bad).not.toBeNull();
  });

  it("previews the output name", () => {
    expect(previewOutputName("{stem}-{n}", { stem: "report" })).toBe("report-001.pdf");
    expect(
      previewOutputName("{date}_{title}", { stem: "r", title: "Q3", date: "2026-11-11" }),
    ).toBe("2026-11-11_Q3.pdf");
    expect(previewOutputName(undefined, { stem: "report", ext: "docx" })).toBe("report.docx");
  });
});

/* ---------------------------------------------------------------- batch queue */

const items: BatchItem[] = [
  { id: "a", name: "a.pdf", status: "queued", inputSize: 2048 },
  { id: "b", name: "b.pdf", status: "running", inputSize: 4096 },
  { id: "c", name: "c.pdf", status: "done", inputSize: 10240, outputSize: 2048 },
  { id: "d", name: "d.pdf", status: "failed", inputSize: 1024, error: "Encrypted file" },
  { id: "e", name: "e.pdf", status: "cancelled", inputSize: 512 },
];

function handlers() {
  return {
    onRunBatch: vi.fn(),
    onCancelBatch: vi.fn(),
    onRetryFailed: vi.fn(),
    onDownloadZip: vi.fn(),
    onClearCompleted: vi.fn(),
    onCancelItem: vi.fn(),
    onRemoveItem: vi.fn(),
    onDownloadItem: vi.fn(),
  };
}

describe("<BatchQueuePanel />", () => {
  it("shows an empty state", () => {
    render(<BatchQueuePanel items={[]} {...handlers()} />);
    expect(screen.getByText(/Add files to run this tool/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Run batch" })).toBeNull();
  });

  it("shows progress, total sizes and every item state", () => {
    render(<BatchQueuePanel items={items} {...handlers()} />);
    expect(document.querySelector('[data-slot="batch-overall"]')?.textContent).toBe("1 of 5 done");
    expect(document.querySelector('[data-slot="batch-sizes"]')?.textContent).toBe(
      "18 KB in · 2.0 KB out",
    );
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("20");
    for (const item of items) {
      const row = screen.getByText(item.name).closest("li");
      expect(row?.getAttribute("data-status")).toBe(item.status);
    }
    const failed = screen.getByText("d.pdf").closest("li") as HTMLElement;
    expect(within(failed).getByText("Failed")).toBeTruthy();
    expect(within(failed).getByText("Encrypted file")).toBeTruthy();
    const done = screen.getByText("c.pdf").closest("li") as HTMLElement;
    expect(within(done).getByText("10 KB in · 2.0 KB out")).toBeTruthy();
  });

  it("runs, cancels, retries, zips and clears", () => {
    const on = handlers();
    render(
      <BatchQueuePanel items={[items[0], items[2], items[3], items[4]] as BatchItem[]} {...on} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Run batch" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel batch" }));
    fireEvent.click(screen.getByRole("button", { name: "Retry failed" }));
    fireEvent.click(screen.getByRole("button", { name: "Download all (.zip)" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear completed" }));
    expect(on.onRunBatch).toHaveBeenCalledOnce();
    expect(on.onCancelBatch).toHaveBeenCalledOnce();
    expect(on.onRetryFailed).toHaveBeenCalledOnce();
    expect(on.onDownloadZip).toHaveBeenCalledOnce();
    expect(on.onClearCompleted).toHaveBeenCalledOnce();
  });

  it("offers per-item cancel, remove and download by state", () => {
    const on = handlers();
    render(<BatchQueuePanel items={items} {...on} />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel a.pdf" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel b.pdf" }));
    fireEvent.click(screen.getByRole("button", { name: "Download c.pdf" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove d.pdf" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove e.pdf" }));
    expect(on.onCancelItem.mock.calls).toEqual([["a"], ["b"]]);
    expect(on.onDownloadItem).toHaveBeenCalledWith("c");
    expect(on.onRemoveItem.mock.calls).toEqual([["d"], ["e"]]);
    expect(screen.queryByRole("button", { name: "Download a.pdf" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Remove b.pdf" })).toBeNull();
  });

  it("disables actions that do not apply", () => {
    render(<BatchQueuePanel items={[items[1] as BatchItem]} {...handlers()} />);
    const disabled = (name: string) =>
      (screen.getByRole("button", { name }) as HTMLButtonElement).disabled;
    expect(disabled("Run batch")).toBe(true);
    expect(disabled("Cancel batch")).toBe(false);
    expect(disabled("Retry failed")).toBe(true);
    expect(disabled("Download all (.zip)")).toBe(true);
    expect(disabled("Clear completed")).toBe(true);
  });

  it("warns about files over the per-file browser budget", () => {
    render(<BatchQueuePanel items={items} fileBudgetBytes={3072} {...handlers()} />);
    const guard = document.querySelector('[data-slot="batch-memory-guard"]') as HTMLElement;
    expect(guard.textContent).toContain("b.pdf, c.pdf");
    expect(guard.textContent).toContain("3.0 KB");
    cleanup();
    render(<BatchQueuePanel items={items} fileBudgetBytes={1 << 30} {...handlers()} />);
    expect(document.querySelector('[data-slot="batch-memory-guard"]')).toBeNull();
  });
});

/* ---------------------------------------------------------------- recipe builder */

const tools = ["pdf.merge", "pdf.rotate", "pdf.decrypt", "pdf.encrypt"].map(tool);

function lastRecipe(onChange: ReturnType<typeof vi.fn>): AriadRecipe {
  return onChange.mock.calls.at(-1)?.[0] as AriadRecipe;
}

describe("<RecipeBuilder />", () => {
  it("adds, reorders and removes steps and connects them with the thread", async () => {
    const onChange = vi.fn();
    render(<RecipeBuilder tools={tools} onChange={onChange} />);
    expect(screen.getByText("Add a tool to start a recipe.")).toBeTruthy();
    const picker = screen.getByLabelText("Tool to add");
    fireEvent.change(picker, { target: { value: "pdf.merge" } });
    fireEvent.click(screen.getByRole("button", { name: "Add step" }));
    fireEvent.change(picker, { target: { value: "pdf.rotate" } });
    fireEvent.click(screen.getByRole("button", { name: "Add step" }));
    fireEvent.change(picker, { target: { value: "__convert__" } });
    fireEvent.click(screen.getByRole("button", { name: "Add step" }));
    await waitFor(() => expect(lastRecipe(onChange).steps).toHaveLength(3));
    expect(lastRecipe(onChange).steps.map((s) => ("tool" in s ? s.tool : "convert"))).toEqual([
      "pdf.merge",
      "pdf.rotate",
      "convert",
    ]);
    expect(document.querySelectorAll('[data-slot="recipe-thread"]')).toHaveLength(2);
    expect(screen.getByText("3 steps")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Move step 1 down" }));
    await waitFor(() =>
      expect(lastRecipe(onChange).steps.map((s) => ("tool" in s ? s.tool : "convert"))).toEqual([
        "pdf.rotate",
        "pdf.merge",
        "convert",
      ]),
    );
    expect(
      (screen.getByRole("button", { name: "Move step 1 up" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "Move step 3 down" }) as HTMLButtonElement).disabled,
    ).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Remove step 2" }));
    await waitFor(() =>
      expect(lastRecipe(onChange).steps.map((s) => ("tool" in s ? s.tool : "convert"))).toEqual([
        "pdf.rotate",
        "convert",
      ]),
    );
  });

  it("edits parameters, conditions, convert targets and the naming template", async () => {
    const onChange = vi.fn();
    render(
      <RecipeBuilder
        tools={tools}
        initialRecipe={{
          version: "ariad-recipe/1",
          steps: [{ tool: "pdf.rotate", params: {} }, { convert: { to: "docx" } }],
        }}
        onChange={onChange}
      />,
    );
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "180" } });
    await waitFor(() =>
      expect(lastRecipe(onChange).steps[0]).toEqual({
        tool: "pdf.rotate",
        params: { angle: 180, pages: "all" },
      }),
    );

    const conditions = screen.getAllByLabelText("Condition (optional)");
    fireEvent.change(conditions[0] as HTMLElement, { target: { value: "page_count > 1" } });
    await waitFor(() => expect(lastRecipe(onChange).steps[0]?.if).toBe("page_count > 1"));
    fireEvent.change(conditions[1] as HTMLElement, { target: { value: "colour == 1" } });
    expect(screen.getByText("Unknown condition field colour.")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Share link" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    fireEvent.change(conditions[1] as HTMLElement, { target: { value: "" } });

    fireEvent.change(screen.getByLabelText("Convert to"), { target: { value: "epub" } });
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "faithful" } });
    await waitFor(() =>
      expect(lastRecipe(onChange).steps[1]).toEqual({
        convert: { to: "epub", profile: "faithful" },
      }),
    );

    const naming = screen.getByLabelText("Output name template");
    fireEvent.change(naming, { target: { value: "{stem}-{n}" } });
    expect(screen.getByText(/Preview: report-001\.pdf/)).toBeTruthy();
    await waitFor(() => expect(lastRecipe(onChange).naming).toBe("{stem}-{n}"));
    fireEvent.change(naming, { target: { value: "{ext}" } });
    expect(screen.getByText(/Unknown token \{ext\}/)).toBeTruthy();
  });

  it("exports JSON with secrets replaced by a prompt reference", async () => {
    const onExportJson = vi.fn();
    render(
      <RecipeBuilder
        tools={tools}
        initialRecipe={{ version: "ariad-recipe/1", steps: [{ tool: "pdf.decrypt", params: {} }] }}
        onExportJson={onExportJson}
      />,
    );
    fireEvent.change(screen.getByLabelText("password"), { target: { value: "hunter2" } });
    await waitFor(() =>
      expect(screen.getByText('Secrets replaced with {"$secret":"prompt"}')).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Export JSON" }));
    const [json, filename] = onExportJson.mock.calls[0] as [string, string];
    expect(filename).toBe("recipe.json");
    expect(json).not.toContain("hunter2");
    expect(JSON.parse(json)).toEqual({
      version: "ariad-recipe/1",
      steps: [{ tool: "pdf.decrypt", params: { password: { $secret: "prompt" } } }],
    });
    // The exported file is a valid ariad-recipe/1 for the host.
    expect(codeOf(() => parseRecipe(JSON.parse(json)))).toBeUndefined();
  });

  it("copies a share link whose fragment carries no secret", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(
      <RecipeBuilder
        tools={tools}
        shareBaseUrl="https://example.test/app"
        initialRecipe={{ version: "ariad-recipe/1", steps: [{ tool: "pdf.decrypt", params: {} }] }}
      />,
    );
    fireEvent.change(screen.getByLabelText("password"), { target: { value: "hunter2" } });
    await waitFor(() =>
      expect(
        (screen.getByRole("button", { name: "Share link" }) as HTMLButtonElement).disabled,
      ).toBe(false),
    );
    fireEvent.click(screen.getByRole("button", { name: "Share link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
    const link = writeText.mock.calls[0]?.[0] as string;
    expect(link.startsWith("https://example.test/app#recipe=")).toBe(true);
    expect(link).not.toContain("hunter2");
    const shared = decodeRecipeFragment(link.slice(link.indexOf("#")));
    expect(shared.recipe.steps[0]).toEqual({
      tool: "pdf.decrypt",
      params: { password: { $secret: "prompt" } },
    });
    const status = await screen.findByRole("status");
    expect(status.textContent).toContain('Secrets replaced with {"$secret":"prompt"}');
  });

  it("previews the CLI command and copies it", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<RecipeBuilder tools={tools} />);
    const command = "ashift recipe run recipe.json input.pdf -o out/";
    expect(document.querySelector('[data-slot="recipe-command"]')?.textContent).toBe(command);
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(command));
  });

  it("holds a pasted recipe behind the confirmation gate and loads it only on confirm", async () => {
    const onRun = vi.fn();
    const onChange = vi.fn();
    render(<RecipeBuilder tools={tools} onRun={onRun} onChange={onChange} />);
    const pasted = JSON.stringify({
      version: "ariad-recipe/1",
      steps: [
        { tool: "pdf.merge", params: { outline: "per-file" } },
        { tool: "pdf.decrypt", params: { password: { $secret: "prompt" } }, if: "is_encrypted" },
      ],
    });
    fireEvent.change(screen.getByLabelText("Paste ariad-recipe/1 JSON"), {
      target: { value: pasted },
    });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    const dialog = await screen.findByRole("dialog");
    expect(dialog.querySelector('[data-origin="untrusted"]')).toBeTruthy();
    expect(within(dialog).getByText("Step 1")).toBeTruthy();
    expect(within(dialog).getByText("Step 2")).toBeTruthy();
    expect(within(dialog).getByText("pdf.decrypt")).toBeTruthy();
    expect(within(dialog).getByText("Runs only if is_encrypted")).toBeTruthy();
    expect(within(dialog).getByText(/password: you will be asked in memory/)).toBeTruthy();
    // Nothing is loaded and nothing can run while the gate is open.
    expect(lastRecipe(onChange)?.steps ?? []).toHaveLength(0);
    expect(onRun).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm & Load" }));
    await waitFor(() => expect(lastRecipe(onChange).steps).toHaveLength(2));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onRun).not.toHaveBeenCalled(); // loading never runs
    expect(lastRecipe(onChange).steps[1]).toEqual({
      tool: "pdf.decrypt",
      params: { password: { $secret: "prompt" } },
      if: "is_encrypted",
    });
    expect((screen.getByRole("button", { name: "Run recipe" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
    fireEvent.click(screen.getByRole("button", { name: "Run recipe" }));
    expect(onRun).toHaveBeenCalledOnce();
  });

  it("drops a cancelled import and reports an invalid one", async () => {
    const onChange = vi.fn();
    render(<RecipeBuilder tools={tools} onChange={onChange} />);
    const paste = screen.getByLabelText("Paste ariad-recipe/1 JSON");
    fireEvent.change(paste, {
      target: { value: JSON.stringify({ version: "ariad-recipe/1", steps: [rotate] }) },
    });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByText("Add a tool to start a recipe.")).toBeTruthy();

    fireEvent.change(paste, {
      target: { value: JSON.stringify({ version: "ariad-recipe/9", steps: [rotate] }) },
    });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Unsupported recipe version");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("opens a recipe from a link behind the same gate", async () => {
    const incoming = decodeRecipeFragment(encodeRecipeFragment(base));
    const onRun = vi.fn();
    render(<RecipeBuilder tools={tools} incoming={incoming} onRun={onRun} />);
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Convert to docx (editable)")).toBeTruthy();
    expect(within(dialog).getByText("pdf.merge")).toBeTruthy();
  });
});

describe("<RecipeConfirmDialog />", () => {
  it("stays closed without a recipe and reports confirm and cancel", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { rerender } = render(
      <RecipeConfirmDialog
        recipe={null}
        origin="untrusted"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    rerender(
      <RecipeConfirmDialog
        recipe={{ ...base, naming: "{stem}-out" }}
        origin="untrusted"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("Output names: {stem}-out")).toBeTruthy();
    expect(within(dialog).getByText(/"outline": "per-file"/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm & Load" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
