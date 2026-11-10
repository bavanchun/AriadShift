// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { ParamsForm, validatePageRange, validateParams } from "@/components/tools/params-form";
import { toolCatalog } from "@/converter/tool-catalog";

const catalog = toolCatalog();
const tool = (id: string) => {
  const found = catalog.find((entry) => entry.id === id);
  if (!found) throw new Error(`Missing ${id}`);
  return found;
};
afterEach(cleanup);

describe("schema params form", () => {
  it("resolves real PageRange refs and numeric enum options, then emits validity with params", () => {
    const onChange = vi.fn();
    render(createElement(ParamsForm, { tool: tool("pdf.rotate"), pageCount: 5, onChange }));
    expect(onChange).toHaveBeenLastCalledWith(
      { pages: "all" },
      expect.objectContaining({ isValid: false }),
    );
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "90" } });
    fireEvent.change(screen.getByLabelText("pages"), { target: { value: "1-3, 5, even, odd" } });
    expect(onChange).toHaveBeenLastCalledWith(
      { angle: 90, pages: "1-3, 5, even, odd" },
      { isValid: true, errors: {} },
    );
    fireEvent.change(screen.getByLabelText("pages"), { target: { value: "6" } });
    expect(screen.getByRole("alert").textContent).toContain("page count");
    expect(onChange.mock.lastCall?.[1].isValid).toBe(false);
  });

  it("supports bounded numbers, boolean, paper, color, file slots and string tags", () => {
    const onChange = vi.fn();
    const schema = {
      type: "object",
      properties: {
        dpi: { type: "integer", minimum: 36, maximum: 600 },
        flatten: { type: "boolean", default: false },
        paper: { type: "string", format: "paper-size" },
        color: { type: "string", pattern: "^#[0-9A-Fa-f]{6}$" },
        image: { type: "string", format: "file" },
        tags: { type: "array", items: { type: "string" } },
      },
    };
    render(
      createElement(ParamsForm, {
        tool: { id: "test.params", params_schema: schema, secret_params: [] },
        inputs: [{ id: "slot-2", name: "logo.png" }],
        onChange,
      }),
    );
    fireEvent.change(screen.getByLabelText("dpi"), { target: { value: "601" } });
    expect(onChange.mock.lastCall?.[1].errors.dpi).toBe("Maximum: 600");
    fireEvent.change(screen.getByLabelText("dpi"), { target: { value: "300" } });
    fireEvent.click(screen.getByLabelText("flatten"));
    fireEvent.change(screen.getByLabelText("paper"), { target: { value: "Letter" } });
    fireEvent.change(screen.getByLabelText("color"), { target: { value: "#112233" } });
    fireEvent.change(screen.getByLabelText("image"), { target: { value: "slot-2" } });
    fireEvent.change(screen.getByLabelText("tags"), { target: { value: "one, two" } });
    expect(onChange).toHaveBeenLastCalledWith(
      {
        dpi: 300,
        flatten: true,
        paper: "Letter",
        color: "#112233",
        image: "slot-2",
        tags: ["one", "two"],
      },
      { isValid: true, errors: {} },
    );
  });

  it("keeps secrets masked and transient, and drops them when switching tools", () => {
    const local = vi.spyOn(Storage.prototype, "setItem");
    const replace = vi.spyOn(history, "replaceState");
    const onChange = vi.fn();
    const view = render(createElement(ParamsForm, { tool: tool("pdf.encrypt"), onChange }));
    const input = screen.getByLabelText("owner password") as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: "secret-test-value" } });
    expect(onChange.mock.lastCall?.[0].owner_password).toBe("secret-test-value");
    expect(local).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    view.rerender(createElement(ParamsForm, { tool: tool("pdf.rotate"), onChange }));
    expect(onChange.mock.lastCall?.[0]).not.toHaveProperty("owner_password");
    local.mockRestore();
    replace.mockRestore();
  });

  it("parses object and complex array JSON and blocks malformed values", () => {
    const onChange = vi.fn();
    render(createElement(ParamsForm, { tool: tool("pdf.forms_fill"), onChange }));
    fireEvent.change(screen.getByLabelText("values"), { target: { value: "{" } });
    expect(onChange.mock.lastCall?.[1].isValid).toBe(false);
    fireEvent.change(screen.getByLabelText("values"), { target: { value: '{"name":"An"}' } });
    expect(onChange.mock.lastCall?.[0].values).toEqual({ name: "An" });
    expect(onChange.mock.lastCall?.[1].isValid).toBe(true);
  });

  it("checks integers, array references, required nested fields and enum values", () => {
    const schema = tool("pdf.split").params_schema;
    expect(validateParams(schema, { every: 1.5 }).isValid).toBe(false);
    expect(validateParams(schema, { mode: "unknown" }).isValid).toBe(false);
    expect(validateParams(schema, { ranges: ["8"] }, 5).isValid).toBe(false);
    expect(validateParams(schema, { ranges: ["1-3", "last"] }, 5).isValid).toBe(true);
    expect(validateParams(tool("pdf.crop").params_schema, { box: { left: 0 } }).isValid).toBe(
      false,
    );
  });
});

it.each(["1-3, 5, even, odd", "all", "last", "last-2", "2-", "-3", "last-2-last", "5-1"])(
  "accepts PageRange %s",
  (value) => {
    expect(validatePageRange(value, 5)).toBeNull();
  },
);
it.each(["", "0", "1,,2", "6", "last-5", "1-9", "text", "-", "1.5"])(
  "rejects invalid PageRange %s",
  (value) => {
    expect(validatePageRange(value, 5)).not.toBeNull();
  },
);
