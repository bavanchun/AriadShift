// @vitest-environment jsdom
// The account pages follow the active locale: links carry its prefix, and the page that
// signs a user in sends them back to the locale-less `next` path inside that locale.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import { AccountShell } from "@/components/account/account-shell";
import type { AccountUser, AuthApi } from "@/components/account/auth-api";
import { signInHref } from "@/components/account/session-gate";
import { SignInPanel } from "@/components/account/sign-in-panel";
import { loadMessages } from "@/i18n/messages";
import { renderIntl } from "./intl";

const navigation = vi.hoisted(() => ({ pathname: "/en/account" }));
vi.mock("next/navigation", () => ({ usePathname: () => navigation.pathname }));

const CONFIG = { apiUrl: "http://localhost:3902", authUrl: "http://localhost:3901" };
const USER: AccountUser = {
  id: "user-1",
  name: "Ada",
  email: "ada@example.com",
  twoFactorEnabled: false,
};

function fakeAuth(user: AccountUser | null): AuthApi {
  return {
    getSession: vi.fn().mockResolvedValue(user),
    signInEmail: vi.fn().mockResolvedValue({ twoFactorRequired: false }),
  } as unknown as AuthApi;
}

async function signIn(next: string, locale: string): Promise<string> {
  window.history.replaceState(null, "", `/${locale}/sign-in?next=${encodeURIComponent(next)}`);
  const navigate = vi.fn();
  const ui = (
    <AccountShell config={CONFIG} auth={fakeAuth(null)}>
      <SignInPanel navigate={navigate} />
    </AccountShell>
  );
  if (locale === "en") {
    renderIntl(ui);
  } else {
    // The `vi` catalogue is not written yet; English messages stand in for it.
    render(
      <NextIntlClientProvider locale={locale} messages={loadMessages("en")}>
        {ui}
      </NextIntlClientProvider>,
    );
  }
  fireEvent.change(await screen.findByLabelText("Email"), { target: { value: USER.email } });
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "correct horse" } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  await waitFor(() => expect(navigate).toHaveBeenCalledTimes(1));
  return navigate.mock.calls[0]?.[0] as string;
}

describe("sign-in return path", () => {
  it("returns to the requested page inside the active locale", async () => {
    expect(await signIn("/jobs?status=running#top", "en")).toBe("/en/jobs?status=running#top");
  });

  it("uses the locale of the page that signed the user in", async () => {
    expect(await signIn("/api-keys", "vi")).toBe("/vi/api-keys");
  });

  it.each(["https://evil.example", "//evil.example", "/\\evil.example", "/%2f/evil.example"])(
    "sends %s to the account page instead",
    async (next) => {
      expect(await signIn(next, "en")).toBe("/en/account");
    },
  );
});

describe("locale-aware links", () => {
  it("builds the sign-in link from the locale and a locale-less path", () => {
    expect(signInHref("vi", "/jobs?status=running")).toBe(
      "/vi/sign-in?next=%2Fjobs%3Fstatus%3Drunning",
    );
    expect(signInHref("en", "/account")).toBe("/en/sign-in?next=%2Faccount");
  });

  it("prefixes navigation and the sign-in link at locale vi and marks the current page", async () => {
    navigation.pathname = "/vi/jobs";
    render(
      <NextIntlClientProvider locale="vi" messages={loadMessages("en")}>
        <AccountShell config={CONFIG} auth={fakeAuth(null)}>
          <p>content</p>
        </AccountShell>
      </NextIntlClientProvider>,
    );
    const href = (name: string) => screen.getByRole("link", { name }).getAttribute("href");
    expect(href("Account")).toBe("/vi/account");
    expect(href("API keys")).toBe("/vi/api-keys");
    expect(href("Jobs")).toBe("/vi/jobs");
    expect(screen.getByRole("link", { name: "Jobs" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("link", { name: "Account" }).getAttribute("aria-current")).toBeNull();
    expect(await screen.findByRole("link", { name: "Sign in" })).toBeTruthy();
    expect(href("Sign in")).toBe("/vi/sign-in?next=%2Fjobs");
  });

  it("links the not-configured notice to the converter of the active locale", () => {
    render(
      <NextIntlClientProvider locale="vi" messages={loadMessages("en")}>
        <AccountShell config={null}>
          <p>content</p>
        </AccountShell>
      </NextIntlClientProvider>,
    );
    expect(
      screen.getByRole("link", { name: "convert files in your browser" }).getAttribute("href"),
    ).toBe("/vi/app");
  });
});
