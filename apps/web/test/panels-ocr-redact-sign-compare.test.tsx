// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { expect as baseExpect } from "vitest";
import { ComparePanel, isCompareTool, VIEW_MODES } from "@/components/tools/panels/compare";
import { isOcrTool, OCR_TOOLS, OcrPanel } from "@/components/tools/panels/ocr";
import { filterLanguages } from "@/components/tools/panels/ocr/languages";
import {
  isRedactTool,
  REDACT_TOOLS,
  RedactionVerificationCard,
  type RedactionVerificationReport,
  RedactPanel,
} from "@/components/tools/panels/redact";
import {
  DATE_REGEX,
  parseRect,
  SSN_REGEX,
  validateRegex,
} from "@/components/tools/panels/redact/targets";
import {
  isSignTool,
  SIGN_TOOLS,
  SignatureValidationCard,
  SignPanel,
  type SignValidationReport,
  signatureStatus,
} from "@/components/tools/panels/sign";
import { toVectorData } from "@/components/tools/panels/sign/signature-pad";
import type { ParamsValidation } from "@/components/tools/params-form";
import type { WorkspaceFile } from "@/components/workspace/types";
import { toolCatalog } from "@/converter/tool-catalog";

// jsdom lacks the browser APIs that floating and range components probe for.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView ??= () => {};
Element.prototype.getAnimations ??= () => [];
window.matchMedia ??= ((query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addEventListener() {},
  removeEventListener() {},
  addListener() {},
  removeListener() {},
  dispatchEvent: () => false,
})) as typeof window.matchMedia;

// The repository has no jest-dom setup, so the handful of DOM matchers these tests need live here.
interface DomMatchers {
  toBeInTheDocument(): void;
  toHaveAttribute(name: string, value?: string): void;
  toBeDisabled(): void;
  toBeEnabled(): void;
  toHaveValue(value: string | undefined): void;
  toHaveTextContent(text: string | RegExp): void;
  toHaveAccessibleName(name: string): void;
}
type Matchable = ReturnType<typeof baseExpect<unknown>>;
// Shadows the global so the matchers added below type-check without a module augmentation.
const expect = baseExpect as ((
  actual: unknown,
) => Matchable & DomMatchers & { not: Matchable["not"] & DomMatchers }) &
  typeof baseExpect;
const asElement = (received: unknown) => received as HTMLElement;
expect.extend({
  toBeInTheDocument(received: unknown) {
    const pass = received instanceof Element && document.contains(received);
    return { pass, message: () => `expected element ${pass ? "not " : ""}to be in the document` };
  },
  toHaveAttribute(received: unknown, name: string, value?: string) {
    const element = asElement(received);
    const actual = element.getAttribute(name);
    const pass = value === undefined ? actual !== null : actual === value;
    return {
      pass,
      message: () =>
        `expected ${name}=${String(actual)} ${pass ? "not " : ""}to match ${String(value)}`,
    };
  },
  toBeDisabled(received: unknown) {
    const pass = asElement(received).matches(":disabled");
    return { pass, message: () => `expected element ${pass ? "not " : ""}to be disabled` };
  },
  toBeEnabled(received: unknown) {
    const pass = !asElement(received).matches(":disabled");
    return { pass, message: () => `expected element ${pass ? "not " : ""}to be enabled` };
  },
  toHaveValue(received: unknown, value: string | undefined) {
    const actual = (received as HTMLInputElement).value;
    return {
      pass: actual === value,
      message: () => `expected value ${JSON.stringify(actual)} to be ${JSON.stringify(value)}`,
    };
  },
  toHaveTextContent(received: unknown, text: string | RegExp) {
    const actual = asElement(received).textContent ?? "";
    const pass = typeof text === "string" ? actual.includes(text) : text.test(actual);
    return {
      pass,
      message: () =>
        `expected text ${JSON.stringify(actual)} ${pass ? "not " : ""}to contain ${String(text)}`,
    };
  },
  toHaveAccessibleName(received: unknown, name: string) {
    const element = asElement(received);
    const actual = element.getAttribute("aria-label") ?? element.textContent ?? "";
    return {
      pass: actual === name,
      message: () =>
        `expected accessible name ${JSON.stringify(actual)} to be ${JSON.stringify(name)}`,
    };
  },
});

const catalog = toolCatalog();
const tool = (id: string) => {
  const found = catalog.find((entry) => entry.id === id);
  if (!found) throw new Error(`No tool ${id}`);
  return found;
};

function workspaceFile(name: string, pageCount: number | null = 10): WorkspaceFile {
  return {
    id: `id-${name}`,
    key: `key-${name}`,
    name,
    size: 1024,
    pageCount,
    file: new File(["%PDF-1.7"], name, { type: "application/pdf" }),
  };
}

type Emission = { params: Record<string, unknown>; validation: ParamsValidation };

/** Renders a panel and exposes the most recent parameters and validation it reported. */
function mount(
  Panel: (props: never) => React.ReactNode,
  toolId: string,
  props: Record<string, unknown> = {},
) {
  const emissions: Emission[] = [];
  const onChange = vi.fn((params: Record<string, unknown>, validation: ParamsValidation) => {
    emissions.push({ params, validation });
  });
  const view = render(
    createElement(Panel as never, {
      tool: tool(toolId),
      files: [workspaceFile("a.pdf")],
      pageCount: 10,
      onChange,
      ...props,
    }),
  );
  const last = () => {
    const emission = emissions[emissions.length - 1];
    if (!emission) throw new Error("The panel has not reported yet");
    return emission;
  };
  return { ...view, onChange, emissions, last };
}

afterEach(() => {
  cleanup();
  localStorage.clear();
  sessionStorage.clear();
});

describe("OCR panel", () => {
  it("recognises its tools", () => {
    expect([...OCR_TOOLS].every(isOcrTool)).toBe(true);
    expect(isOcrTool("pdf.merge")).toBe(false);
    for (const id of OCR_TOOLS) expect(catalog.some((entry) => entry.id === id)).toBe(true);
  });

  it("starts with Vietnamese and English and reports valid parameters", () => {
    const { last } = mount(OcrPanel, "pdf.ocr");
    expect(last().params).toMatchObject({
      languages: ["vie", "eng"],
      mode: "skip-text",
      output: "pdf",
      pages: "all",
      dpi: 300,
      deskew: false,
      auto_rotate: false,
      clean: false,
    });
    expect(last().validation.isValid).toBe(true);
    expect(screen.getByRole("button", { name: "Vietnamese (vie)" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("lists Vietnamese first among the language choices", () => {
    mount(OcrPanel, "pdf.ocr");
    const group = screen.getByRole("group", { name: "Available languages" });
    expect(within(group).getAllByRole("button")[0]).toHaveAccessibleName("Vietnamese (vie)");
  });

  it("adds and removes languages in priority order", () => {
    const { last } = mount(OcrPanel, "pdf.ocr");
    fireEvent.click(screen.getByRole("button", { name: "Japanese (jpn)" }));
    expect(last().params.languages).toEqual(["vie", "eng", "jpn"]);
    fireEvent.click(screen.getByRole("button", { name: "Vietnamese (vie)" }));
    expect(last().params.languages).toEqual(["eng", "jpn"]);
    fireEvent.click(screen.getByRole("button", { name: "Vietnamese (vie)" }));
    expect(last().params.languages).toEqual(["eng", "jpn", "vie"]);
  });

  it("rejects an empty language selection", () => {
    const { last } = mount(OcrPanel, "pdf.ocr");
    fireEvent.click(screen.getByRole("button", { name: "Vietnamese (vie)" }));
    fireEvent.click(screen.getByRole("button", { name: "English (eng)" }));
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.languages).toBe("Choose at least one language.");
  });

  it("filters languages by name, native name and code without caring about accents", () => {
    expect(filterLanguages("tieng").map((language) => language.code)).toEqual(["vie"]);
    expect(filterLanguages("JPN").map((language) => language.code)).toEqual(["jpn"]);
    expect(filterLanguages("chinese").map((language) => language.code)).toEqual([
      "chi_sim",
      "chi_tra",
    ]);
    mount(OcrPanel, "pdf.ocr");
    fireEvent.change(screen.getByRole("searchbox", { name: "Search languages" }), {
      target: { value: "korean" },
    });
    const group = screen.getByRole("group", { name: "Available languages" });
    expect(within(group).getAllByRole("button")).toHaveLength(1);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search languages" }), {
      target: { value: "zzz" },
    });
    expect(screen.getByText("No language matches your search.")).toBeInTheDocument();
  });

  it("emits the scan clean-up toggles and the existing-text policy", () => {
    const { last } = mount(OcrPanel, "pdf.ocr");
    fireEvent.click(screen.getByRole("checkbox", { name: /Straighten skewed pages/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Fix page orientation/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Clean the background/ }));
    expect(last().params).toMatchObject({ deskew: true, auto_rotate: true, clean: true });
    fireEvent.click(screen.getByRole("button", { name: "Force OCR" }));
    expect(last().params.mode).toBe("force");
    fireEvent.click(screen.getByRole("button", { name: "Redo" }));
    expect(last().params.mode).toBe("redo");
    fireEvent.click(screen.getByRole("button", { name: "Searchable PDF + plain text" }));
    expect(last().params.output).toBe("pdf+txt");
  });

  it("validates the page range and the resolution against the schema", () => {
    const { last } = mount(OcrPanel, "pdf.ocr");
    fireEvent.change(screen.getByLabelText("Pages to recognise"), { target: { value: "99" } });
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.pages).toMatch(/exceeds page count/);
    fireEvent.change(screen.getByLabelText("Pages to recognise"), { target: { value: "1-3" } });
    fireEvent.change(screen.getByLabelText("Render resolution (DPI)"), {
      target: { value: "100" },
    });
    expect(last().validation.errors.dpi).toBe("Minimum: 150");
    fireEvent.change(screen.getByLabelText("Render resolution (DPI)"), { target: { value: "" } });
    expect(last().validation.isValid).toBe(true);
    expect(last().params).not.toHaveProperty("dpi");
  });

  it("explains the text layer with an overlay and a plain extraction view", () => {
    mount(OcrPanel, "pdf.ocr");
    const layer = () => document.querySelector('[data-slot="ocr-text-layer"]') as HTMLElement;
    expect(screen.getByText("Scanned page image")).toBeInTheDocument();
    expect(layer()).toHaveAttribute("data-view", "overlay");
    // The primary language drives the sample text.
    expect(screen.getByText("Hợp đồng dịch vụ số 042")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Plain text extraction" }));
    expect(layer()).toHaveAttribute("data-view", "plain");
    expect(screen.getByText(/without the scan or its layout/)).toBeInTheDocument();
  });

  it("offers image OCR only its own parameters", () => {
    const { last } = mount(OcrPanel, "image.ocr");
    expect(Object.keys(last().params).sort()).toEqual(["dpi", "languages", "output"]);
    expect(screen.queryByRole("checkbox", { name: /Straighten/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Plain text" }));
    expect(last().params.output).toBe("txt");
    expect(last().validation.isValid).toBe(true);
  });

  it("covers deskew and auto-rotate with their own parameters", () => {
    const deskew = mount(OcrPanel, "pdf.deskew");
    expect(deskew.last().params).toMatchObject({
      pages: "all",
      dpi: 150,
      max_angle_deg: 10,
      min_angle_deg: 0.1,
    });
    fireEvent.change(screen.getByLabelText("Largest angle to correct (degrees)"), {
      target: { value: "60" },
    });
    expect(deskew.last().validation.errors.max_angle_deg).toBe("Maximum: 45");
    deskew.unmount();
    const rotate = mount(OcrPanel, "pdf.auto_rotate");
    expect(rotate.last().params).toMatchObject({ pages: "all", dpi: 300, min_confidence: 2 });
    fireEvent.change(screen.getByLabelText("Minimum confidence to rotate"), {
      target: { value: "200" },
    });
    expect(rotate.last().validation.isValid).toBe(false);
    expect(rotate.last().validation.errors.min_confidence).toBe("Maximum: 100");
  });

  it("disables every control while the tool runs", () => {
    mount(OcrPanel, "pdf.ocr", { disabled: true });
    expect(screen.getByRole("searchbox", { name: "Search languages" })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: /Clean the background/ })).toBeDisabled();
  });
});

describe("Redact panel", () => {
  const addTerm = (text: string) => {
    fireEvent.change(screen.getByLabelText("Add a word or phrase"), { target: { value: text } });
    fireEvent.keyDown(screen.getByLabelText("Add a word or phrase"), { key: "Enter" });
  };

  it("recognises its tools", () => {
    expect([...REDACT_TOOLS].every(isRedactTool)).toBe(true);
    expect(isRedactTool("pdf.ocr")).toBe(false);
  });

  it("warns that redaction is permanent and needs a target before it is valid", () => {
    const { last } = mount(RedactPanel, "pdf.redact");
    expect(screen.getByText("Redaction is permanent")).toBeInTheDocument();
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.targets).toBe(
      "Add at least one word, pattern or area to redact.",
    );
  });

  it("collects words and phrases as exact targets", () => {
    const { last } = mount(RedactPanel, "pdf.redact");
    addTerm("Project Atlas");
    addTerm("  Project Atlas  ");
    addTerm("Nguyễn Văn A");
    expect(last().params.targets).toEqual([
      { kind: "exact", text: "Project Atlas" },
      { kind: "exact", text: "Nguyễn Văn A" },
    ]);
    expect(last().validation.isValid).toBe(true);
    expect(screen.getByText("2 items selected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove Project Atlas" }));
    expect(last().params.targets).toEqual([{ kind: "exact", text: "Nguyễn Văn A" }]);
  });

  it("turns the sensitive pattern chips into detector and regex targets", () => {
    const { last } = mount(RedactPanel, "pdf.redact");
    fireEvent.click(screen.getByRole("button", { name: "Email" }));
    fireEvent.click(screen.getByRole("button", { name: "Credit Card" }));
    fireEvent.click(screen.getByRole("button", { name: "SSN / National ID" }));
    fireEvent.click(screen.getByRole("button", { name: "Date" }));
    expect(last().params.targets).toEqual([
      { kind: "preset", preset: "email" },
      { kind: "preset", preset: "credit_card" },
      { kind: "preset", preset: "cccd" },
      { kind: "preset", preset: "cmnd" },
      { kind: "regex", pattern: SSN_REGEX },
      { kind: "regex", pattern: DATE_REGEX },
    ]);
    expect(last().validation.isValid).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Email" }));
    expect(last().params.targets).not.toContainEqual({ kind: "preset", preset: "email" });
  });

  it("validates custom regular expressions as they are typed", () => {
    expect(validateRegex("")).toBeNull();
    expect(validateRegex("INV-\\d{6}")).toBeNull();
    expect(validateRegex("(")).toMatch(/^Invalid regular expression/);
    expect(validateRegex("(?=a)b")).toBe("Lookahead and lookbehind are not supported.");
    expect(validateRegex("(a)\\1")).toBe("Back-references are not supported.");
    expect(validateRegex("x".repeat(1025))).toMatch(/at most 1024 bytes/);

    const { last } = mount(RedactPanel, "pdf.redact");
    expect(screen.queryByLabelText("Regular expression")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Custom Regex" }));
    const input = screen.getByLabelText("Regular expression");
    const add = within(input.closest("[data-slot='redact-regex']") as HTMLElement).getByRole(
      "button",
      { name: "Add" },
    );
    fireEvent.change(input, { target: { value: "(" } });
    const regexPanel = document.querySelector("[data-slot='redact-regex']") as HTMLElement;
    expect(within(regexPanel).getByRole("alert")).toHaveTextContent(/Invalid regular expression/);
    expect(add).toBeDisabled();
    fireEvent.change(input, { target: { value: "INV-\\d{6}" } });
    expect(screen.getByText("Valid pattern.")).toBeInTheDocument();
    expect(add).toBeEnabled();
    fireEvent.click(add);
    expect(last().params.targets).toEqual([{ kind: "regex", pattern: "INV-\\d{6}" }]);
  });

  it("adds a page area as [page, left, bottom, right, top]", () => {
    expect(parseRect({ page: "", left: "", bottom: "", right: "", top: "" })).toEqual({
      error: null,
    });
    expect(parseRect({ page: "1", left: "9", bottom: "0", right: "9", top: "5" })).toEqual({
      error: "Left must be smaller than right.",
    });
    expect(parseRect({ page: "11", left: "0", bottom: "0", right: "9", top: "5" }, 10)).toEqual({
      error: "The document has 10 pages.",
    });

    const { last } = mount(RedactPanel, "pdf.redact");
    const area = (label: string, value: string) =>
      fireEvent.change(screen.getByLabelText(label), { target: { value } });
    area("Page", "2");
    area("Left", "50");
    area("Bottom", "700");
    area("Right", "300");
    area("Top", "740");
    fireEvent.click(screen.getByRole("button", { name: "Add area" }));
    expect(last().params.targets).toEqual([
      {
        kind: "rectangle",
        page: 2,
        rect: { left: 50, bottom: 700, right: 300, top: 740 },
      },
    ]);
    expect(screen.getByText("[2, 50, 700, 300, 740]")).toBeInTheDocument();
    expect(last().validation.isValid).toBe(true);
  });

  it("refuses an inverted or incomplete area", () => {
    mount(RedactPanel, "pdf.redact");
    const area = (label: string, value: string) =>
      fireEvent.change(screen.getByLabelText(label), { target: { value } });
    area("Page", "1");
    area("Left", "300");
    expect(screen.getByText("Fill in all five values.")).toBeInTheDocument();
    area("Bottom", "0");
    area("Right", "50");
    area("Top", "10");
    expect(screen.getByText("Left must be smaller than right.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add area" })).toBeDisabled();
  });

  it("sets the fill colour and the overlay label", () => {
    const { last } = mount(RedactPanel, "pdf.redact");
    addTerm("secret");
    expect(last().params).toMatchObject({ color: "#000000", invalidate_signatures: false });
    expect(last().params).not.toHaveProperty("overlay_text");
    fireEvent.click(screen.getByRole("button", { name: "Use fill #dc2626" }));
    fireEvent.click(screen.getByRole("button", { name: "CONFIDENTIAL" }));
    expect(last().params).toMatchObject({ color: "#dc2626", overlay_text: "CONFIDENTIAL" });
    fireEvent.click(screen.getByRole("button", { name: "Use fill #ffffff" }));
    fireEvent.change(screen.getByLabelText("Label on the redacted area"), {
      target: { value: "SEALED" },
    });
    expect(last().params).toMatchObject({ color: "#ffffff", overlay_text: "SEALED" });
    fireEvent.click(screen.getByRole("button", { name: "No label" }));
    expect(last().params).not.toHaveProperty("overlay_text");
  });

  it("reports the carriers to clean and the signature consent", () => {
    const { last } = mount(RedactPanel, "pdf.redact");
    addTerm("secret");
    fireEvent.click(screen.getByRole("checkbox", { name: "Embedded files" }));
    expect(last().params.scope).toMatchObject({ attachments: false, text: true });
    expect(screen.getByText(/verification report partial/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /invalidate existing signatures/ }));
    expect(last().params.invalidate_signatures).toBe(true);
    expect(last().validation.isValid).toBe(true);
  });

  it("keeps the standalone verifier free of appearance settings", () => {
    const { last } = mount(RedactPanel, "pdf.redact_verify");
    addTerm("secret");
    expect(Object.keys(last().params).sort()).toEqual(["scope", "targets"]);
    expect(screen.queryByText("Redaction is permanent")).not.toBeInTheDocument();
    expect(screen.queryByText("Redaction appearance")).not.toBeInTheDocument();
    expect(screen.getByText(/never reports Verified/)).toBeInTheDocument();
    expect(last().validation.isValid).toBe(true);
  });

  it("caps text patterns at the engine limit", () => {
    const { last } = mount(RedactPanel, "pdf.redact");
    for (let index = 0; index < 101; index += 1) addTerm(`term-${index}`);
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.targets).toBe("Use at most 100 words, patterns and detectors.");
  });

  it("previews the verification checklist before anything runs", () => {
    mount(RedactPanel, "pdf.redact");
    const card = document.querySelector('[data-slot="redaction-verification-card"]');
    expect(card).toHaveAttribute("data-status", "planned");
    expect(screen.getByText("What is checked after you apply")).toBeInTheDocument();
  });
});

describe("Redaction verification card", () => {
  const passed = {
    pdfium_text: "pass",
    core_text: "pass",
    region_render: "pass",
    image_pixels: "pass",
    object_graph: "pass",
    rewrite_history: "pass",
  } as const;
  const card = () =>
    document.querySelector('[data-slot="redaction-verification-card"]') as HTMLElement;

  it("lists the six gates as pending before a run", () => {
    render(<RedactionVerificationCard />);
    expect(card()).toHaveAttribute("data-status", "planned");
    expect(screen.getByText("What is checked after you apply")).toBeInTheDocument();
    const items = within(card()).getAllByRole("listitem");
    expect(items).toHaveLength(6);
    for (const item of items) expect(item).toHaveAttribute("data-result", "pending");
    expect(screen.getByText("Independent text re-extraction")).toBeInTheDocument();
  });

  it("shows a verified report with every gate passed and the removal counts", () => {
    const report: RedactionVerificationReport = {
      status: "verified",
      checks: passed,
      target_count: 3,
      match_count: 17,
      carriers: { text_removed: 17, metadata_scrubbed: 2, attachments_removed: 0 },
    };
    render(<RedactionVerificationCard report={report} />);
    expect(card()).toHaveAttribute("data-status", "verified");
    expect(screen.getByText("Verified")).toBeInTheDocument();
    expect(
      screen.getByText(/physically removed and cannot be copied or searched/),
    ).toBeInTheDocument();
    for (const item of within(card()).getAllByRole("listitem").slice(0, 6))
      expect(item).toHaveAttribute("data-result", "pass");
    expect(screen.getByText("Occurrences removed").nextSibling).toHaveTextContent("17");
    expect(screen.getByText("Text occurrences").nextSibling).toHaveTextContent("17");
    expect(screen.getByText("Metadata entries scrubbed")).toBeInTheDocument();
    expect(screen.queryByText("Images modified")).not.toBeInTheDocument();
  });

  it("flags a failed gate and its warnings", () => {
    render(
      <RedactionVerificationCard
        report={{
          status: "failed",
          checks: { ...passed, pdfium_text: "fail", image_pixels: "unavailable" },
          target_count: 1,
          match_count: 1,
          warnings: ["Existing signatures were invalidated."],
        }}
      />,
    );
    expect(card()).toHaveAttribute("data-status", "failed");
    expect(screen.getByText(/Do not share this file/)).toBeInTheDocument();
    const results = within(card())
      .getAllByRole("listitem")
      .filter((item) => item.hasAttribute("data-check"))
      .map((item) => item.getAttribute("data-result"));
    expect(results).toEqual(["fail", "pass", "pass", "unavailable", "pass", "pass"]);
    expect(screen.getByText("Existing signatures were invalidated.")).toBeInTheDocument();
  });

  it("calls a report with excluded carriers partial", () => {
    render(
      <RedactionVerificationCard
        standalone
        report={{ status: "partial", checks: passed, target_count: 1, match_count: 0 }}
      />,
    );
    expect(screen.getByText("Partial")).toBeInTheDocument();
    expect(screen.getByText(/never reports Verified/)).toBeInTheDocument();
  });
});

describe("Sign panel", () => {
  const pad = () => screen.getByRole("img", { name: "Signature drawing area" });
  const scribble = () => {
    fireEvent.pointerDown(pad(), { clientX: 20, clientY: 30, pointerId: 1 });
    fireEvent.pointerMove(pad(), { clientX: 120, clientY: 90, pointerId: 1 });
    fireEvent.pointerMove(pad(), { clientX: 220, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(pad(), { pointerId: 1 });
  };

  it("recognises its tools", () => {
    expect([...SIGN_TOOLS].every(isSignTool)).toBe(true);
    expect(isSignTool("pdf.sign_token")).toBe(false);
    for (const id of SIGN_TOOLS) expect(catalog.some((entry) => entry.id === id)).toBe(true);
  });

  describe("visual signature", () => {
    it("needs a drawing before it is valid, then emits stroke coordinates", () => {
      const { last } = mount(SignPanel, "pdf.sign_visual");
      expect(last().params.mode).toBe("draw");
      expect(last().validation.isValid).toBe(false);
      expect(last().validation.errors.vector_data).toBe("Draw your signature to continue.");
      scribble();
      const vector = String(last().params.vector_data);
      expect(vector.split(" ").length).toBeGreaterThanOrEqual(3);
      expect(last().validation.isValid).toBe(true);
      expect(document.querySelector('[data-slot="signature-stroke"]')).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Clear" }));
      expect(last().validation.isValid).toBe(false);
      expect(last().params).not.toHaveProperty("vector_data");
    });

    it("scales pad points into the signature box with y pointing up", () => {
      expect(
        toVectorData(
          [
            [0, 160],
            [400, 0],
          ],
          { width: 180, height: 60 },
        ),
      ).toBe("0,0 180,60");
      expect(toVectorData([[200, 80]], { width: 100, height: 50 })).toBe("50,25");
    });

    it("types a signature and previews three styles", () => {
      const { last } = mount(SignPanel, "pdf.sign_visual");
      fireEvent.click(screen.getByRole("tab", { name: "Type" }));
      expect(last().params.mode).toBe("type");
      expect(last().validation.errors.text).toBeDefined();
      fireEvent.change(screen.getByLabelText("Signature text"), {
        target: { value: "Ada Lovelace" },
      });
      expect(last().params.text).toBe("Ada Lovelace");
      expect(last().validation.isValid).toBe(true);
      const previews = within(screen.getByRole("list", { name: "Style previews" }));
      expect(previews.getAllByText("Ada Lovelace")).toHaveLength(3);
      expect(previews.getByText("Calligraphic")).toBeInTheDocument();
      expect(previews.getByText("Serif")).toBeInTheDocument();
      expect(previews.getByText("Sans")).toBeInTheDocument();
    });

    it("uploads a PNG or JPEG as base64 and refuses anything else", async () => {
      const { last } = mount(SignPanel, "pdf.sign_visual");
      fireEvent.click(screen.getByRole("tab", { name: "Upload image" }));
      expect(last().params.mode).toBe("image");
      expect(last().validation.isValid).toBe(false);
      const input = screen.getByLabelText("Signature image");

      fireEvent.change(input, {
        target: { files: [new File(["gif"], "stamp.gif", { type: "image/gif" })] },
      });
      expect(await screen.findByText("Choose a PNG or JPEG image.")).toBeInTheDocument();
      expect(last().validation.isValid).toBe(false);

      fireEvent.change(input, {
        target: { files: [new File(["png-bytes"], "stamp.png", { type: "image/png" })] },
      });
      await waitFor(() => expect(last().params.image_data).toBe(btoa("png-bytes")));
      expect(last().validation.isValid).toBe(true);
      expect(screen.getByAltText("Selected signature image")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Remove image" }));
      expect(last().validation.isValid).toBe(false);
    });

    it("places the box on a page and validates it", () => {
      const { last } = mount(SignPanel, "pdf.sign_visual");
      fireEvent.click(screen.getByRole("tab", { name: "Type" }));
      fireEvent.change(screen.getByLabelText("Signature text"), { target: { value: "Ada" } });
      expect(last().params).toMatchObject({ page: 1, rect: [72, 72, 252, 132] });
      expect(document.querySelector('[data-slot="sign-placement-box"]')).toBeInTheDocument();
      fireEvent.change(screen.getByLabelText("Right (x2)"), { target: { value: "10" } });
      expect(last().validation.isValid).toBe(false);
      expect(last().validation.errors.rect).toBe(
        "Left must be smaller than right, and bottom smaller than top.",
      );
      fireEvent.change(screen.getByLabelText("Right (x2)"), { target: { value: "300" } });
      fireEvent.change(screen.getByLabelText("Page"), { target: { value: "11" } });
      expect(last().validation.errors.page).toBe("The document has 10 pages.");
      fireEvent.change(screen.getByLabelText("Page"), { target: { value: "3" } });
      fireEvent.change(screen.getByLabelText("Signer name"), { target: { value: "Ada L." } });
      expect(last().validation.isValid).toBe(true);
      expect(last().params).toMatchObject({
        page: 3,
        rect: [72, 72, 300, 132],
        signer_name: "Ada L.",
      });
    });
  });

  describe("certificate signing", () => {
    const certificate = (name = "me.p12") =>
      new File(["not-a-real-key"], name, { type: "application/x-pkcs12" });
    const choose = (file: File) =>
      fireEvent.change(screen.getByLabelText("Certificate file (.p12 or .pfx)"), {
        target: { files: [file] },
      });

    it("explains that signing runs on the computer and keeps secrets in memory", () => {
      mount(SignPanel, "pdf.sign");
      const note = document.querySelector('[data-slot="sign-certificate-note"]') as HTMLElement;
      expect(note).toHaveTextContent(/never written to storage/);
      expect(note).toHaveTextContent(
        /runs on your computer through the sign engine or the ashift command/,
      );
    });

    it("needs a certificate and a passphrase", () => {
      const { last } = mount(SignPanel, "pdf.sign");
      expect(last().validation.isValid).toBe(false);
      expect(last().validation.errors.certificate).toBe("Choose the certificate to sign with.");
      expect(last().validation.errors.password).toBe(
        "Enter the passphrase, or confirm that the file has none.",
      );
      choose(certificate("me.pem"));
      expect(last().validation.errors.certificate).toBe(
        "Choose a PKCS#12 file ending in .p12 or .pfx.",
      );
      choose(certificate("me.PFX"));
      expect(last().validation.errors.certificate).toBeUndefined();
      fireEvent.click(screen.getByRole("checkbox", { name: "This certificate has no passphrase" }));
      expect(last().validation.isValid).toBe(true);
      expect(screen.getByLabelText("Certificate passphrase")).toBeDisabled();
    });

    it("never emits, stores or links the certificate or its passphrase", () => {
      const { last, emissions } = mount(SignPanel, "pdf.sign");
      choose(certificate());
      expect(screen.getByText("me.p12 · 14 bytes")).toBeInTheDocument();
      fireEvent.change(screen.getByLabelText("Certificate passphrase"), {
        target: { value: "hunter2-passphrase" },
      });
      expect(screen.getByLabelText("Certificate passphrase")).toHaveAttribute("type", "password");
      expect(last().validation.isValid).toBe(true);
      expect(last().params).toMatchObject({
        certificate: { $secret: "prompt" },
        password: { $secret: "prompt" },
        level: "B-B",
      });
      const everything = JSON.stringify(emissions);
      expect(everything).not.toContain("hunter2-passphrase");
      expect(everything).not.toContain("not-a-real-key");
      expect(localStorage.length).toBe(0);
      expect(sessionStorage.length).toBe(0);
      expect(window.location.href).not.toContain("hunter2");
      fireEvent.click(screen.getByRole("button", { name: "Forget certificate and passphrase" }));
      expect(screen.getByLabelText("Certificate passphrase")).toHaveValue("");
      expect(last().validation.isValid).toBe(false);
    });

    it("collects signer details and a visible signature with whole-number placement", () => {
      const { last } = mount(SignPanel, "pdf.sign");
      choose(certificate());
      fireEvent.change(screen.getByLabelText("Certificate passphrase"), { target: { value: "x" } });
      fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "Approval" } });
      fireEvent.change(screen.getByLabelText("Location"), {
        target: { value: "Ho Chi Minh City" },
      });
      fireEvent.change(screen.getByLabelText("Contact info"), {
        target: { value: "ada@example.com" },
      });
      fireEvent.change(screen.getByLabelText("Signer name"), { target: { value: "Ada Lovelace" } });
      expect(last().params).toMatchObject({
        reason: "Approval",
        location: "Ho Chi Minh City",
        contact: "ada@example.com",
      });
      expect(last().params).not.toHaveProperty("visible");
      fireEvent.click(screen.getByRole("checkbox", { name: /Show a visible signature/ }));
      expect(last().params.visible).toEqual({
        page: 1,
        rect: [72, 72, 252, 132],
        appearance: "Ada Lovelace",
      });
      fireEvent.change(screen.getByLabelText("Left (x1)"), { target: { value: "72.5" } });
      expect(last().validation.isValid).toBe(false);
      expect(last().validation.errors.visible).toBe("Use whole numbers.");
    });

    it("requires a timestamp authority above the basic level", () => {
      const { last } = mount(SignPanel, "pdf.sign");
      choose(certificate());
      fireEvent.click(screen.getByRole("checkbox", { name: "This certificate has no passphrase" }));
      expect(last().validation.isValid).toBe(true);
      fireEvent.click(screen.getByRole("button", { name: "B-T · with timestamp" }));
      expect(last().params.level).toBe("B-T");
      expect(last().validation.errors.tsa_url).toBe(
        "Enter the timestamp authority URL for this level.",
      );
      fireEvent.change(screen.getByLabelText("Timestamp authority URL"), {
        target: { value: "https://tsa.example.com" },
      });
      expect(last().validation.isValid).toBe(true);
      expect(last().params.tsa_url).toBe("https://tsa.example.com");
    });
  });

  it("configures signature validation and shows the empty result card", () => {
    const { last } = mount(SignPanel, "pdf.sign_verify");
    expect(last().params).toEqual({
      fetch_revocation: false,
      trust_anchors: [],
      offline_evidence: [],
    });
    expect(
      screen.getByText("Validation results appear here after the check runs."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /Check revocation online/ }));
    fireEvent.change(screen.getByLabelText("Trusted root certificates"), {
      target: { value: "/etc/ca/root.pem\n\n /etc/ca/other.der " },
    });
    expect(last().params).toMatchObject({
      fetch_revocation: true,
      trust_anchors: ["/etc/ca/root.pem", "/etc/ca/other.der"],
    });
    expect(last().validation.isValid).toBe(true);
  });

  it("makes removing a signature an explicit decision", () => {
    const { last } = mount(SignPanel, "pdf.sign_remove");
    expect(last().params).toEqual({ confirm_invalidation: false });
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.confirm_invalidation).toBe("Confirm to continue.");
    fireEvent.click(
      screen.getByRole("checkbox", { name: /removing a signature breaks the document/ }),
    );
    fireEvent.change(screen.getByLabelText("Signature field to remove"), {
      target: { value: "Signature2" },
    });
    expect(last().params).toEqual({ confirm_invalidation: true, field_name: "Signature2" });
    expect(last().validation.isValid).toBe(true);
  });
});

describe("Signature validation card", () => {
  const signer = (name: string, issuer = "Example CA") => {
    const subject = `Common Name: ${name}, Organisation: Ariad`;
    return {
      subject,
      issuer: issuer === "self" ? subject : `Common Name: ${issuer}`,
      serial_number: "42",
    };
  };
  const base = {
    integrity: true,
    valid: true,
    coverage: "entire_file" as const,
    certificate_status: "valid",
    revocation_status: "not_revoked" as const,
    modifications: { docmdp_ok: true, bottom_line: true, modification_level: "none" },
  };
  const report = (...signatures: SignValidationReport["signatures"]): SignValidationReport => ({
    valid: signatures.every((entry) => entry.valid),
    integrity: signatures.every((entry) => entry.integrity),
    signature_count: signatures.length,
    signatures,
  });

  it("derives the status badge", () => {
    expect(signatureStatus({ ...base, field_name: "S", signer: signer("Ada") })).toBe("VALID");
    expect(
      signatureStatus({ ...base, valid: false, field_name: "S", signer: signer("Ada", "self") }),
    ).toBe("SELF_SIGNED");
    expect(
      signatureStatus({
        ...base,
        valid: false,
        integrity: false,
        field_name: "S",
        signer: signer("Ada", "self"),
      }),
    ).toBe("INVALID");
    expect(signatureStatus({ ...base, valid: false, field_name: "S", signer: signer("Ada") })).toBe(
      "INVALID",
    );
  });

  it("shows signer, issuer, time, digest and coverage for a valid signature", () => {
    render(
      <SignatureValidationCard
        locale="en"
        report={report({
          ...base,
          field_name: "Signature1",
          signer: signer("Ada Lovelace"),
          signing_time: "2026-03-12T08:30:00+00:00",
          timestamp: { timestamp: "2026-03-12T08:31:00+00:00", integrity: true, valid: true },
          digest_algorithm: "SHA-256",
        })}
      />,
    );
    expect(screen.getByText("All signatures valid")).toBeInTheDocument();
    const entry = document.querySelector('[data-slot="signature-entry"]') as HTMLElement;
    expect(entry).toHaveAttribute("data-status", "VALID");
    expect(within(entry).getByText("VALID")).toBeInTheDocument();
    expect(within(entry).getByText("Ada Lovelace")).toBeInTheDocument();
    expect(within(entry).getByText("Example CA")).toBeInTheDocument();
    expect(within(entry).getByText("Mar 12, 2026, 8:30 AM UTC")).toBeInTheDocument();
    expect(within(entry).getByText("Mar 12, 2026, 8:31 AM UTC")).toBeInTheDocument();
    expect(within(entry).getByText("SHA-256")).toBeInTheDocument();
    expect(
      within(entry).getByText("Entire file: nothing changed after signing"),
    ).toBeInTheDocument();
    expect(within(entry).getByText("Not revoked")).toBeInTheDocument();
    expect(
      within(entry).getByText(/Permitted by the document's change policy/),
    ).toBeInTheDocument();
  });

  it("marks a self-signed certificate and an invalid signature differently", () => {
    render(
      <SignatureValidationCard
        report={report(
          { ...base, valid: false, field_name: "Self", signer: signer("Ada", "self") },
          {
            ...base,
            valid: false,
            integrity: false,
            coverage: "partial",
            field_name: "Broken",
            signer: signer("Bob"),
            revocation_status: "revoked",
            error: "digest mismatch",
          },
        )}
      />,
    );
    expect(screen.getByText("Not every signature is valid")).toBeInTheDocument();
    const [self, broken] = Array.from(document.querySelectorAll('[data-slot="signature-entry"]'));
    expect(self).toHaveAttribute("data-status", "SELF_SIGNED");
    expect(within(self as HTMLElement).getByText("SELF-SIGNED")).toBeInTheDocument();
    expect(broken).toHaveAttribute("data-status", "INVALID");
    expect(within(broken as HTMLElement).getByText("Broken")).toBeInTheDocument();
    expect(
      within(broken as HTMLElement).getByText("Partial: part of the file is not covered"),
    ).toBeInTheDocument();
    expect(within(broken as HTMLElement).getByText("Revoked")).toBeInTheDocument();
    expect(within(broken as HTMLElement).getByRole("alert")).toHaveTextContent("digest mismatch");
    // A report that omits fields says so instead of inventing them.
    expect(within(self as HTMLElement).getAllByText("Not reported").length).toBeGreaterThan(0);
  });

  it("says so when the document has no signatures", () => {
    render(
      <SignatureValidationCard
        report={{ valid: false, integrity: false, signature_count: 0, signatures: [] }}
      />,
    );
    expect(screen.getByText("This document has no signatures.")).toBeInTheDocument();
    expect(document.querySelector('[data-slot="signature-entry"]')).not.toBeInTheDocument();
  });
});

describe("Compare panel", () => {
  const files = [workspaceFile("contract-v1.pdf", 12), workspaceFile("contract-v2.pdf", 8)];

  it("recognises its tool", () => {
    expect(isCompareTool("pdf.compare")).toBe(true);
    expect(isCompareTool("pdf.merge")).toBe(false);
    expect(VIEW_MODES).toEqual({ "side-by-side": "both", overlay: "visual", text: "text" });
  });

  it("selects the first two files as baseline and revised and reports their order", () => {
    const onInputsChange = vi.fn();
    const { last } = mount(ComparePanel, "pdf.compare", { files, onInputsChange });
    expect(screen.getByLabelText("Baseline (File A)")).toHaveValue(files[0]?.id);
    expect(screen.getByLabelText("Revised (File B)")).toHaveValue(files[1]?.id);
    expect(onInputsChange).toHaveBeenLastCalledWith([files[0]?.id, files[1]?.id]);
    expect(last().params).toEqual({ mode: "both" });
    expect(last().validation.isValid).toBe(true);
  });

  it("swaps baseline and revised with A ⇄ B", () => {
    const onInputsChange = vi.fn();
    mount(ComparePanel, "pdf.compare", { files, onInputsChange });
    fireEvent.click(screen.getByRole("button", { name: "Swap baseline and revised" }));
    expect(screen.getByLabelText("Baseline (File A)")).toHaveValue(files[1]?.id);
    expect(screen.getByLabelText("Revised (File B)")).toHaveValue(files[0]?.id);
    expect(onInputsChange).toHaveBeenLastCalledWith([files[1]?.id, files[0]?.id]);
    expect(screen.getByRole("button", { name: "Swap baseline and revised" })).toHaveTextContent(
      "A ⇄ B",
    );
  });

  it("lets either side pick a different file and refuses the same one twice", () => {
    const three = [...files, workspaceFile("contract-v3.pdf", 8)];
    const onInputsChange = vi.fn();
    const { last } = mount(ComparePanel, "pdf.compare", { files: three, onInputsChange });
    fireEvent.change(screen.getByLabelText("Revised (File B)"), {
      target: { value: three[2]?.id },
    });
    expect(onInputsChange).toHaveBeenLastCalledWith([three[0]?.id, three[2]?.id]);
    fireEvent.change(screen.getByLabelText("Revised (File B)"), {
      target: { value: three[0]?.id },
    });
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.inputs).toBe("Choose two different files.");
  });

  it("needs exactly two files", () => {
    const { last } = mount(ComparePanel, "pdf.compare", { files: [files[0]] });
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.inputs).toBe(
      "Add a second PDF to compare. Comparison needs exactly two files.",
    );
    expect(screen.getByRole("button", { name: "Swap baseline and revised" })).toBeDisabled();
  });

  it("validates the page filter against the shorter document", () => {
    const { last } = mount(ComparePanel, "pdf.compare", { files });
    fireEvent.change(screen.getByLabelText("Page range"), { target: { value: "1-5" } });
    expect(last().params.pages).toBe("1-5");
    expect(last().validation.isValid).toBe(true);
    fireEvent.change(screen.getByLabelText("Page range"), { target: { value: "1-9" } });
    expect(last().validation.isValid).toBe(false);
    expect(last().validation.errors.pages).toMatch(/exceeds page count/);
    fireEvent.change(screen.getByLabelText("Page range"), { target: { value: "" } });
    expect(last().params).not.toHaveProperty("pages");
  });

  it("switches between side-by-side, overlay and text diff views", () => {
    const onViewOptionsChange = vi.fn();
    const { last } = mount(ComparePanel, "pdf.compare", { files, onViewOptionsChange });
    const preview = () => document.querySelector('[data-slot="compare-preview"]') as HTMLElement;
    expect(preview()).toHaveAttribute("data-view", "side-by-side");
    expect(screen.getByLabelText(/Sensitivity threshold/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Visual Overlay" }));
    expect(last().params.mode).toBe("visual");
    expect(preview()).toHaveAttribute("data-view", "overlay");

    fireEvent.click(screen.getByRole("button", { name: "Text Diff List" }));
    expect(last().params.mode).toBe("text");
    expect(preview()).toHaveAttribute("data-view", "text");
    expect(screen.queryByLabelText(/Sensitivity threshold/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Highlight colour")).not.toBeInTheDocument();
    expect(within(preview()).getByText("− Net 30 days")).toBeInTheDocument();
    expect(within(preview()).getByText("+ Net 45 days")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Side-by-Side" }));
    expect(last().params.mode).toBe("both");
    expect(onViewOptionsChange).toHaveBeenLastCalledWith({ highlight: "#dc2626", threshold: 10 });
  });

  it("reports the overlay highlight colour and threshold", () => {
    const onViewOptionsChange = vi.fn();
    const { last } = mount(ComparePanel, "pdf.compare", { files, onViewOptionsChange });
    fireEvent.click(screen.getByRole("button", { name: "Visual Overlay" }));
    fireEvent.change(screen.getByLabelText(/Sensitivity threshold/), { target: { value: "35" } });
    fireEvent.change(screen.getByLabelText("Highlight colour"), { target: { value: "#00ff00" } });
    expect(onViewOptionsChange).toHaveBeenLastCalledWith({ highlight: "#00ff00", threshold: 35 });
    expect(screen.getByText("Sensitivity threshold: 35%")).toBeInTheDocument();
    // Display settings are not tool parameters.
    expect(last().params).toEqual({ mode: "visual" });
    expect(last().validation.isValid).toBe(true);
  });

  it("falls back to file order when a file is re-ingested under a new id", () => {
    const onInputsChange = vi.fn();
    const { rerender } = mount(ComparePanel, "pdf.compare", { files, onInputsChange });
    const reingested = files.map((file) => ({ ...file, id: `${file.id}-2` }));
    rerender(
      createElement(ComparePanel, {
        tool: tool("pdf.compare"),
        files: reingested,
        pageCount: 8,
        onChange: vi.fn(),
        onInputsChange,
      }),
    );
    expect(onInputsChange).toHaveBeenLastCalledWith([reingested[0]?.id, reingested[1]?.id]);
  });
});
