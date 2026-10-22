import { fireEvent, render, screen } from "@testing-library/react";
import { ThemeToggle } from "../src/components/theme-toggle";
import { THEME_STORAGE_KEY } from "../src/hooks/use-theme";

function mockSystemTheme(dark: boolean) {
  window.matchMedia = vi.fn().mockReturnValue({
    matches: dark,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
}

describe("ThemeToggle", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.className = "";
  });

  it("follows the system theme until a choice is made", () => {
    mockSystemTheme(true);
    render(<ThemeToggle />);
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(screen.getByRole("button").getAttribute("aria-label")).toContain("system");
  });

  it("cycles light, dark and system and persists the choice", () => {
    mockSystemTheme(false);
    render(<ThemeToggle />);
    const button = screen.getByRole("button");

    fireEvent.click(button);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);

    fireEvent.click(button);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    fireEvent.click(button);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBeNull();
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});
