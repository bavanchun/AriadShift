// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import RecipesHost from "@/components/app/recipes-host";
import { type AriadRecipe, encodeRecipeFragment } from "@/lib/recipe";

afterEach(cleanup);

const recipe: AriadRecipe = {
  version: "ariad-recipe/1",
  steps: [
    { tool: "pdf.rotate", params: { angle: 90, pages: "all" } },
    { tool: "pdf.compress", params: {} },
  ],
};

describe("<RecipesHost />", () => {
  it("holds a #recipe= link behind the confirmation gate and loads it only on confirm", () => {
    const onResolved = vi.fn();
    render(
      <RecipesHost draft={null} fragment={encodeRecipeFragment(recipe)} onResolved={onResolved} />,
    );
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("Load this recipe?")).toBeTruthy();
    expect(within(dialog).getByText("pdf.rotate")).toBeTruthy();
    // Nothing is in the builder until the user agrees.
    expect(document.querySelector('[data-slot="recipe-step"]')).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm & Load" }));
    expect(onResolved).toHaveBeenCalledOnce();
    expect(document.querySelectorAll('[data-slot="recipe-step"]')).toHaveLength(2);
  });

  it("drops a cancelled link", () => {
    const onResolved = vi.fn();
    render(
      <RecipesHost draft={null} fragment={encodeRecipeFragment(recipe)} onResolved={onResolved} />,
    );
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    expect(onResolved).toHaveBeenCalledOnce();
    expect(document.querySelector('[data-slot="recipe-step"]')).toBeNull();
  });

  it("reports a link that is not a valid recipe", () => {
    render(<RecipesHost draft={null} fragment="#recipe=not-base64!" onResolved={vi.fn()} />);
    expect(document.querySelector('[data-slot="recipe-link-problem"]')?.textContent).toMatch(
      /base64url/,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("refuses a link whose recipe carries a literal secret", () => {
    const json = JSON.stringify({
      version: "ariad-recipe/1",
      steps: [{ tool: "pdf.decrypt", params: { password: "hunter2" } }],
    });
    const fragment = `#recipe=${btoa(json).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "")}`;
    render(<RecipesHost draft={null} fragment={fragment} onResolved={vi.fn()} />);
    expect(document.querySelector('[data-slot="recipe-link-problem"]')?.textContent).toMatch(
      /never carry a secret/,
    );
  });

  it("opens the builder with the steps handed over, without any typed secret", () => {
    render(
      <RecipesHost
        draft={{
          nonce: 1,
          steps: [
            { toolId: "pdf.rotate", params: { angle: 90, pages: "all" } },
            { toolId: "pdf.decrypt", params: { password: "hunter2" } },
          ],
        }}
        fragment={null}
        onResolved={vi.fn()}
      />,
    );
    expect(document.querySelectorAll('[data-slot="recipe-step"]')).toHaveLength(2);
    expect(document.body.textContent).not.toContain("hunter2");
    expect(screen.getByText('Secrets replaced with {"$secret":"prompt"}')).toBeTruthy();
  });
});
