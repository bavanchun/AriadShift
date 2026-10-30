// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AccountProvider } from "@/components/account/account-context";
import { createBetterAuthApi } from "@/components/account/auth-api";
import { SignInPanel } from "@/components/account/sign-in-panel";
import { Turnstile } from "@/components/cloud/turnstile";

const cloud = vi.hoisted(() => ({ turnstileSiteKey: "test-site-key" as string | null }));
vi.mock("@/lib/cloud/config", async (original) => ({
  ...(await original<object>()),
  CLOUD: cloud,
}));

type WidgetOptions = Parameters<NonNullable<Window["turnstile"]>["render"]>[1];
let options: WidgetOptions;
const remove = vi.fn();
const calls: { path: string; headers: Headers }[] = [];

beforeEach(() => {
  cloud.turnstileSiteKey = "test-site-key";
  calls.length = 0;
  window.turnstile = {
    render: vi.fn((_container, value) => {
      options = value;
      return "widget";
    }),
    reset: vi.fn(),
    remove,
  };
});
afterEach(() => {
  cleanup();
  delete window.turnstile;
  vi.restoreAllMocks();
});

function panel() {
  const fetchImpl = (async (input, init) => {
    const path = new URL(String(input)).pathname;
    calls.push({ path, headers: new Headers(init?.headers) });
    return path.endsWith("get-session")
      ? Response.json(null)
      : Response.json({ message: "Try again", code: "REFUSED" }, { status: 403 });
  }) as typeof fetch;
  return render(
    <AccountProvider
      config={{ authUrl: "http://localhost:3901", apiUrl: "http://localhost:3902" }}
      auth={createBetterAuthApi("http://localhost:3901", fetchImpl)}
    >
      <SignInPanel navigate={vi.fn()} />
    </AccountProvider>,
  );
}

async function signupForm() {
  panel();
  fireEvent.click(screen.getByRole("button", { name: "Create one" }));
  await waitFor(() => expect(window.turnstile?.render).toHaveBeenCalled());
  for (const [label, value] of [
    ["Name", "Ada"],
    ["Email", "ada@example.com"],
    ["Password", "correct horse battery staple"],
    ["Confirm password", "correct horse battery staple"],
  ]) {
    fireEvent.change(screen.getByLabelText(label as string), { target: { value } });
  }
  return screen.getByRole("button", { name: "Create account" }) as HTMLButtonElement;
}

describe("signup Turnstile", () => {
  it("uses the signup action and requires a token before submission", async () => {
    const submit = await signupForm();
    expect(options.action).toBe("signup");
    expect(submit.disabled).toBe(true);
    fireEvent.submit(submit.closest("form") as HTMLFormElement);
    await act(async () => {});
    expect(calls.filter((c) => c.path.endsWith("sign-up/email"))).toHaveLength(0);
    expect(screen.getByRole("heading", { name: "Create an account" })).toBe(document.activeElement);
  });

  it("forwards the fresh token, then resets it even when signup fails", async () => {
    const submit = await signupForm();
    const previousWidget = options;
    act(() => options.callback("fresh-proof"));
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);
    await waitFor(() => expect(screen.getByText("Try again")).toBeTruthy());
    const signup = calls.find((c) => c.path.endsWith("sign-up/email"));
    expect(signup?.headers.get("x-captcha-response")).toBe("fresh-proof");
    expect(submit.disabled).toBe(true);
    expect(remove).toHaveBeenCalled();
    expect(window.turnstile?.render).toHaveBeenCalledTimes(2);
    act(() => previousWidget.callback("late-stale-proof"));
    expect(submit.disabled).toBe(true);
  });

  it.each(["expired-callback", "error-callback"] as const)(
    "invalidates a challenge on %s",
    async (callback) => {
      const submit = await signupForm();
      act(() => options.callback("fresh-proof"));
      expect(submit.disabled).toBe(false);
      act(() => options[callback]());
      expect(submit.disabled).toBe(true);
    },
  );

  it("keeps the upload action as the shared widget default", async () => {
    render(<Turnstile siteKey="upload-test-key" resetKey={0} onToken={vi.fn()} />);
    await waitFor(() => expect(options.action).toBe("upload"));
  });

  it("keeps signup available without a configured site key", async () => {
    cloud.turnstileSiteKey = null;
    panel();
    fireEvent.click(screen.getByRole("button", { name: "Create one" }));
    expect(
      (screen.getByRole("button", { name: "Create account" }) as HTMLButtonElement).disabled,
    ).toBe(false);
    expect(screen.queryByTestId("turnstile")).toBeNull();
  });

  it("prevents submission when the required widget script cannot load", async () => {
    delete window.turnstile;
    panel();
    fireEvent.click(screen.getByRole("button", { name: "Create one" }));
    const script = document.querySelector<HTMLScriptElement>(
      'script[src^="https://challenges.cloudflare.com/"]',
    );
    expect(script).toBeTruthy();
    await act(async () => script?.dispatchEvent(new Event("error")));
    expect(screen.getByRole("alert").textContent).toContain("could not be loaded");
    expect(
      (screen.getByRole("button", { name: "Create account" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(calls.some((call) => call.path.endsWith("sign-up/email"))).toBe(false);
  });
});
