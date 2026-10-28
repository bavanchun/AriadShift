// @vitest-environment jsdom
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import type { Mock } from "vitest";
import { AccountPage } from "@/components/account/account-page";
import { AccountShell } from "@/components/account/account-shell";
import { ApiKeysPanel } from "@/components/account/api-keys-panel";
import type { AccountUser, AuthApi } from "@/components/account/auth-api";
import { AuthError } from "@/components/account/auth-api";
import { SignInPanel } from "@/components/account/sign-in-panel";
import type { AccountCloudConfig } from "@/lib/cloud/config";

vi.mock("next/navigation", () => ({ usePathname: () => "/account" }));

const CONFIG: AccountCloudConfig = {
  apiUrl: "http://localhost:3902",
  authUrl: "http://localhost:3901",
};

const USER: AccountUser = {
  id: "user-1",
  name: "Ada",
  email: "ada@example.com",
  twoFactorEnabled: false,
};

type FakeAuth = { [K in keyof AuthApi]: Mock<AuthApi[K]> };

function fakeAuth(user: AccountUser | null = USER): FakeAuth {
  return {
    getSession: vi.fn<AuthApi["getSession"]>().mockResolvedValue(user),
    signInEmail: vi.fn<AuthApi["signInEmail"]>().mockResolvedValue({ twoFactorRequired: false }),
    signUpEmail: vi.fn<AuthApi["signUpEmail"]>().mockResolvedValue(),
    signInPasskey: vi.fn<AuthApi["signInPasskey"]>().mockResolvedValue(),
    verifyTotp: vi.fn<AuthApi["verifyTotp"]>().mockResolvedValue(),
    verifyBackupCode: vi.fn<AuthApi["verifyBackupCode"]>().mockResolvedValue(),
    signOut: vi.fn<AuthApi["signOut"]>().mockResolvedValue(),
    updateName: vi.fn<AuthApi["updateName"]>().mockResolvedValue(),
    changePassword: vi.fn<AuthApi["changePassword"]>().mockResolvedValue(),
    listPasskeys: vi
      .fn<AuthApi["listPasskeys"]>()
      .mockResolvedValue([{ id: "pk-1", name: "Work laptop", createdAt: "2026-10-01T09:00:00Z" }]),
    addPasskey: vi.fn<AuthApi["addPasskey"]>().mockResolvedValue(),
    deletePasskey: vi.fn<AuthApi["deletePasskey"]>().mockResolvedValue(),
    enableTwoFactor: vi.fn<AuthApi["enableTwoFactor"]>().mockResolvedValue({
      totpUri: "otpauth://totp/AriadShift:ada?secret=JBSWY3DPEHPK3PXP&issuer=AriadShift",
      backupCodes: ["aaaa-1111", "bbbb-2222"],
    }),
    disableTwoFactor: vi.fn<AuthApi["disableTwoFactor"]>().mockResolvedValue(),
    regenerateBackupCodes: vi
      .fn<AuthApi["regenerateBackupCodes"]>()
      .mockResolvedValue(["cccc-3333"]),
    deleteAccount: vi.fn<AuthApi["deleteAccount"]>().mockResolvedValue(),
    listApiKeys: vi.fn<AuthApi["listApiKeys"]>().mockResolvedValue([
      {
        id: "key-1",
        name: "CI",
        start: "ashift_ab",
        createdAt: "2026-10-01T09:00:00Z",
        expiresAt: null,
        enabled: true,
      },
    ]),
    createApiKey: vi.fn<AuthApi["createApiKey"]>().mockResolvedValue({
      id: "key-2",
      name: "Deploy",
      start: "ashift_cd",
      createdAt: "2026-10-25T09:00:00Z",
      expiresAt: "2027-01-23T09:00:00Z",
      enabled: true,
      key: "ashift_cdSECRETVALUE",
    }),
    deleteApiKey: vi.fn<AuthApi["deleteApiKey"]>().mockResolvedValue(),
    apiToken: vi.fn<AuthApi["apiToken"]>().mockResolvedValue("jwt"),
  };
}

function renderInShell(ui: ReactNode, auth: FakeAuth) {
  return render(
    <AccountShell config={CONFIG} auth={auth}>
      {ui}
    </AccountShell>,
  );
}

function fill(label: string | RegExp, value: string, container: HTMLElement = document.body) {
  fireEvent.change(within(container).getByLabelText(label), { target: { value } });
}

beforeEach(() => {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
  window.history.replaceState(null, "", "/");
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("account shell", () => {
  it("shows the not-configured state and calls nothing without cloud origins", () => {
    const auth = fakeAuth();
    render(
      <AccountShell config={null} auth={auth}>
        <p>account content</p>
      </AccountShell>,
    );
    expect(screen.getByText("Cloud is not configured")).toBeTruthy();
    expect(screen.queryByText("account content")).toBeNull();
    expect(screen.getByRole("link", { name: "convert files in your browser" })).toBeTruthy();
    expect(auth.getSession).not.toHaveBeenCalled();
  });

  it("marks the current section and names the server", async () => {
    renderInShell(<p>content</p>, fakeAuth());
    const navigation = screen.getByRole("navigation", { name: "Account" });
    expect(
      within(navigation).getByRole("link", { name: "Account" }).getAttribute("aria-current"),
    ).toBe("page");
    expect(
      within(navigation).getByRole("link", { name: "Jobs" }).getAttribute("aria-current"),
    ).toBeNull();
    expect(screen.getByText("Server: localhost:3902")).toBeTruthy();
    expect(await screen.findByText("ada@example.com")).toBeTruthy();
  });

  it("asks a signed-out visitor to sign in and come back", async () => {
    renderInShell(<AccountPage />, fakeAuth(null));
    const links = await screen.findAllByRole("link", { name: "Sign in" });
    expect(links.map((link) => link.getAttribute("href"))).toContain("/sign-in?next=%2Faccount");
  });

  it("offers a retry when the auth service is unreachable", async () => {
    const auth = fakeAuth();
    auth.getSession.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    renderInShell(<AccountPage />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Profile" })).toBeTruthy();
  });
});

describe("sign in", () => {
  it("finishes a two-factor sign-in and returns to the requested page", async () => {
    window.history.replaceState(null, "", "/sign-in?next=%2Fjobs");
    const auth = fakeAuth(null);
    auth.signInEmail.mockResolvedValue({ twoFactorRequired: true });
    const navigate = vi.fn();
    renderInShell(<SignInPanel navigate={navigate} />, auth);

    fill("Email", "ada@example.com");
    fill("Password", "correct horse");
    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));

    const heading = await screen.findByRole("heading", { name: "Two-factor check" });
    await waitFor(() => expect(document.activeElement).toBe(heading));
    expect(auth.signInEmail).toHaveBeenCalledWith("ada@example.com", "correct horse");

    fill("Authentication code", "123 456");
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith("/jobs"));
    expect(auth.verifyTotp).toHaveBeenCalledWith("123456", false);
  });

  it("accepts a backup code instead", async () => {
    const auth = fakeAuth(null);
    auth.signInEmail.mockResolvedValue({ twoFactorRequired: true });
    const navigate = vi.fn();
    renderInShell(<SignInPanel navigate={navigate} />, auth);
    fill("Email", "ada@example.com");
    fill("Password", "correct horse");
    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
    fireEvent.click(await screen.findByRole("button", { name: "Use a backup code instead" }));
    fill("Backup code", "aaaa-1111");
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith("/account"));
    expect(auth.verifyBackupCode).toHaveBeenCalledWith("aaaa-1111");
  });

  it("shows the service's refusal", async () => {
    const auth = fakeAuth(null);
    auth.signInEmail.mockRejectedValue(new AuthError("Invalid email or password", 401));
    renderInShell(<SignInPanel navigate={vi.fn()} />, auth);
    fill("Email", "ada@example.com");
    fill("Password", "wrong password");
    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Invalid email or password");
  });

  it("checks the confirmation before creating an account", async () => {
    const auth = fakeAuth(null);
    const navigate = vi.fn();
    renderInShell(<SignInPanel navigate={navigate} />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Create one" }));
    fill("Name", "Ada");
    fill("Email", "ada@example.com");
    fill(/^Password/, "correct horse");
    fill("Confirm password", "correct horsf");
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByText("The passwords do not match.")).toBeTruthy();
    expect(auth.signUpEmail).not.toHaveBeenCalled();

    fill("Confirm password", "correct horse");
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith("/account"));
    expect(auth.signUpEmail).toHaveBeenCalledWith("Ada", "ada@example.com", "correct horse");
  });
});

describe("account page", () => {
  it("saves the name", async () => {
    const auth = fakeAuth();
    renderInShell(<AccountPage />, auth);
    const section = await screen.findByRole("region", { name: "Profile" });
    fill("Name", "Ada Lovelace", section);
    fireEvent.click(within(section).getByRole("button", { name: "Save name" }));
    expect(await within(section).findByText("Name saved.")).toBeTruthy();
    expect(auth.updateName).toHaveBeenCalledWith("Ada Lovelace");
  });

  it("changes the password and signs out other sessions by default", async () => {
    const auth = fakeAuth();
    renderInShell(<AccountPage />, auth);
    const section = await screen.findByRole("region", { name: "Password" });
    fill("Current password", "old password", section);
    fill("New password", "new password", section);
    fill("Confirm new password", "new passworx", section);
    fireEvent.click(within(section).getByRole("button", { name: "Change password" }));
    expect(await within(section).findByText("The new passwords do not match.")).toBeTruthy();
    expect(auth.changePassword).not.toHaveBeenCalled();

    fill("Confirm new password", "new password", section);
    fireEvent.click(within(section).getByRole("button", { name: "Change password" }));
    expect(
      await within(section).findByText("Password changed. Other sessions were signed out."),
    ).toBeTruthy();
    expect(auth.changePassword).toHaveBeenCalledWith("old password", "new password", true);
  });

  it("removes a passkey after confirmation", async () => {
    const auth = fakeAuth();
    renderInShell(<AccountPage />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Remove passkey Work laptop" }));
    const dialog = screen.getByRole("alertdialog", { name: "Remove Work laptop?" });
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Cancel" }));
    auth.listPasskeys.mockResolvedValue([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove passkey" }));
    const section = screen.getByRole("region", { name: "Passkeys" });
    expect(await within(section).findByText("Work laptop removed.")).toBeTruthy();
    expect(auth.deletePasskey).toHaveBeenCalledWith("pk-1");
    expect(screen.getByText("No passkeys yet.")).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("adds a passkey under the given name", async () => {
    const auth = fakeAuth();
    renderInShell(<AccountPage />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Add a passkey" }));
    const dialog = screen.getByRole("dialog", { name: "Add a passkey" });
    expect(document.activeElement).toBe(within(dialog).getByLabelText("Passkey name"));
    fill("Passkey name", "Phone", dialog);
    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
    const section = screen.getByRole("region", { name: "Passkeys" });
    expect(await within(section).findByText("Passkey added.")).toBeTruthy();
    expect(auth.addPasskey).toHaveBeenCalledWith("Phone");
  });

  it("turns on two-factor authentication with a verified code", async () => {
    const auth = fakeAuth();
    renderInShell(<AccountPage />, auth);
    const section = await screen.findByRole("region", { name: "Two-factor authentication" });
    fireEvent.click(
      within(section).getByRole("button", { name: "Turn on two-factor authentication" }),
    );

    const passwordDialog = screen.getByRole("dialog", {
      name: "Turn on two-factor authentication",
    });
    fill("Password", "secret password", passwordDialog);
    fireEvent.click(within(passwordDialog).getByRole("button", { name: "Continue" }));

    const setup = await screen.findByRole("dialog", { name: "Set up your authenticator" });
    expect(auth.enableTwoFactor).toHaveBeenCalledWith("secret password");
    expect(within(setup).getByText("JBSW Y3DP EHPK 3PXP")).toBeTruthy();
    expect(within(setup).getByText("aaaa-1111")).toBeTruthy();
    expect(document.activeElement).toBe(within(setup).getByLabelText("Authentication code"));

    auth.getSession.mockResolvedValue({ ...USER, twoFactorEnabled: true });
    fill("Authentication code", "654321", setup);
    fireEvent.click(within(setup).getByRole("button", { name: "Turn on" }));

    expect(await within(section).findByText("Two-factor authentication is on.")).toBeTruthy();
    expect(auth.verifyTotp).toHaveBeenCalledWith("654321");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      within(section).getByRole("button", { name: "Turn off two-factor authentication" }),
    ).toBeTruthy();
    // The opening button is gone, so focus lands on the section heading.
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(section).getByRole("heading", { name: "Two-factor authentication" }),
      ),
    );
  });

  it("deletes the account after the password is confirmed", async () => {
    const auth = fakeAuth();
    const navigate = vi.fn();
    renderInShell(<AccountPage navigate={navigate} />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Delete account" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete your account?" });
    fill("Password", "secret password", dialog);
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete account" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith("/"));
    expect(auth.deleteAccount).toHaveBeenCalledWith("secret password");
  });

  it("signs out", async () => {
    const auth = fakeAuth();
    const navigate = vi.fn();
    renderInShell(<AccountPage navigate={navigate} />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith("/sign-in"));
    expect(auth.signOut).toHaveBeenCalled();
  });

  it("does not submit account forms when Cancel is clicked", async () => {
    const auth = fakeAuth();
    renderInShell(<AccountPage />, auth);

    // 1. Passkey creation form
    fireEvent.click(await screen.findByRole("button", { name: "Add a passkey" }));
    const passkeyDialog = screen.getByRole("dialog", { name: "Add a passkey" });
    fill("Passkey name", "Phone", passkeyDialog);
    const passkeyCancel = within(passkeyDialog).getByRole("button", {
      name: "Cancel",
    }) as HTMLButtonElement;
    expect(passkeyCancel.type).toBe("button");
    fireEvent.click(passkeyCancel);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(auth.addPasskey).not.toHaveBeenCalled();

    // 2. Two-factor password form
    const twoFactorSection = screen.getByRole("region", { name: "Two-factor authentication" });
    fireEvent.click(
      within(twoFactorSection).getByRole("button", { name: "Turn on two-factor authentication" }),
    );
    const twoFactorPasswordDialog = screen.getByRole("dialog", {
      name: "Turn on two-factor authentication",
    });
    fill("Password", "secret password", twoFactorPasswordDialog);
    const twoFactorPasswordCancel = within(twoFactorPasswordDialog).getByRole("button", {
      name: "Cancel",
    }) as HTMLButtonElement;
    expect(twoFactorPasswordCancel.type).toBe("button");
    fireEvent.click(twoFactorPasswordCancel);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(auth.enableTwoFactor).not.toHaveBeenCalled();

    // 3. Two-factor verification form
    fireEvent.click(
      within(twoFactorSection).getByRole("button", { name: "Turn on two-factor authentication" }),
    );
    const passwordDialogAgain = screen.getByRole("dialog", {
      name: "Turn on two-factor authentication",
    });
    fill("Password", "secret password", passwordDialogAgain);
    fireEvent.click(within(passwordDialogAgain).getByRole("button", { name: "Continue" }));
    const verifyDialog = await screen.findByRole("dialog", { name: "Set up your authenticator" });
    fill("Authentication code", "654321", verifyDialog);
    const verifyCancel = within(verifyDialog).getByRole("button", {
      name: "Cancel",
    }) as HTMLButtonElement;
    expect(verifyCancel.type).toBe("button");
    fireEvent.click(verifyCancel);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(auth.verifyTotp).not.toHaveBeenCalled();

    // 4. Delete account form
    fireEvent.click(screen.getByRole("button", { name: "Delete account" }));
    const deleteDialog = screen.getByRole("alertdialog", { name: "Delete your account?" });
    fill("Password", "secret password", deleteDialog);
    const deleteCancel = within(deleteDialog).getByRole("button", {
      name: "Cancel",
    }) as HTMLButtonElement;
    expect(deleteCancel.type).toBe("button");
    fireEvent.click(deleteCancel);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(auth.deleteAccount).not.toHaveBeenCalled();
  });
});

describe("API keys", () => {
  it("shows a new key once and returns focus to the create button", async () => {
    const auth = fakeAuth();
    renderInShell(<ApiKeysPanel />, auth);
    expect(await screen.findByText("CI")).toBeTruthy();

    const create = screen.getByRole("button", { name: "Create API key" });
    create.focus();
    fireEvent.click(create);
    const form = screen.getByRole("dialog", { name: "Create an API key" });
    expect(document.activeElement).toBe(within(form).getByLabelText("Name"));
    fill("Name", "Deploy", form);
    fireEvent.change(within(form).getByLabelText("Expires after"), { target: { value: "90" } });
    fireEvent.click(within(form).getByRole("button", { name: "Create key" }));

    const secret = await screen.findByRole("dialog", { name: "Your new API key" });
    const field = within(secret).getByLabelText("API key") as HTMLInputElement;
    expect(field.value).toBe("ashift_cdSECRETVALUE");
    expect(field.readOnly).toBe(true);
    expect(document.activeElement).toBe(field);
    expect(auth.createApiKey).toHaveBeenCalledWith("Deploy", 90);

    fireEvent.click(within(secret).getByRole("button", { name: "Copy key" }));
    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith("ashift_cdSECRETVALUE"),
    );

    fireEvent.click(within(secret).getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByDisplayValue("ashift_cdSECRETVALUE")).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Create API key" }));
  });

  it("creates a key without expiry", async () => {
    const auth = fakeAuth();
    renderInShell(<ApiKeysPanel />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Create API key" }));
    const form = screen.getByRole("dialog", { name: "Create an API key" });
    fill("Name", "Forever", form);
    fireEvent.change(within(form).getByLabelText("Expires after"), {
      target: { value: "never" },
    });
    fireEvent.click(within(form).getByRole("button", { name: "Create key" }));
    await screen.findByRole("dialog", { name: "Your new API key" });
    expect(auth.createApiKey).toHaveBeenCalledWith("Forever", null);
  });

  it("closes on Escape and restores focus", async () => {
    renderInShell(<ApiKeysPanel />, fakeAuth());
    const create = await screen.findByRole("button", { name: "Create API key" });
    create.focus();
    fireEvent.click(create);
    const dialog = screen.getByRole("dialog", { name: "Create an API key" });
    act(() => {
      dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(create);
  });

  it("revokes a key after confirmation", async () => {
    const auth = fakeAuth();
    renderInShell(<ApiKeysPanel />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Revoke API key CI" }));
    const dialog = screen.getByRole("alertdialog", { name: "Revoke CI?" });
    auth.listApiKeys.mockResolvedValue([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke key" }));
    const section = screen.getByRole("region", { name: "Your keys" });
    expect(
      await within(section).findByText(
        "API key CI revoked. Requests that use it are refused from now on.",
      ),
    ).toBeTruthy();
    expect(auth.deleteApiKey).toHaveBeenCalledWith("key-1");
    expect(await screen.findByText("No API keys yet.")).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("heading", { name: "Your keys" }));
  });

  it("keeps the dialog open with the error when creation fails", async () => {
    const auth = fakeAuth();
    auth.createApiKey.mockRejectedValue(new AuthError("Too many keys", 400));
    renderInShell(<ApiKeysPanel />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Create API key" }));
    const form = screen.getByRole("dialog", { name: "Create an API key" });
    fill("Name", "Deploy", form);
    fireEvent.click(within(form).getByRole("button", { name: "Create key" }));
    expect(await within(form).findByText("Too many keys")).toBeTruthy();
    expect(within(form).getByLabelText("Name").getAttribute("aria-invalid")).toBe("true");
  });

  it("does not submit the API key creation form when Cancel is clicked", async () => {
    const auth = fakeAuth();
    renderInShell(<ApiKeysPanel />, auth);
    fireEvent.click(await screen.findByRole("button", { name: "Create API key" }));
    const form = screen.getByRole("dialog", { name: "Create an API key" });
    fill("Name", "Deploy", form);
    const cancel = within(form).getByRole("button", { name: "Cancel" }) as HTMLButtonElement;
    expect(cancel.type).toBe("button");
    fireEvent.click(cancel);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(auth.createApiKey).not.toHaveBeenCalled();
  });
});
