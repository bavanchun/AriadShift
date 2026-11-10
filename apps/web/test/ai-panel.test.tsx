// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { AiPanel, type AiPanelProps } from "@/components/ai/ai-panel";
import {
  buildAiCliHandoff,
  buildAskParams,
  buildExtractTablesParams,
  buildRecipeParams,
  buildSummarizeParams,
  buildTranslateParams,
  commonParams,
  isAskOutput,
  isRecipeOutput,
  isSummarizeOutput,
  isTablesOutput,
  isTranslateOutput,
  parseNaturalLanguageRecipeFallback,
  safeArtifactHref,
  scrubRecipeSecrets,
  scrubSecrets,
  tablesToCsv,
} from "@/components/ai/ai-params";
import type { AiOutput, AiRequest } from "@/components/ai/ai-types";
import { CloudConsentDialog } from "@/components/ai/cloud-consent-dialog";

const LOCAL = { provider: "local", consent: false } as const;
const SECRET = "sk-secret-123";
const CLOUD = { provider: "cloud", consent: true, apiKey: SECRET } as const;
const SUMMARY: AiOutput = {
  summary: "Short.",
  bullet_points: ["One"],
  language: "en",
  length: "standard",
};

let fetchSpy: ReturnType<typeof vi.fn>;
beforeEach(() => {
  fetchSpy = vi.fn();
  vi.stubGlobal("fetch", fetchSpy);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function panel(props: AiPanelProps = {}) {
  return render(createElement(AiPanel, props));
}
/** The privacy badge; hidden:true because an open modal hides the rest of the page from queries. */
function badge() {
  return screen.getByRole("status", { hidden: true });
}
function tab(name: RegExp | string) {
  return screen.getByRole("tab", { name });
}
async function openTab(name: RegExp | string) {
  fireEvent.click(tab(name));
  await waitFor(() => expect(tab(name).getAttribute("aria-selected")).toBe("true"));
}
/** Switches to Cloud AI through the consent dialog, with an optional bring-your-own key. */
async function grantConsent(apiKey?: string) {
  fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("checkbox"));
  if (apiKey)
    fireEvent.change(within(dialog).getByLabelText(/API key/), { target: { value: apiKey } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Confirm and continue" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

describe("parameter builders", () => {
  it("emits no consent flag or key for a local request", () => {
    expect(commonParams({ ...LOCAL, apiKey: SECRET })).toEqual({ provider: "local" });
  });

  it("carries explicit consent, the key and the model for a cloud request", () => {
    expect(commonParams({ ...CLOUD, model: " m1 " })).toEqual({
      provider: "cloud",
      cloud_consent: true,
      model: "m1",
      api_key: SECRET,
    });
    expect(commonParams({ provider: "cloud", consent: false })).toEqual({
      provider: "cloud",
      cloud_consent: false,
    });
  });

  it("builds ai.summarize with standard length by default", () => {
    expect(buildSummarizeParams({}, LOCAL)).toEqual({
      params: { length: "standard", provider: "local" },
      errors: {},
      isValid: true,
    });
    expect(buildSummarizeParams({ length: "brief", language: " vi " }, LOCAL).params).toEqual({
      length: "brief",
      language: "vi",
      provider: "local",
    });
  });

  it("rejects an empty or blank ai.ask question", () => {
    expect(buildAskParams({ question: "" }, LOCAL).errors.question).toBeTruthy();
    expect(buildAskParams({ question: "   " }, LOCAL).isValid).toBe(false);
    expect(buildAskParams({ question: " Who signs? " }, LOCAL).params.question).toBe("Who signs?");
  });

  it("keeps max_citations within 1..20", () => {
    for (const bad of [0, 21, -3, 2.5, Number.NaN])
      expect(
        buildAskParams({ question: "q", maxCitations: bad }, LOCAL).errors.max_citations,
      ).toBeTruthy();
    for (const good of [1, 5, 20]) {
      const built = buildAskParams({ question: "q", maxCitations: good }, LOCAL);
      expect(built.isValid).toBe(true);
      expect(built.params.max_citations).toBe(good);
    }
  });

  it("requires a target language for ai.translate", () => {
    expect(buildTranslateParams({ targetLang: " " }, LOCAL).errors.target_lang).toBeTruthy();
    expect(
      buildTranslateParams({ targetLang: "vi", sourceLang: "en", format: "docx" }, LOCAL).params,
    ).toEqual({ target_lang: "vi", source_lang: "en", format: "docx", provider: "local" });
  });

  it("validates a custom page range for ai.extract_tables and drops 'all'", () => {
    const validate = (value: string) => (value === "1-3" ? null : "bad range");
    expect(
      buildExtractTablesParams({ pages: "all", format: "csv" }, LOCAL, validate).params,
    ).toEqual({ format: "csv", provider: "local" });
    expect(buildExtractTablesParams({ pages: "1-3" }, LOCAL, validate).params.pages).toBe("1-3");
    expect(buildExtractTablesParams({ pages: "zz" }, LOCAL, validate).errors.pages).toBe(
      "bad range",
    );
  });

  it("requires a non-empty prompt for ai.recipe_from_text", () => {
    expect(buildRecipeParams({ prompt: "  " }, LOCAL).errors.prompt).toBeTruthy();
    expect(buildRecipeParams({ prompt: "merge" }, LOCAL).isValid).toBe(true);
  });
});

describe("secret handling", () => {
  it("scrubs api_key from parameters", () => {
    expect(scrubSecrets({ provider: "cloud", api_key: SECRET, cloud_consent: true })).toEqual({
      provider: "cloud",
      cloud_consent: true,
    });
  });

  it("never writes the key into a desktop command, and reads it from the environment instead", () => {
    const params = buildAskParams({ question: "What is due?" }, CLOUD).params;
    const { command, readsApiKeyFromStdin } = buildAiCliHandoff("ai.ask", params, "report.pdf");
    expect(command).not.toContain(SECRET);
    expect(readsApiKeyFromStdin).toBe(true);
    expect(command).toContain('"$ASHIFT_AI_API_KEY"');
    expect(command).toContain("--secret 'api_key=-'");
    expect(command).toContain("--param 'question=What is due?'");
    expect(command).toContain("'ai.ask' 'report.pdf'");
  });

  it("builds a plain local command with no secret channel", () => {
    const params = buildSummarizeParams({ length: "brief" }, LOCAL).params;
    const { command, readsApiKeyFromStdin } = buildAiCliHandoff("ai.summarize", params, "a.pdf");
    expect(readsApiKeyFromStdin).toBe(false);
    expect(command).toBe(
      "ashift tool 'ai.summarize' 'a.pdf' --param 'length=brief' --param 'provider=local'",
    );
  });

  it("quotes hostile file names and omits the input for ai.recipe_from_text", () => {
    const hostile = buildAiCliHandoff("ai.ask", { question: "q" }, "-x'; rm -rf ~ #.pdf");
    expect(hostile.command).toContain(`'./-x'"'"'; rm -rf ~ #.pdf'`);
    const recipe = buildAiCliHandoff("ai.recipe_from_text", { prompt: "merge" }, "a.pdf");
    expect(recipe.command).not.toContain("a.pdf");
  });

  it("scrubs a key a runner left inside recipe steps", () => {
    const scrubbed = scrubRecipeSecrets({
      version: "ariad-recipe/1",
      steps: [{ tool: "pdf.merge", params: { api_key: SECRET, keep: 1 } }],
    });
    expect(JSON.stringify(scrubbed)).not.toContain(SECRET);
    expect(scrubbed.steps[0]?.params).toEqual({ keep: 1 });
  });
});

describe("offline recipe parser", () => {
  it("maps 'merge, OCR, compress under 5 MB' to an imported draft needing confirmation", () => {
    const draft = parseNaturalLanguageRecipeFallback("merge, OCR, compress under 5 MB");
    expect(draft).toMatchObject({ origin: "imported", requires_confirmation: true, unmatched: [] });
    expect(draft?.recipe).toEqual({
      version: "ariad-recipe/1",
      steps: [
        { tool: "pdf.merge", params: {} },
        { tool: "pdf.ocr", params: {} },
        { tool: "pdf.compress", params: { target_bytes: 5 * 1024 * 1024 } },
      ],
    });
  });

  it("maps rotate, deskew and a text watermark, keeping the watermark's case", () => {
    const draft = parseNaturalLanguageRecipeFallback("rotate 90, deskew, watermark CONFIDENTIAL");
    expect(draft?.recipe.steps).toEqual([
      { tool: "pdf.rotate", params: { pages: "all", angle: 90 } },
      { tool: "pdf.deskew", params: {} },
      { tool: "pdf.watermark", params: { kind: "text", text: "CONFIDENTIAL" } },
    ]);
  });

  it("handles left rotations, quoted watermarks and an unsized compress", () => {
    const draft = parseNaturalLanguageRecipeFallback(
      'rotate left and then compress then watermark "Do not copy"',
    );
    expect(draft?.recipe.steps).toEqual([
      { tool: "pdf.rotate", params: { pages: "all", angle: -90 } },
      { tool: "pdf.compress", params: { preset: "ebook" } },
      { tool: "pdf.watermark", params: { kind: "text", text: "Do not copy" } },
    ]);
  });

  it("reports clauses it does not understand and returns null when nothing matches", () => {
    expect(parseNaturalLanguageRecipeFallback("merge, make coffee")?.unmatched).toEqual([
      "make coffee",
    ]);
    expect(parseNaturalLanguageRecipeFallback("make coffee")).toBeNull();
    expect(parseNaturalLanguageRecipeFallback("   ")).toBeNull();
  });

  it("produces a draft that serialises without any secret field", () => {
    const draft = parseNaturalLanguageRecipeFallback("merge, OCR");
    expect(JSON.stringify(draft)).not.toMatch(/api_key|sk-/);
  });
});

describe("output helpers", () => {
  it("escapes CSV and neutralises spreadsheet formulas", () => {
    expect(tablesToCsv([["a,b", 'say "hi"']])).toBe('"a,b","say ""hi"""');
    expect(tablesToCsv([["x"], ["y"]])).toBe("x\r\ny");
    expect(tablesToCsv([["=SUM(A1)", "+cmd", "@x", "-5", "-abc"]])).toBe(
      "'=SUM(A1),'+cmd,'@x,-5,'-abc",
    );
  });

  it("only allows safe artifact links", () => {
    expect(safeArtifactHref("blob:https://x/abc")).toBe("blob:https://x/abc");
    expect(safeArtifactHref("https://cdn.example/a.pdf")).toBe("https://cdn.example/a.pdf");
    expect(safeArtifactHref("javascript:alert(1)")).toBeUndefined();
    expect(safeArtifactHref("data:text/html,x")).toBeUndefined();
    expect(safeArtifactHref("//evil.example/x")).toBeUndefined();
  });

  it("rejects malformed tool outputs", () => {
    expect(isAskOutput({ answer: "a", citations: [{ page: 1, quote: "q", score: 0.5 }] })).toBe(
      true,
    );
    expect(isAskOutput({ answer: "a", citations: [{ page: "1", quote: "q", score: 0.5 }] })).toBe(
      false,
    );
    expect(isSummarizeOutput({ summary: "s", bullet_points: [] })).toBe(true);
    expect(isSummarizeOutput({ summary: 1 })).toBe(false);
    expect(isTablesOutput({ tables: [{ page: 2, rows: [["a"]] }] })).toBe(true);
    expect(isTablesOutput({ tables: [{ page: "2" }] })).toBe(false);
    expect(isTranslateOutput({ artifact: { name: "a.pdf", href: "blob:x" } })).toBe(true);
    expect(isTranslateOutput({})).toBe(false);
    expect(
      isRecipeOutput({
        recipe: { version: "ariad-recipe/1", steps: [] },
        origin: "imported",
        requires_confirmation: true,
      }),
    ).toBe(true);
    expect(
      isRecipeOutput({ recipe: { version: "ariad-recipe/1", steps: [] }, origin: "native" }),
    ).toBe(false);
  });
});

describe("offline-first default", () => {
  it("starts on this device with the privacy badge and makes no network request", () => {
    panel({ inputName: "a.pdf" });
    const status = badge();
    expect(status.getAttribute("data-provider")).toBe("local");
    expect(status.textContent).toContain("On this device · 0 bytes uploaded");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("exposes an accessible tablist of five tabs with one selected", async () => {
    panel();
    expect(screen.getByRole("tablist", { name: "AI tools" })).toBeTruthy();
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual([
      "Summarise",
      "Ask",
      "Translate",
      "Extract tables",
      "Recipe",
    ]);
    expect(tabs.filter((t) => t.getAttribute("aria-selected") === "true")).toHaveLength(1);
    await openTab("Translate");
    expect(screen.getByLabelText("Translate into")).toBeTruthy();
  });

  it("moves focus between tabs with the arrow keys", async () => {
    panel();
    tab("Summarise").focus();
    fireEvent.keyDown(tab("Summarise"), { key: "ArrowRight" });
    await waitFor(() => expect(document.activeElement).toBe(tab("Ask")));
    fireEvent.keyDown(tab("Ask"), { key: "ArrowLeft" });
    await waitFor(() => expect(document.activeElement).toBe(tab("Summarise")));
  });

  it("hands off to Desktop when no local model is loaded, without running anything", () => {
    const onExecute = vi.fn();
    panel({ inputName: "report.pdf", onExecute });
    expect(screen.getByText("Run on Desktop")).toBeTruthy();
    expect(screen.getByText(/ashift tool 'ai.summarize' 'report.pdf'/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Run" })).toBeNull();
    expect(onExecute).not.toHaveBeenCalled();
  });

  it("copies the desktop command", async () => {
    const copyText = vi.fn().mockResolvedValue(undefined);
    panel({ inputName: "report.pdf", copyText });
    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));
    await waitFor(() => expect(copyText).toHaveBeenCalledTimes(1));
    expect(copyText.mock.calls[0]?.[0]).toContain("ashift tool 'ai.summarize'");
  });
});

describe("cloud consent gate", () => {
  it("opens the consent dialog on switching to Cloud and runs nothing", async () => {
    const onExecute = vi.fn();
    panel({ onExecute, localRuntime: true });
    fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toContain("What leaves your device");
    expect(badge().getAttribute("data-provider")).toBe("local");
    expect(onExecute).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("cannot be confirmed until the consent checkbox is checked", async () => {
    panel({ onExecute: vi.fn() });
    fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", {
      name: "Confirm and continue",
    }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    fireEvent.click(within(dialog).getByRole("checkbox"));
    expect(confirm.disabled).toBe(false);
  });

  it("stays local when the dialog is cancelled or dismissed with Escape", async () => {
    panel({ onExecute: vi.fn() });
    fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(badge().getAttribute("data-provider")).toBe("local");

    fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(badge().getAttribute("data-provider")).toBe("local");
  });

  it("collects the key in a password field that is never autofilled", async () => {
    panel({ onExecute: vi.fn() });
    fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
    const key = within(await screen.findByRole("dialog")).getByLabelText(
      /API key/,
    ) as HTMLInputElement;
    expect(key.type).toBe("password");
    expect(key.autocomplete).toBe("off");
  });

  it("runs a cloud request only with cloud_consent true, and keeps the key out of storage and the page", async () => {
    const onExecute = vi.fn<(request: AiRequest) => AiOutput>(() => SUMMARY);
    panel({ onExecute });
    await grantConsent(SECRET);
    expect(badge().textContent).toContain("Cloud AI (consent granted for session)");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await screen.findByText("Short.");
    expect(onExecute).toHaveBeenCalledTimes(1);
    expect(onExecute.mock.calls[0]?.[0]).toEqual({
      tool: "ai.summarize",
      params: { length: "standard", provider: "cloud", cloud_consent: true, api_key: SECRET },
    });
    expect(JSON.stringify({ ...localStorage })).not.toContain(SECRET);
    expect(JSON.stringify({ ...sessionStorage })).not.toContain(SECRET);
    expect(window.location.href).not.toContain(SECRET);
    expect(document.body.innerHTML).not.toContain(SECRET);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("revokes consent and returns to Local, so the next request carries no consent", async () => {
    const onExecute = vi.fn<(request: AiRequest) => AiOutput>(() => SUMMARY);
    panel({ onExecute, localRuntime: true });
    await grantConsent(SECRET);
    fireEvent.click(screen.getByRole("button", { name: "Revoke & switch to Local" }));
    expect(badge().getAttribute("data-provider")).toBe("local");
    expect(badge().textContent).toContain("0 bytes uploaded");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(onExecute).toHaveBeenCalledTimes(1));
    const params = onExecute.mock.calls[0]?.[0].params as Record<string, unknown>;
    expect(params.provider).toBe("local");
    expect(params).not.toHaveProperty("cloud_consent");
    expect(params).not.toHaveProperty("api_key");
    // Switching back to Cloud asks again.
    fireEvent.click(screen.getByRole("button", { name: "Cloud AI" }));
    expect(await screen.findByRole("dialog")).toBeTruthy();
  });

  it("shows a cloud hand-off command that reads the key from the environment", async () => {
    panel({ inputName: "a.pdf" });
    await grantConsent(SECRET);
    const command = screen.getByText(/printf %s/).textContent ?? "";
    expect(command).toContain('"$ASHIFT_AI_API_KEY"');
    expect(command).toContain("cloud_consent=true");
    expect(document.body.textContent).not.toContain(SECRET);
  });

  it("never reports a confirmation from the dialog without the checked box", () => {
    const onConfirm = vi.fn();
    render(
      createElement(CloudConsentDialog, {
        open: true,
        operation: "Ask",
        onConfirm,
        onCancel: vi.fn(),
      }),
    );
    const dialog = screen.getByRole("dialog");
    fireEvent.submit(dialog.querySelector("form") as HTMLFormElement);
    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm and continue" }));
    expect(onConfirm).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("checkbox"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm and continue" }));
    expect(onConfirm).toHaveBeenCalledWith({ consent: true });
  });

  it("names the operation that would run", () => {
    render(
      createElement(CloudConsentDialog, {
        open: true,
        operation: "Translate",
        onConfirm: vi.fn(),
        onCancel: vi.fn(),
      }),
    );
    expect(screen.getByRole("dialog").textContent).toContain("Translate");
  });
});

describe("tools", () => {
  it("summarises on a loaded local model with the chosen length and language", async () => {
    const onExecute = vi.fn<(request: AiRequest) => AiOutput>(() => ({
      summary: "All about invoices.",
      bullet_points: ["Due in 30 days", "Net terms"],
      language: "vi",
      length: "brief",
    }));
    const copyText = vi.fn().mockResolvedValue(undefined);
    const onInsertNote = vi.fn();
    panel({ localRuntime: true, onExecute, copyText, onInsertNote });
    fireEvent.change(screen.getByLabelText("Length"), { target: { value: "brief" } });
    fireEvent.change(screen.getByLabelText("Output language"), { target: { value: "vi" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await screen.findByText("All about invoices.");
    expect(onExecute).toHaveBeenCalledWith({
      tool: "ai.summarize",
      params: { length: "brief", language: "vi", provider: "local" },
    });
    expect(screen.getByText("Due in 30 days")).toBeTruthy();
    const text = "All about invoices.\n- Due in 30 days\n- Net terms";
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(copyText).toHaveBeenCalledWith(text));
    fireEvent.click(screen.getByRole("button", { name: "Insert as note" }));
    expect(onInsertNote).toHaveBeenCalledWith(text);
  });

  it("asks with Enter, blocks an empty question and renders clickable page citations", async () => {
    const onExecute = vi.fn<(request: AiRequest) => AiOutput>(() => ({
      answer: "Payment is due on 1 March.",
      citations: [
        { page: 3, quote: "due on 1 March", score: 0.92 },
        { page: 7, quote: "late fee applies", score: 0.41 },
      ],
    }));
    const onJumpToPage = vi.fn();
    panel({ localRuntime: true, onExecute, onJumpToPage });
    await openTab("Ask");
    const input = screen.getByLabelText("Question");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onExecute).not.toHaveBeenCalled();
    expect(screen.getAllByRole("alert").length).toBeGreaterThan(0);

    fireEvent.change(input, { target: { value: "When is payment due?" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText("Payment is due on 1 March.");
    expect(onExecute).toHaveBeenCalledWith({
      tool: "ai.ask",
      params: { question: "When is payment due?", max_citations: 5, provider: "local" },
    });
    const chip = screen.getByRole("button", { name: "Go to page 3" });
    expect(chip.textContent).toContain("p. 3");
    expect(chip.textContent).toContain("due on 1 March");
    expect(chip.textContent).toContain("92%");
    fireEvent.click(chip);
    expect(onJumpToPage).toHaveBeenCalledWith(3, "due on 1 March");
    fireEvent.click(screen.getByRole("button", { name: "Go to page 7" }));
    expect(onJumpToPage).toHaveBeenLastCalledWith(7, "late fee applies");
  });

  it("refuses a citation limit outside 1..20", async () => {
    const onExecute = vi.fn();
    panel({ localRuntime: true, onExecute });
    await openTab("Ask");
    fireEvent.change(screen.getByLabelText("Question"), { target: { value: "q?" } });
    fireEvent.change(screen.getByLabelText("Citations (1–20)"), { target: { value: "21" } });
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    expect(onExecute).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Citations (1–20)"), { target: { value: "20" } });
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    await waitFor(() => expect(onExecute).toHaveBeenCalledTimes(1));
  });

  it("translates with the chosen languages and format and offers a safe download", async () => {
    const onExecute = vi.fn<(request: AiRequest) => AiOutput>(() => ({
      artifact: { name: "report.vi.docx", href: "blob:https://app/xyz" },
    }));
    panel({ localRuntime: true, onExecute });
    await openTab("Translate");
    expect(screen.getByText(/Structure, IDs, images and styles are preserved/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Translate into"), { target: { value: "ja" } });
    fireEvent.change(screen.getByLabelText("From"), { target: { value: "en" } });
    fireEvent.change(screen.getByLabelText("Output format"), { target: { value: "docx" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await screen.findByText(/Translation ready/);
    expect(onExecute).toHaveBeenCalledWith({
      tool: "ai.translate",
      params: { target_lang: "ja", source_lang: "en", format: "docx", provider: "local" },
    });
    expect(screen.getByRole("link", { name: "Download" }).getAttribute("href")).toBe(
      "blob:https://app/xyz",
    );
  });

  it("does not render a script URL returned as a download link", async () => {
    panel({
      localRuntime: true,
      onExecute: () => ({ artifact: { name: "x.pdf", href: "javascript:alert(1)" } }),
    });
    await openTab("Translate");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await screen.findByText(/Translation ready/);
    expect(screen.queryByRole("link", { name: "Download" })).toBeNull();
  });

  it("extracts tables for a custom page range, previews them and downloads CSV", async () => {
    const onExecute = vi.fn<(request: AiRequest) => AiOutput>(() => ({
      tables: [
        {
          page: 2,
          rows: [
            ["Item", "Cost"],
            ["Pen", "=1+1"],
          ],
        },
      ],
    }));
    const saveFile = vi.fn();
    panel({ localRuntime: true, onExecute, saveFile, pageCount: 10 });
    await openTab("Extract tables");
    fireEvent.change(screen.getByLabelText("Pages"), { target: { value: "custom" } });
    fireEvent.change(screen.getByLabelText("Custom range"), { target: { value: "2-4" } });
    fireEvent.change(screen.getByLabelText("Output format"), { target: { value: "xlsx" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await screen.findByText("Extracted tables");
    expect(onExecute).toHaveBeenCalledWith({
      tool: "ai.extract_tables",
      params: { pages: "2-4", format: "xlsx", provider: "local" },
    });
    expect(screen.getByRole("columnheader", { name: "Item" })).toBeTruthy();
    expect(screen.getByRole("cell", { name: "Pen" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Download CSV" }));
    expect(saveFile).toHaveBeenCalledTimes(1);
    const [name, blob] = saveFile.mock.calls[0] as [string, Blob];
    expect(name).toBe("tables.csv");
    expect(await blob.text()).toContain("Pen,'=1+1");
  });

  it("rejects an invalid page range without running", async () => {
    const onExecute = vi.fn();
    panel({ localRuntime: true, onExecute, pageCount: 4 });
    await openTab("Extract tables");
    fireEvent.change(screen.getByLabelText("Pages"), { target: { value: "custom" } });
    fireEvent.change(screen.getByLabelText("Custom range"), { target: { value: "9" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(onExecute).not.toHaveBeenCalled();
    expect(screen.getByRole("alert").textContent).toContain("page count");
  });

  it("runs the local quick CSV tool with empty params", async () => {
    const onExecute = vi.fn().mockResolvedValue(undefined);
    panel({ localRuntime: true, onExecute });
    await openTab("Extract tables");
    fireEvent.click(screen.getByRole("button", { name: "Quick CSV (local)" }));
    await waitFor(() =>
      expect(onExecute).toHaveBeenCalledWith({ tool: "pdf.tables_to_csv", params: {} }),
    );
  });

  it("says so when no tables are found", async () => {
    panel({ localRuntime: true, onExecute: () => ({ tables: [] }) });
    await openTab("Extract tables");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByText(/No tables found/)).toBeTruthy();
  });

  it("surfaces a runner failure and a malformed result", async () => {
    const onExecute = vi
      .fn<(request: AiRequest) => Promise<AiOutput>>()
      .mockRejectedValueOnce(new Error("Model unavailable"))
      .mockResolvedValueOnce({ nonsense: true } as unknown as AiOutput);
    panel({ localRuntime: true, onExecute });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Model unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("The request failed."));
  });

  it("does not start a second request while one is running", async () => {
    let finish: (value: AiOutput) => void = () => {};
    const onExecute = vi.fn(() => new Promise<AiOutput>((resolve) => (finish = resolve)));
    panel({ localRuntime: true, onExecute });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    fireEvent.click(screen.getByRole("button", { name: "Working…" }));
    expect(onExecute).toHaveBeenCalledTimes(1);
    await act(async () => finish(SUMMARY));
    await screen.findByText("Short.");
  });
});

describe("recipe prompt", () => {
  it("drafts an imported recipe offline and opens it for review without executing", async () => {
    const onExecute = vi.fn();
    const onOpenRecipeDraft = vi.fn();
    panel({ onExecute, onOpenRecipeDraft });
    await openTab("Recipe");
    const prompt = screen.getByLabelText("Describe the steps");
    fireEvent.change(prompt, { target: { value: "merge, OCR, compress under 5 MB" } });
    fireEvent.keyDown(prompt, { key: "Enter" });
    expect(await screen.findByText("pdf.merge")).toBeTruthy();
    expect(screen.getByText("pdf.ocr")).toBeTruthy();
    expect(screen.getByText("pdf.compress")).toBeTruthy();
    expect(screen.getByText(/never runs by itself/)).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: "Open in Recipe Builder (review required)" }),
    );
    expect(onOpenRecipeDraft).toHaveBeenCalledTimes(1);
    const draft = onOpenRecipeDraft.mock.calls[0]?.[0];
    expect(draft.version).toBe("ariad-recipe/1");
    expect(draft.steps.map((s: { tool: string }) => s.tool)).toEqual([
      "pdf.merge",
      "pdf.ocr",
      "pdf.compress",
    ]);
    expect(onExecute).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("ignores Shift+Enter, blocks an empty prompt and explains an unrecognised one", async () => {
    panel();
    await openTab("Recipe");
    const prompt = screen.getByLabelText("Describe the steps");
    fireEvent.keyDown(prompt, { key: "Enter" });
    expect(screen.queryByText("Draft steps")).toBeNull();
    fireEvent.change(prompt, { target: { value: "make coffee" } });
    fireEvent.keyDown(prompt, { key: "Enter", shiftKey: true });
    expect(screen.queryByText(/No known steps found/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Draft recipe" }));
    expect(await screen.findByText(/No known steps found/)).toBeTruthy();
  });

  it("scrubs a key from a cloud recipe draft before handing it to the builder", async () => {
    const onOpenRecipeDraft = vi.fn();
    const onExecute = vi.fn(() => ({
      recipe: {
        version: "ariad-recipe/1" as const,
        steps: [{ tool: "pdf.merge", params: { api_key: SECRET } }],
      },
      origin: "imported" as const,
      requires_confirmation: true as const,
    }));
    panel({ onExecute, onOpenRecipeDraft });
    await openTab("Recipe");
    await grantConsent(SECRET);
    fireEvent.change(screen.getByLabelText("Describe the steps"), { target: { value: "merge" } });
    fireEvent.click(screen.getByRole("button", { name: "Draft recipe" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Open in Recipe Builder (review required)" }),
    );
    expect(onExecute).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(onOpenRecipeDraft.mock.calls)).not.toContain(SECRET);
    expect(document.body.textContent).not.toContain(SECRET);
  });
});

describe("labels", () => {
  it("accepts partial label overrides and keeps English defaults for the rest", () => {
    panel({
      labels: {
        title: "Trợ lý AI",
        badgeLocal: "Trên thiết bị · 0 byte tải lên",
        tabs: { ask: "Hỏi" } as never,
      },
    });
    expect(screen.getByRole("complementary", { name: "Trợ lý AI" })).toBeTruthy();
    expect(badge().textContent).toContain("Trên thiết bị · 0 byte tải lên");
    expect(tab("Hỏi")).toBeTruthy();
    expect(tab("Summarise")).toBeTruthy();
  });
});
