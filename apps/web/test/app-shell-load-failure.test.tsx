// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import { loadMessages, scopeMessages } from "@/i18n/messages";

const mocks = vi.hoisted(() => ({ failTools: true }));

vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams("") }));
vi.mock("@/components/converter/converter-app", () => ({
  ConverterApp: () => <div data-testid="converter-stub" />,
}));
// The first import of the hub fails, like a chunk that could not be fetched; the next one works.
vi.mock("@/components/app/tools-host", () => {
  if (mocks.failTools) {
    mocks.failTools = false;
    throw new Error("chunk failed");
  }
  return { default: () => <div data-testid="tools-stub" /> };
});

import { AppShell } from "@/components/app/app-shell";

afterEach(cleanup);

it("says so and retries when the tool hub cannot be loaded", async () => {
  const messages = scopeMessages(loadMessages("en"), ["common", "web.app"]);
  render(
    <NextIntlClientProvider locale="en" messages={messages} timeZone="UTC">
      <AppShell />
    </NextIntlClientProvider>,
  );
  fireEvent.click(screen.getByRole("tab", { name: "Tools" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "The tools could not be loaded.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByTestId("tools-stub")).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
});
