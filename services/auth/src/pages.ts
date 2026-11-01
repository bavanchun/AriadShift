// Sign-in and consent pages for OAuth authorization. The web app has no
// account screens, so this service renders the two pages Better Auth's OAuth
// provider redirects to. They are static HTML; `pages/oauth.js` drives the
// existing session endpoints (email and password with two-factor, sign-up,
// passkeys, GitHub and Google) and the provider's consent endpoint from the
// browser.

import { readFileSync } from "node:fs";
import type { AuthServiceConfig } from "./config.ts";

/** Where the page script and stylesheet are served. */
export const ASSET_PATH = "/oauth/assets";

export type SocialProvider = "github" | "google";

export interface PageOptions {
  /** Social sign-in providers with credentials configured. */
  socialProviders: SocialProvider[];
  /** Whether passkeys work on this service's origin. */
  passkeys: boolean;
  /** Whether account creation is enabled (`AUTH_ALLOW_SIGN_UP=true`). */
  allowSignUp?: boolean;
  /** Cloudflare Turnstile site key for the signup bot check, when configured. */
  turnstileSiteKey?: string;
}

const PROVIDER_LABELS: Record<SocialProvider, string> = {
  github: "Continue with GitHub",
  google: "Continue with Google",
};

/**
 * Passkeys belong to a relying-party id. They can be used on this service's
 * pages only when that id is its host or a parent domain of it.
 */
export function pageOptions(
  config: Pick<AuthServiceConfig, "baseUrl" | "passkeyRpId" | "github" | "google"> &
    Partial<Pick<AuthServiceConfig, "allowSignUp" | "turnstileSiteKey">>,
): PageOptions {
  const host = new URL(config.baseUrl).hostname;
  const rpId = config.passkeyRpId;
  const allowSignUp = Boolean(config.allowSignUp);
  const turnstileSiteKey = allowSignUp ? config.turnstileSiteKey?.trim() : undefined;
  return {
    socialProviders: [
      ...(config.github ? (["github"] as const) : []),
      ...(config.google ? (["google"] as const) : []),
    ],
    passkeys: host === rpId || host.endsWith(`.${rpId}`),
    allowSignUp,
    ...(turnstileSiteKey ? { turnstileSiteKey } : {}),
  };
}

/**
 * Headers for both pages. The script and stylesheet come from this origin
 * only, framing is refused so the consent button cannot be clickjacked, and
 * the signed authorization query in the URL is never sent as a referrer.
 */
export const PAGE_HEADERS: Readonly<Record<string, string>> = {
  "Content-Security-Policy":
    "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; " +
    "img-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
  "X-Frame-Options": "DENY",
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
  "Cache-Control": "no-store",
};

/** Page headers, allowing Cloudflare Turnstile only when sign-up and a site key are configured. */
export function pageHeaders(options?: PageOptions): Readonly<Record<string, string>> {
  if (!options?.allowSignUp || !options?.turnstileSiteKey) return PAGE_HEADERS;
  return {
    ...PAGE_HEADERS,
    "Content-Security-Policy":
      "default-src 'none'; script-src 'self' https://challenges.cloudflare.com; " +
      "style-src 'self'; connect-src 'self'; img-src 'self'; " +
      "frame-src https://challenges.cloudflare.com; form-action 'self'; " +
      "frame-ancestors 'none'; base-uri 'none'",
  };
}

function escapeAttribute(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function readAsset(name: string): string {
  return readFileSync(new URL(`./pages/${name}`, import.meta.url), "utf8");
}

/** Static assets, read once at start. */
export const ASSETS: Readonly<Record<string, { body: string; type: string }>> = {
  "oauth.js": { body: readAsset("oauth.js"), type: "text/javascript; charset=utf-8" },
  "oauth.css": { body: readAsset("oauth.css"), type: "text/css; charset=utf-8" },
};

function layout(title: string, page: string, attributes: string, main: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>${title} · AriadShift</title>
<link rel="stylesheet" href="${ASSET_PATH}/oauth.css">
<script type="module" src="${ASSET_PATH}/oauth.js"></script>
</head>
<body data-page="${page}"${attributes}>
<main>
${main}
<p id="status" class="status" role="status" aria-live="polite"></p>
<noscript><p class="status error">This page needs JavaScript.</p></noscript>
</main>
</body>
</html>
`;
}

export function signInPage(options: PageOptions): string {
  const allowSignUp = Boolean(options.allowSignUp);
  const social = options.socialProviders
    .map(
      (provider) =>
        `<button type="button" class="secondary" data-provider="${provider}">${PROVIDER_LABELS[provider]}</button>`,
    )
    .join("\n");
  const passkey = options.passkeys
    ? `<button type="button" class="secondary" id="passkey">Sign in with a passkey</button>`
    : "";
  const createAccount = allowSignUp
    ? `<button type="button" class="secondary" id="create-account">Create an account</button>`
    : "";
  const alternativeButtons = [createAccount, passkey, social].filter(Boolean).join("\n");
  const alternatives = alternativeButtons
    ? `<div class="alternatives">\n${alternativeButtons}\n</div>`
    : "";
  const attributes =
    allowSignUp && options.turnstileSiteKey
      ? ` data-turnstile-site-key="${escapeAttribute(options.turnstileSiteKey)}"`
      : "";
  const signUpForm = allowSignUp
    ? `<form id="sign-up-form" method="post" action="/oauth/sign-in" hidden>
<label>Name <input name="name" type="text" autocomplete="name" required></label>
<label>Email <input name="email" type="email" autocomplete="email" required></label>
<label>Password <input name="password" type="password" autocomplete="new-password" minlength="12" required></label>
<div id="turnstile" hidden></div>
<button type="submit" id="sign-up-submit">Create account</button>
</form>\n`
    : "";
  return layout(
    "Sign in",
    "sign-in",
    attributes,
    `<h1 id="heading">Sign in to AriadShift</h1>
<p id="lead" class="lead">An application is asking to use your AriadShift account. Sign in to continue.</p>
<form id="password-form" method="post" action="/oauth/sign-in">
<label>Email <input name="email" type="email" autocomplete="username webauthn" required></label>
<label>Password <input name="password" type="password" autocomplete="current-password" required></label>
<button type="submit">Sign in</button>
</form>
${signUpForm}<form id="two-factor-form" method="post" action="/oauth/sign-in" hidden>
<p>Enter the code from your authenticator app, or one of your backup codes.</p>
<label>Code <input name="code" autocomplete="one-time-code" required></label>
<label class="check"><input name="backup" type="checkbox"> This is a backup code</label>
<button type="submit">Verify</button>
</form>${alternatives ? `\n${alternatives}` : ""}`,
  );
}

export function consentPage(): string {
  return layout(
    "Allow access",
    "consent",
    "",
    `<h1>Allow access?</h1>
<p class="lead"><strong id="client-name">An application</strong> wants to use your AriadShift account.</p>
<p id="account" class="muted"></p>
<h2>It will be able to</h2>
<ul id="scopes"></ul>
<dl>
<dt>Service</dt><dd id="resources"></dd>
<dt>Returns you to</dt><dd id="redirect"></dd>
</dl>
<p class="muted">Allow only applications you trust. The name above is chosen by the application itself; check where it returns you.</p>
<div class="actions">
<button type="button" class="secondary" id="deny">Deny</button>
<button type="button" id="allow">Allow</button>
</div>`,
  );
}
