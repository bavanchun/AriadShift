// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { COMPRESS_TOOLS, CompressPanel, isCompressTool } from "@/components/tools/panels/compress";
import {
  COMPRESS_PRESETS,
  compressionRatio,
  estimateSize,
} from "@/components/tools/panels/compress/compress-model";
import { formatBytes } from "@/components/tools/panels/compress/panel-contract";
import { isSecurityTool, SECURITY_TOOLS, SecurityPanel } from "@/components/tools/panels/security";
import { passwordStrength } from "@/components/tools/panels/security/security-model";
import {
  isWatermarkTool,
  WATERMARK_TOOLS,
  WatermarkPanel,
} from "@/components/tools/panels/watermark";
import {
  anchorPoint,
  batesLabel,
  defaultSettings,
  expandTemplate,
  overlayItems,
} from "@/components/tools/panels/watermark/watermark-model";
import type { WorkspaceFile } from "@/components/workspace/types";
import { toolCatalog } from "@/converter/tool-catalog";

const catalog = toolCatalog();
const tool = (id: string) => {
  const found = catalog.find((item) => item.id === id);
  if (!found) throw new Error(`tool ${id} is not in the registry`);
  return found;
};

function workspaceFile(size = 2_400_000, pageCount: number | null = 12): WorkspaceFile {
  return {
    id: "input-1",
    key: "key-1",
    name: "report.pdf",
    size,
    pageCount,
    file: new File(["%PDF"], "report.pdf", { type: "application/pdf" }),
  };
}

type Emit = ReturnType<typeof vi.fn>;
const lastCall = (onChange: Emit) => {
  const call = onChange.mock.calls.at(-1);
  if (!call) throw new Error("onChange was never called");
  return {
    params: call[0] as Record<string, unknown>,
    validation: call[1] as { isValid: boolean; errors: Record<string, string> },
  };
};

afterEach(cleanup);

describe("panel tool lists", () => {
  it("only name tools the registry defines", () => {
    for (const id of [...COMPRESS_TOOLS, ...SECURITY_TOOLS, ...WATERMARK_TOOLS]) {
      expect(
        catalog.some((item) => item.id === id),
        id,
      ).toBe(true);
    }
  });

  it("recognise their own tools and no others", () => {
    expect(isCompressTool("pdf.compress")).toBe(true);
    expect(isCompressTool("pdf.encrypt")).toBe(false);
    expect(isSecurityTool("pdf.sanitize")).toBe(true);
    expect(isSecurityTool("pdf.watermark")).toBe(false);
    expect(isWatermarkTool("pdf.qr")).toBe(true);
    expect(isWatermarkTool("pdf.compress")).toBe(false);
  });
});

describe("size estimation", () => {
  it("shrinks more for lower resolution and quality", () => {
    const screen = compressionRatio({ preset: "screen", dpi: 72, quality: 50, grayscale: false });
    const ebook = compressionRatio({ preset: "ebook", dpi: 150, quality: 70, grayscale: false });
    const print = compressionRatio({ preset: "print", dpi: 300, quality: 90, grayscale: false });
    expect(screen).toBeLessThan(ebook);
    expect(ebook).toBeLessThan(print);
    expect(
      compressionRatio({ preset: "lossless", dpi: null, quality: null, grayscale: false }),
    ).toBeGreaterThan(print - 0.05);
  });

  it("brackets the expected size and never claims growth", () => {
    const estimate = estimateSize(1_000_000, 0.4);
    expect(estimate.low).toBe(300_000);
    expect(estimate.high).toBe(500_000);
    expect(estimate.reductionPercent).toBe(60);
    expect(estimateSize(1_000_000, 1).reductionPercent).toBe(0);
    expect(estimateSize(1_000_000, 1).high).toBe(1_000_000);
  });

  it("formats bytes in decimal units", () => {
    expect(formatBytes(2_400_000, "en")).toBe("2.4 MB");
    expect(formatBytes(900, "en")).toBe("900 B");
  });
});

describe("CompressPanel", () => {
  it("offers the four level presets", () => {
    render(
      <CompressPanel
        tool={tool("pdf.compress")}
        files={[workspaceFile()]}
        onChange={vi.fn()}
        locale="en"
      />,
    );
    for (const text of [
      "Screen · 72 DPI",
      "eBook · 150 DPI",
      "Printer · 300 DPI",
      "Prepress / Lossless",
    ]) {
      expect(
        screen.getByRole("button", { name: new RegExp(text.replace("/", "\\/")) }),
      ).toBeTruthy();
    }
    expect(COMPRESS_PRESETS).toHaveLength(4);
  });

  it("emits the chosen preset with its resolution and quality", () => {
    const onChange = vi.fn();
    render(
      <CompressPanel
        tool={tool("pdf.compress")}
        files={[workspaceFile()]}
        onChange={onChange}
        locale="en"
      />,
    );
    expect(lastCall(onChange).params).toMatchObject({
      preset: "ebook",
      max_dpi: 150,
      jpeg_quality: 70,
    });

    fireEvent.click(screen.getByRole("button", { name: /Screen/ }));
    expect(lastCall(onChange).params).toEqual({ preset: "screen", max_dpi: 72, jpeg_quality: 50 });
    expect(screen.getByRole("button", { name: /Screen/ }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    expect(lastCall(onChange).validation.isValid).toBe(true);
  });

  it("drops image limits for the lossless profile", () => {
    const onChange = vi.fn();
    render(
      <CompressPanel
        tool={tool("pdf.compress")}
        files={[workspaceFile()]}
        onChange={onChange}
        locale="en"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Lossless/ }));
    expect(lastCall(onChange).params).toEqual({ preset: "lossless" });
    expect(screen.getByTestId("dpi-value").textContent).toBe("Original");
  });

  it("leaves the named profile when a slider moves", () => {
    const onChange = vi.fn();
    render(
      <CompressPanel
        tool={tool("pdf.compress")}
        files={[workspaceFile()]}
        onChange={onChange}
        locale="en"
      />,
    );
    fireEvent.change(screen.getByRole("slider", { name: /Image resolution/ }), {
      target: { value: "96" },
    });
    expect(lastCall(onChange).params).toEqual({ max_dpi: 96, jpeg_quality: 70 });
    expect(screen.getByTestId("dpi-value").textContent).toBe("96 DPI");
    fireEvent.change(screen.getByRole("slider", { name: /Image quality/ }), {
      target: { value: "40" },
    });
    expect(lastCall(onChange).params).toEqual({ max_dpi: 96, jpeg_quality: 40 });
    expect(screen.getByTestId("quality-value").textContent).toBe("40%");
    for (const button of screen.getAllByRole("button"))
      expect(button.getAttribute("aria-pressed")).toBe("false");
  });

  it("estimates the before and after size from the file", () => {
    render(
      <CompressPanel
        tool={tool("pdf.compress")}
        files={[workspaceFile(2_400_000)]}
        onChange={vi.fn()}
        locale="en"
      />,
    );
    const estimate = estimateSize(
      2_400_000,
      compressionRatio({ preset: "ebook", dpi: 150, quality: 70, grayscale: false }),
    );
    const text = screen.getByTestId("size-estimate").textContent ?? "";
    expect(text).toContain("2.4 MB");
    expect(text).toContain(formatBytes(estimate.low, "en"));
    expect(text).toContain(formatBytes(estimate.high, "en"));
    expect(text).toContain(`-${estimate.reductionPercent}%`);

    fireEvent.click(screen.getByRole("button", { name: /Screen/ }));
    const smaller = estimateSize(
      2_400_000,
      compressionRatio({ preset: "screen", dpi: 72, quality: 50, grayscale: false }),
    );
    expect(smaller.reductionPercent).toBeGreaterThan(estimate.reductionPercent);
    expect(screen.getByTestId("size-estimate").textContent).toContain(
      `-${smaller.reductionPercent}%`,
    );
  });

  it("asks for a file when there is none", () => {
    render(<CompressPanel tool={tool("pdf.compress")} files={[]} onChange={vi.fn()} locale="en" />);
    expect(screen.queryByTestId("size-estimate")).toBeNull();
    expect(screen.getByText(/Add a file/)).toBeTruthy();
  });

  it("adds grayscale to the compression parameters and the estimate", () => {
    const onChange = vi.fn();
    render(
      <CompressPanel
        tool={tool("pdf.compress")}
        files={[workspaceFile()]}
        onChange={onChange}
        locale="en"
      />,
    );
    const before = screen.getByTestId("size-estimate").textContent;
    fireEvent.click(screen.getByLabelText(/Convert images to grayscale/));
    expect(lastCall(onChange).params).toMatchObject({ grayscale: true });
    expect(screen.getByTestId("size-estimate").textContent).not.toBe(before);
  });

  it("previews grayscale before and after, with no parameters", () => {
    const onChange = vi.fn();
    render(
      <CompressPanel
        tool={tool("pdf.grayscale")}
        files={[workspaceFile()]}
        onChange={onChange}
        locale="en"
      />,
    );
    expect(screen.getByRole("img", { name: /Before/ }).getAttribute("data-variant")).toBe("before");
    expect(screen.getByRole("img", { name: /After/ }).className).toContain("grayscale");
    expect(screen.getByText(/contrast is preserved/)).toBeTruthy();
    expect(lastCall(onChange).params).toEqual({});
    expect(screen.queryByText("Compression level")).toBeNull();
    expect(screen.getByTestId("size-estimate").textContent).toContain("-25%");
  });

  it("explains optimisation for linearize without promising savings", () => {
    render(
      <CompressPanel
        tool={tool("pdf.linearize")}
        files={[workspaceFile()]}
        onChange={vi.fn()}
        locale="en"
      />,
    );
    expect(screen.getByText(/fast web view/)).toBeTruthy();
    expect(screen.getByTestId("size-estimate").textContent).toContain("≈ 0%");
  });
});

describe("password strength", () => {
  it("grows with length and variety", () => {
    expect(passwordStrength("").level).toBe("empty");
    expect(passwordStrength("abc").level).toBe("weak");
    expect(passwordStrength("correcthorse").bits).toBeGreaterThan(passwordStrength("abc").bits);
    expect(passwordStrength("Tr0ub4dor&3-horse-staple").level).toBe("strong");
  });

  it("discounts repeated characters", () => {
    expect(passwordStrength("aaaaaaaaaaaaaaaa").bits).toBeLessThan(
      passwordStrength("abcdefghijklmnop").bits,
    );
  });
});

describe("SecurityPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  it("rejects an empty owner password and accepts one once typed", () => {
    const onChange = vi.fn();
    render(
      <SecurityPanel tool={tool("pdf.encrypt")} files={[workspaceFile()]} onChange={onChange} />,
    );
    expect(lastCall(onChange).validation).toEqual({
      isValid: false,
      errors: { owner_password: "Enter an owner password." },
    });
    const field = screen.getByLabelText("Owner password");
    fireEvent.blur(field);
    expect(screen.getByRole("alert").textContent).toBe("Enter an owner password.");

    fireEvent.change(field, { target: { value: "s3cret-Owner!" } });
    const { params, validation } = lastCall(onChange);
    expect(validation.isValid).toBe(true);
    expect(params).toMatchObject({ algorithm: "aes-256", owner_password: "s3cret-Owner!" });
    expect(params).not.toHaveProperty("user_password");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("includes the user password only when one is typed", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Owner password"), { target: { value: "owner" } });
    fireEvent.change(screen.getByLabelText("User password (optional)"), {
      target: { value: "reader" },
    });
    expect(lastCall(onChange).params).toMatchObject({
      owner_password: "owner",
      user_password: "reader",
    });
  });

  it("shows and hides a password", () => {
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={vi.fn()} />);
    const field = screen.getByLabelText("Owner password") as HTMLInputElement;
    expect(field.type).toBe("password");
    fireEvent.click(screen.getByRole("button", { name: "Show Owner password" }));
    expect(field.type).toBe("text");
    fireEvent.click(screen.getByRole("button", { name: "Hide Owner password" }));
    expect(field.type).toBe("password");
  });

  it("shows a live strength meter", () => {
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={vi.fn()} />);
    expect(screen.getByTestId("strength-owner").textContent).toBe("No password");
    const meter = screen.getByRole("meter", {
      name: /Owner password strength/,
    }) as HTMLMeterElement;
    expect(meter.value).toBe(0);
    fireEvent.change(screen.getByLabelText("Owner password"), { target: { value: "abc" } });
    expect(screen.getByTestId("strength-owner").textContent).toContain("Weak");
    fireEvent.change(screen.getByLabelText("Owner password"), {
      target: { value: "Tr0ub4dor&3-horse-staple" },
    });
    expect(screen.getByTestId("strength-owner").textContent).toContain("Strong");
    expect(meter.value).toBe(4);
  });

  it("badges the cipher and lets the user pick the older one", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={onChange} />);
    expect(screen.getByTestId("cipher-badge").textContent).toBe("AES-256");
    fireEvent.change(screen.getByLabelText("Cipher"), { target: { value: "aes-128" } });
    expect(screen.getByTestId("cipher-badge").textContent).toBe("AES-128");
    expect(lastCall(onChange).params.algorithm).toBe("aes-128");
  });

  it("keeps secrets out of storage and the address bar", () => {
    const before = window.location.href;
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Owner password"), {
      target: { value: "never-persist-me" },
    });
    fireEvent.change(screen.getByLabelText("User password (optional)"), {
      target: { value: "nor-me" },
    });
    expect(JSON.stringify({ ...localStorage })).not.toContain("never-persist-me");
    expect(JSON.stringify({ ...sessionStorage })).not.toContain("nor-me");
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(window.location.href).toBe(before);
    for (const label of ["Owner password", "User password (optional)"]) {
      expect(screen.getByLabelText(label).getAttribute("autocomplete")).toBe("new-password");
    }
  });

  it("drops the passwords when the tool changes", () => {
    const { rerender } = render(
      <SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={vi.fn()} />,
    );
    fireEvent.change(screen.getByLabelText("Owner password"), { target: { value: "owner" } });
    rerender(<SecurityPanel tool={tool("pdf.permissions")} files={[]} onChange={vi.fn()} />);
    expect((screen.getByLabelText("Owner password") as HTMLInputElement).value).toBe("");
  });

  it("maps the permissions matrix onto the engine fields", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).params.permissions).toEqual({
      print: true,
      print_high: true,
      copy: true,
      modify: true,
      annotate: true,
      fill_forms: true,
      assemble: true,
      extract_accessibility: true,
    });
    fireEvent.click(screen.getByLabelText(/^Print/));
    fireEvent.click(screen.getByLabelText(/^Copy text and graphics/));
    fireEvent.click(screen.getByLabelText(/^Annotate and fill forms/));
    expect(lastCall(onChange).params.permissions).toMatchObject({
      print: false,
      print_high: false,
      copy: false,
      annotate: false,
      fill_forms: false,
      modify: true,
      assemble: true,
    });
    fireEvent.click(screen.getByLabelText(/^Modify document/));
    fireEvent.click(screen.getByLabelText(/^Extract pages/));
    expect(lastCall(onChange).params.permissions).toMatchObject({ modify: false, assemble: false });
  });

  it("changes permissions with a required owner password", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.permissions")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).validation.isValid).toBe(false);
    expect(screen.queryByLabelText("Cipher")).toBeNull();
    fireEvent.change(screen.getByLabelText("Owner password"), { target: { value: "owner" } });
    expect(lastCall(onChange).validation.isValid).toBe(true);
    expect(lastCall(onChange).params).not.toHaveProperty("algorithm");
  });

  it("decrypts with an optional password", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.decrypt")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange)).toEqual({ params: {}, validation: { isValid: true, errors: {} } });
    expect(screen.queryByLabelText("Cipher")).toBeNull();
    fireEvent.change(screen.getByLabelText("Document password"), {
      target: { value: "open-sesame" },
    });
    expect(lastCall(onChange).params).toEqual({ password: "open-sesame" });
  });

  it("audits what sanitize will strip", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.sanitize")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).params).toEqual({
      javascript: true,
      embedded_files: true,
      links: false,
      metadata: false,
      xmp: false,
      thumbnails: true,
      forms: "keep",
    });
    expect(screen.getByTestId("sanitize-summary").textContent).toContain("3 kinds");
    const links = screen.getByLabelText(/Launch and URI actions/);
    fireEvent.click(links);
    expect(lastCall(onChange).params.links).toBe(true);
    expect(within(links.closest("label") as HTMLElement).getByText("Will be removed")).toBeTruthy();
    fireEvent.click(screen.getByLabelText(/JavaScript actions/));
    expect(lastCall(onChange).params.javascript).toBe(false);
    expect(
      within(
        (screen.getByLabelText(/JavaScript actions/) as HTMLElement).closest(
          "label",
        ) as HTMLElement,
      ).getByText("Kept"),
    ).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Form submit actions"), { target: { value: "remove" } });
    expect(lastCall(onChange).params.forms).toBe("remove");
    expect(screen.getByTestId("sanitize-summary").textContent).toContain("4 kinds");
  });

  it("strips all metadata by default and lists fields when asked to pick", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.metadata")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).params).toEqual({ strip_all: true });
    expect(screen.queryByLabelText(/Author/)).toBeNull();

    fireEvent.click(screen.getByLabelText(/Strip all metadata/));
    expect(lastCall(onChange).validation.errors.remove).toBe(
      "Choose at least one field to remove.",
    );
    expect(screen.getByRole("alert").textContent).toBe("Choose at least one field to remove.");

    fireEvent.click(screen.getByLabelText(/^Author/));
    fireEvent.click(screen.getByLabelText(/^Keywords/));
    expect(lastCall(onChange)).toEqual({
      params: { strip_all: false, remove: ["Author", "Keywords"] },
      validation: { isValid: true, errors: {} },
    });
    fireEvent.click(screen.getByLabelText(/^Author/));
    expect(lastCall(onChange).params.remove).toEqual(["Keywords"]);
  });

  it("unlocks forms with no parameters", () => {
    const onChange = vi.fn();
    render(<SecurityPanel tool={tool("pdf.unlock_forms")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).params).toEqual({});
    expect(screen.getByText(/read-only flags/)).toBeTruthy();
  });

  it("disables every control while a tool runs", () => {
    render(<SecurityPanel tool={tool("pdf.encrypt")} files={[]} onChange={vi.fn()} disabled />);
    for (const control of document.querySelectorAll("input, select, button")) {
      expect(control.matches(":disabled"), control.outerHTML).toBe(true);
    }
  });
});

describe("overlay maths", () => {
  it("places anchors against the margin and applies upward offsets", () => {
    expect(anchorPoint("top-left", 36)).toEqual({ x: 36, y: 36, textAnchor: "start" });
    expect(anchorPoint("bottom-right", 18)).toEqual({
      x: 595 - 18,
      y: 842 - 18,
      textAnchor: "end",
    });
    expect(anchorPoint("center", 0, 10, 20)).toEqual({
      x: 595 / 2 + 10,
      y: 842 / 2 - 20,
      textAnchor: "middle",
    });
  });

  it("expands templates and Bates labels", () => {
    expect(expandTemplate("Page {n} of {total} on {date}", { n: 3, total: 9, date: "d" })).toBe(
      "Page 3 of 9 on d",
    );
    expect(batesLabel({ prefix: "ACME-", suffix: "-X", digits: 4, start: 7 })).toBe("ACME-0007-X");
  });

  it("shows nothing for an empty watermark", () => {
    expect(
      overlayItems("pdf.watermark", { ...defaultSettings("pdf.watermark"), text: "" }),
    ).toEqual([]);
  });
});

describe("WatermarkPanel", () => {
  const overlayText = () => screen.getAllByTestId("overlay-text");
  const groupOf = (element: Element) => element.parentElement as Element;

  it("previews the default watermark and updates with its text", () => {
    const onChange = vi.fn();
    render(
      <WatermarkPanel tool={tool("pdf.watermark")} files={[workspaceFile()]} onChange={onChange} />,
    );
    expect(overlayText()[0]?.textContent).toBe("CONFIDENTIAL");
    expect(lastCall(onChange).params).toMatchObject({
      kind: "text",
      text: "CONFIDENTIAL",
      anchor: "center",
      layout: "diagonal",
      opacity: 0.3,
      rotation_deg: 45,
      font_size_pt: "auto",
      pages: "all",
    });

    fireEvent.change(screen.getByLabelText("Watermark text"), { target: { value: "INTERNAL" } });
    expect(overlayText()[0]?.textContent).toBe("INTERNAL");
    expect(lastCall(onChange).params.text).toBe("INTERNAL");
  });

  it("rejects an empty watermark text", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Watermark text"), { target: { value: "  " } });
    expect(lastCall(onChange).validation.errors.text).toBe("Enter the watermark text.");
    expect(screen.getByRole("alert").textContent).toBe("Enter the watermark text.");
    expect(overlayText).toThrow();
  });

  it("applies opacity and rotation to the preview", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    expect(groupOf(overlayText()[0] as Element).getAttribute("opacity")).toBe("0.3");
    expect(groupOf(overlayText()[0] as Element).getAttribute("transform")).toMatch(/^rotate\(-45 /);

    fireEvent.change(screen.getByRole("slider", { name: /Opacity/ }), { target: { value: "80" } });
    fireEvent.change(screen.getByRole("slider", { name: /Rotation/ }), {
      target: { value: "-30" },
    });
    expect(groupOf(overlayText()[0] as Element).getAttribute("opacity")).toBe("0.8");
    expect(groupOf(overlayText()[0] as Element).getAttribute("transform")).toMatch(/^rotate\(30 /);
    expect(lastCall(onChange).params).toMatchObject({ opacity: 0.8, rotation_deg: -30 });
  });

  it("limits opacity and rotation to the documented ranges", () => {
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={vi.fn()} />);
    const opacity = screen.getByRole("slider", { name: /Opacity/ }) as HTMLInputElement;
    const rotation = screen.getByRole("slider", { name: /Rotation/ }) as HTMLInputElement;
    expect([opacity.min, opacity.max]).toEqual(["10", "100"]);
    expect([rotation.min, rotation.max]).toEqual(["-90", "90"]);
  });

  it("moves the mark with the 3×3 anchor grid", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    const grid = screen
      .getAllByRole("button", { pressed: false })
      .filter((button) =>
        /^(Top|Middle|Bottom|Center)/.test(button.getAttribute("aria-label") ?? ""),
      );
    expect(grid.length + 1).toBe(9);
    expect(screen.getByRole("button", { name: "Center" }).getAttribute("aria-pressed")).toBe(
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: "Top left" }));
    expect(overlayText()[0]?.getAttribute("x")).toBe("36");
    expect(overlayText()[0]?.getAttribute("text-anchor")).toBe("start");
    expect(lastCall(onChange).params.anchor).toBe("top-left");

    fireEvent.click(screen.getByRole("button", { name: "Bottom right" }));
    expect(overlayText()[0]?.getAttribute("x")).toBe(String(595 - 36));
    expect(overlayText()[0]?.getAttribute("text-anchor")).toBe("end");
  });

  it("offsets the mark from typed values and nudge buttons", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    const startX = Number(overlayText()[0]?.getAttribute("x"));
    const startY = Number(overlayText()[0]?.getAttribute("y"));
    fireEvent.change(screen.getByLabelText("Horizontal offset"), { target: { value: "40" } });
    expect(Number(overlayText()[0]?.getAttribute("x"))).toBe(startX + 40);
    expect(lastCall(onChange).params.offset_x_pt).toBe(40);

    fireEvent.click(screen.getByRole("button", { name: "Nudge up" }));
    expect(lastCall(onChange).params.offset_y_pt).toBe(10);
    expect(Number(overlayText()[0]?.getAttribute("y"))).toBe(startY - 10);
    fireEvent.click(screen.getByRole("button", { name: "Nudge left" }));
    expect(lastCall(onChange).params.offset_x_pt).toBe(30);
  });

  it("drags the mark on the page preview", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    const page = screen.getByRole("img", { name: /Page preview/ });
    // 297.5 × 421 px shows the 595 × 842 pt page at half size.
    page.getBoundingClientRect = () => ({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: 297.5,
      bottom: 421,
      width: 297.5,
      height: 421,
      toJSON() {},
    });
    fireEvent.pointerDown(page, { clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(page, { clientX: 110, clientY: 95, pointerId: 1 });
    expect(lastCall(onChange).params).toMatchObject({ offset_x_pt: 20, offset_y_pt: 10 });
    fireEvent.pointerUp(page, { pointerId: 1 });
    fireEvent.pointerMove(page, { clientX: 200, clientY: 200, pointerId: 1 });
    expect(lastCall(onChange).params).toMatchObject({ offset_x_pt: 20, offset_y_pt: 10 });
  });

  it("repeats a tiled watermark and honours the layer", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    expect(overlayText()).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Tiled" }));
    expect(overlayText().length).toBeGreaterThan(3);
    expect(lastCall(onChange).params.layout).toBe("tiled");
    fireEvent.click(screen.getByRole("button", { name: "Diagonal" }));
    expect(overlayText()).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Under content" }));
    expect(lastCall(onChange).params.layer).toBe("under");
  });

  it("takes colour swatches and a manual font size", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Colour #b91c1c" }));
    expect(overlayText()[0]?.getAttribute("fill")).toBe("#b91c1c");
    expect(lastCall(onChange).params.color).toBe("#b91c1c");
    fireEvent.change(screen.getByLabelText("Font size (pt)"), { target: { value: "40" } });
    expect(overlayText()[0]?.getAttribute("font-size")).toBe("40");
    expect(lastCall(onChange).params.font_size_pt).toBe(40);
  });

  it("validates the page selection against the document", () => {
    const onChange = vi.fn();
    render(
      <WatermarkPanel
        tool={tool("pdf.watermark")}
        files={[workspaceFile(1000, 5)]}
        onChange={onChange}
      />,
    );
    fireEvent.change(screen.getByLabelText("Pages"), { target: { value: "2-9" } });
    expect(lastCall(onChange).validation.isValid).toBe(false);
    expect(lastCall(onChange).validation.errors.pages).toMatch(/exceeds/);
    fireEvent.change(screen.getByLabelText("Pages"), { target: { value: "2-4" } });
    expect(lastCall(onChange).validation.isValid).toBe(true);
  });

  it("stamps presets, a COPY stamp and a custom text", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.stamp")} files={[]} onChange={onChange} />);
    expect(overlayText()[0]?.textContent).toBe("APPROVED");
    expect(screen.getByTestId("overlay-border")).toBeTruthy();
    expect(lastCall(onChange).params).toMatchObject({
      preset: "approved",
      anchor: "top-right",
      pages: "1",
      border: true,
    });
    expect(lastCall(onChange).params).not.toHaveProperty("color");

    fireEvent.click(screen.getByRole("button", { name: "DRAFT" }));
    expect(overlayText()[0]?.textContent).toBe("DRAFT");
    expect(lastCall(onChange).params.preset).toBe("draft");
    fireEvent.click(screen.getByRole("button", { name: "CONFIDENTIAL" }));
    expect(overlayText()[0]?.textContent).toBe("CONFIDENTIAL");

    fireEvent.click(screen.getByRole("button", { name: "COPY" }));
    expect(overlayText()[0]?.textContent).toBe("COPY");
    expect(lastCall(onChange).params).toMatchObject({ preset: "custom", text: "COPY" });

    fireEvent.click(screen.getByRole("button", { name: "Custom" }));
    expect(lastCall(onChange).validation.errors.text).toBe("Enter the stamp text.");
    fireEvent.change(screen.getByLabelText("Stamp text"), { target: { value: "PAID" } });
    expect(overlayText()[0]?.textContent).toBe("PAID");
    expect(lastCall(onChange).validation.isValid).toBe(true);
  });

  it("recolours a stamp only when asked, and can date it", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.stamp")} files={[]} onChange={onChange} />);
    expect(overlayText()[0]?.getAttribute("fill")).toBe("#15803d");
    fireEvent.click(screen.getByRole("button", { name: "Colour #1d4ed8" }));
    expect(overlayText()[0]?.getAttribute("fill")).toBe("#1d4ed8");
    expect(lastCall(onChange).params.color).toBe("#1d4ed8");
    fireEvent.click(screen.getByRole("button", { name: "Preset colour" }));
    expect(lastCall(onChange).params).not.toHaveProperty("color");

    fireEvent.click(screen.getByLabelText("Add the date"));
    expect(overlayText()[0]?.textContent).toBe("APPROVED 2026-12-31");
    fireEvent.change(screen.getByLabelText("Date format"), { target: { value: "dmy" } });
    expect(overlayText()[0]?.textContent).toBe("APPROVED 31/12/2026");
    expect(lastCall(onChange).params).toMatchObject({ show_date: true, date_format: "dmy" });
    fireEvent.click(screen.getByLabelText("Border"));
    expect(screen.queryByTestId("overlay-border")).toBeNull();
  });

  it("numbers pages with the document's page count", () => {
    const onChange = vi.fn();
    render(
      <WatermarkPanel
        tool={tool("pdf.page_numbers")}
        files={[workspaceFile(1000, 12)]}
        onChange={onChange}
      />,
    );
    expect(overlayText()[0]?.textContent).toBe("Page 1 of 12");
    expect(lastCall(onChange).params).toMatchObject({
      format: "Page {n} of {total}",
      anchor: "bottom-center",
      start: 1,
    });
    fireEvent.change(screen.getByLabelText("Start at"), { target: { value: "5" } });
    expect(overlayText()[0]?.textContent).toBe("Page 5 of 12");
    fireEvent.click(screen.getByRole("button", { name: "{n} / {total}" }));
    expect(overlayText()[0]?.textContent).toBe("5 / 12");
    fireEvent.change(screen.getByLabelText("Number format"), { target: { value: "" } });
    expect(lastCall(onChange).validation.errors.format).toBe("Enter a number format.");
  });

  it("shows N when the page count is not known yet", () => {
    render(<WatermarkPanel tool={tool("pdf.page_numbers")} files={[]} onChange={vi.fn()} />);
    expect(overlayText()[0]?.textContent).toBe("Page 1 of N");
  });

  it("previews Bates numbers", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.bates")} files={[]} onChange={onChange} />);
    expect(overlayText()[0]?.textContent).toBe("BATES-000001");
    fireEvent.change(screen.getByLabelText("Prefix"), { target: { value: "ACME " } });
    fireEvent.change(screen.getByLabelText("Digits"), { target: { value: "4" } });
    fireEvent.change(screen.getByLabelText("Start at"), { target: { value: "42" } });
    fireEvent.change(screen.getByLabelText("Suffix"), { target: { value: "-C" } });
    expect(overlayText()[0]?.textContent).toBe("ACME 0042-C");
    expect(lastCall(onChange).params).toMatchObject({
      prefix: "ACME ",
      digits: 4,
      start: 42,
      suffix: "-C",
      anchor: "bottom-right",
    });
  });

  it("previews headers and footers and needs one slot filled", () => {
    const onChange = vi.fn();
    render(
      <WatermarkPanel
        tool={tool("pdf.header_footer")}
        files={[workspaceFile(1000, 8)]}
        onChange={onChange}
      />,
    );
    expect(overlayText().map((node) => node.textContent)).toEqual(["Page 1 of 8"]);
    expect(lastCall(onChange).params).toMatchObject({
      footer: { left: null, center: "Page {n} of {total}", right: null },
      header: { left: null, center: null, right: null },
    });
    fireEvent.change(screen.getByLabelText("Header left"), {
      target: { value: "Quarterly report" },
    });
    expect(overlayText().map((node) => node.textContent)).toContain("Quarterly report");
    expect(screen.queryByRole("button", { name: "Top left" })).toBeNull();

    fireEvent.change(screen.getByLabelText("Header left"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("Footer centre"), { target: { value: "" } });
    expect(lastCall(onChange).validation.errors.header).toBe(
      "Fill in at least one header or footer slot.",
    );
  });

  it("previews a QR placeholder at the payload and position", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.qr")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).validation.errors.content).toBe(
      "Enter the text or link the code should hold.",
    );
    const placeholder = screen.getByTestId("qr-placeholder");
    expect(placeholder.textContent).toContain("QR code placeholder");
    const first = placeholder.innerHTML;

    fireEvent.change(screen.getByLabelText("QR payload"), {
      target: { value: "https://example.com/doc/1" },
    });
    expect(screen.getByTestId("qr-placeholder").getAttribute("data-content")).toBe(
      "https://example.com/doc/1",
    );
    expect(screen.getByTestId("qr-placeholder").innerHTML).not.toBe(first);
    expect(lastCall(onChange).validation.isValid).toBe(true);
    expect(lastCall(onChange).params).toMatchObject({
      content: "https://example.com/doc/1",
      ec_level: "M",
      size_pt: 72,
      anchor: "bottom-right",
      background: "white",
    });

    const bottomRight = screen.getByTestId("qr-placeholder").getAttribute("transform");
    fireEvent.click(screen.getByRole("button", { name: "Top left" }));
    expect(screen.getByTestId("qr-placeholder").getAttribute("transform")).toBe("translate(24 24)");
    expect(screen.getByTestId("qr-placeholder").getAttribute("transform")).not.toBe(bottomRight);

    fireEvent.change(screen.getByRole("slider", { name: /Size/ }), { target: { value: "120" } });
    fireEvent.change(screen.getByLabelText("Error correction"), { target: { value: "H" } });
    expect(lastCall(onChange).params).toMatchObject({ size_pt: 120, ec_level: "H" });
  });

  it("rejects a QR payload over the byte limit", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.qr")} files={[]} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("QR payload"), { target: { value: "é".repeat(1500) } });
    expect(lastCall(onChange).validation.errors.content).toMatch(/at most 2953 bytes/);
  });

  it("removes a watermark by exact candidate identifiers", () => {
    const onChange = vi.fn();
    render(<WatermarkPanel tool={tool("pdf.watermark_remove")} files={[]} onChange={onChange} />);
    expect(lastCall(onChange).validation.errors.candidates).toBe(
      "Enter at least one candidate identifier.",
    );
    expect(screen.getByRole("img", { name: /Before/ })).toBeTruthy();
    expect(screen.getByRole("img", { name: /After/ })).toBeTruthy();
    expect(screen.getByText(/Find watermarks/)).toBeTruthy();

    const input = screen.getByLabelText("Candidate identifiers");
    fireEvent.change(input, { target: { value: "wm-1, wm-2 wm-1\nwm-3" } });
    expect(lastCall(onChange)).toEqual({
      params: { candidates: ["wm-1", "wm-2", "wm-3"] },
      validation: { isValid: true, errors: {} },
    });

    fireEvent.change(input, { target: { value: "wm-*" } });
    expect(lastCall(onChange).validation.errors.candidates).toMatch(/Wildcards/);
  });

  it("shows the watermark on the before page only", () => {
    render(<WatermarkPanel tool={tool("pdf.watermark_remove")} files={[]} onChange={vi.fn()} />);
    const before = screen.getByRole("img", { name: /Before/ });
    const after = screen.getByRole("img", { name: /After/ });
    expect(within(before as unknown as HTMLElement).getByTestId("overlay-text").textContent).toBe(
      "CONFIDENTIAL",
    );
    expect(within(after as unknown as HTMLElement).queryByTestId("overlay-text")).toBeNull();
    fireEvent.change(screen.getByLabelText("Watermark text in the preview"), {
      target: { value: "DRAFT" },
    });
    expect(
      within(screen.getByRole("img", { name: /Before/ }) as unknown as HTMLElement).getByTestId(
        "overlay-text",
      ).textContent,
    ).toBe("DRAFT");
  });

  it("does not carry settings across tools", () => {
    const { rerender } = render(
      <WatermarkPanel tool={tool("pdf.watermark")} files={[]} onChange={vi.fn()} />,
    );
    fireEvent.change(screen.getByLabelText("Watermark text"), { target: { value: "SECRET" } });
    rerender(<WatermarkPanel tool={tool("pdf.stamp")} files={[]} onChange={vi.fn()} />);
    expect(overlayText()[0]?.textContent).toBe("APPROVED");
  });
});
