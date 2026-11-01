// Browser side of the OAuth sign-in and consent pages (see ../pages.ts).
//
// Better Auth's OAuth provider redirects here with a signed copy of the
// authorization request in the query string. Every POST below sends that
// signed query as `oauth_query`; once a request creates a session, the
// provider resumes the authorization and answers `{ redirect, url }`, which
// is the consent page or the client's redirect URI with the code.

const API = "/api/auth";
const TURNSTILE_SCRIPT_URL =
  "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
const query = new URLSearchParams(window.location.search);
const status = document.getElementById("status");

/**
 * Free-form return or redirect parameters that must never be accepted on the
 * OAuth sign-in/sign-up page. Continuation uses only the provider-signed
 * authorization parameters.
 */
const FORBIDDEN_RETURN_PARAMS = new Set([
  "return_to",
  "returnTo",
  "return_url",
  "returnUrl",
  "redirect_to",
  "redirectTo",
  "callbackURL",
  "callbackUrl",
  "callback_url",
  "next",
  "continue",
  "goto",
  "url",
  "target",
  "destination",
]);

/**
 * Core OAuth 2.1 authorization parameters that must be covered by `ba_param`
 * whenever they appear in the query string.
 */
const CORE_OAUTH_PARAMS = new Set([
  "response_type",
  "client_id",
  "redirect_uri",
  "scope",
  "state",
  "code_challenge",
  "code_challenge_method",
]);

/**
 * Validated, provider-signed authorization parameters from `params`, or
 * `undefined` when the signature metadata is missing or tampered with.
 */
function signedDetailsFor(params) {
  if (!params.has("sig")) return undefined;
  const signed = new Set(params.getAll("ba_param"));
  if (signed.size === 0) return undefined;
  for (const key of params.keys()) {
    if (FORBIDDEN_RETURN_PARAMS.has(key)) return undefined;
    if (CORE_OAUTH_PARAMS.has(key) && !signed.has(key)) return undefined;
  }
  const out = new URLSearchParams();
  for (const [key, value] of params) {
    if (key === "sig" || key === "ba_param" || signed.has(key)) out.append(key, value);
  }
  // Resource indicators and the list of covered parameter names may repeat.
  for (const name of new Set(out.keys())) {
    if (name !== "resource" && name !== "ba_param" && out.getAll(name).length > 1) {
      return undefined;
    }
  }
  if (!out.get("sig") || !out.get("client_id") || !out.get("redirect_uri")) {
    return undefined;
  }
  return out;
}

/** The signed parameters only, as Better Auth's own client sends them. */
function signedQuery() {
  return signedDetailsFor(query)?.toString();
}

/** Display only the covered fields; the provider verifies their signature. */
function signedDetails() {
  return signedDetailsFor(query);
}

/** Native callbacks need their full URI; HTTP callbacks show the origin. */
function redirectText(redirect) {
  try {
    const url = new URL(redirect);
    return url.protocol === "http:" || url.protocol === "https:" ? url.origin : url.href;
  } catch {
    return redirect;
  }
}

function show(message, isError = false) {
  status.textContent = message;
  status.classList.toggle("error", isError);
}

function busy(on) {
  for (const button of document.querySelectorAll("button")) button.disabled = on;
}

async function call(method, path, body, extraHeaders = {}) {
  const init = {
    method,
    credentials: "same-origin",
    headers: { accept: "application/json", ...extraHeaders },
  };
  if (method === "POST") {
    init.headers["content-type"] = "application/json";
    init.body = JSON.stringify({ ...body, oauth_query: signedQuery() });
  }
  const response = await fetch(`${API}${path}`, init);
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(
      data.message || data.error_description || `request failed (${response.status})`,
    );
  }
  return data;
}

/** Follows a `{ redirect, url }` answer; returns false when there is none. */
function follow(data) {
  if (data?.redirect !== true || typeof data.url !== "string") return false;
  const target = new URL(data.url, window.location.href);
  if (target.protocol === "javascript:" || target.protocol === "data:") return false;
  window.location.assign(target.href);
  return true;
}

const PRESERVED_OAUTH_PARAMS = [
  "response_type",
  "client_id",
  "redirect_uri",
  "scope",
  "state",
  "code_challenge",
  "code_challenge_method",
];

function sameValues(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

/**
 * Follows the post-sign-up redirect only when it stays on this origin's
 * `/oauth/consent` page and carries the exact same signed OAuth request
 * parameters (`client_id`, `scope`, `state`, `redirect_uri`, `resource`, and
 * PKCE fields).
 */
function followSignUpConsent(data, requestDetails) {
  if (data?.redirect !== true || typeof data.url !== "string" || !requestDetails) {
    return false;
  }
  const current = new URL(window.location.href);
  const target = new URL(data.url, current.href);
  if (target.origin !== current.origin || target.pathname !== "/oauth/consent") {
    return false;
  }
  const targetDetails = signedDetailsFor(target.searchParams);
  if (!targetDetails) return false;
  for (const key of PRESERVED_OAUTH_PARAMS) {
    if ((requestDetails.get(key) ?? null) !== (targetDetails.get(key) ?? null)) {
      return false;
    }
  }
  if (!sameValues(requestDetails.getAll("resource"), targetDetails.getAll("resource"))) {
    return false;
  }
  window.location.assign(target.href);
  return true;
}

async function run(action) {
  busy(true);
  show("");
  try {
    await action();
  } catch (error) {
    show(error instanceof Error ? error.message : String(error), true);
  } finally {
    busy(false);
  }
}

function signedIn(data) {
  if (follow(data)) {
    show("Signed in. Continuing…");
    return;
  }
  show(
    signedQuery()
      ? "Signed in, but the authorization did not continue. Start again from your application."
      : "Signed in. Start the connection from your application to continue.",
  );
}

// --- WebAuthn helpers ------------------------------------------------------

function fromBase64Url(value) {
  const base64 = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0)).buffer;
}

function toBase64Url(buffer) {
  let binary = "";
  for (const byte of new Uint8Array(buffer)) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function passkeySignIn() {
  const options = await call("GET", "/passkey/generate-authenticate-options");
  const credential = await navigator.credentials.get({
    publicKey: {
      ...options,
      challenge: fromBase64Url(options.challenge),
      allowCredentials: (options.allowCredentials ?? []).map((allowed) => ({
        ...allowed,
        id: fromBase64Url(allowed.id),
      })),
    },
  });
  if (!credential) throw new Error("No passkey was selected.");
  const assertion = credential.response;
  const response = {
    id: credential.id,
    rawId: toBase64Url(credential.rawId),
    type: credential.type,
    authenticatorAttachment: credential.authenticatorAttachment ?? undefined,
    clientExtensionResults: credential.getClientExtensionResults(),
    response: {
      clientDataJSON: toBase64Url(assertion.clientDataJSON),
      authenticatorData: toBase64Url(assertion.authenticatorData),
      signature: toBase64Url(assertion.signature),
      userHandle: assertion.userHandle ? toBase64Url(assertion.userHandle) : undefined,
    },
  };
  signedIn(await call("POST", "/passkey/verify-authentication", { response }));
}

// --- Sign-in page ------------------------------------------------------------

function setUpSignIn() {
  const passwordForm = document.getElementById("password-form");
  const signUpForm = document.getElementById("sign-up-form");
  const twoFactorForm = document.getElementById("two-factor-form");
  const createAccountButton = document.getElementById("create-account");
  const signUpSubmit = document.getElementById("sign-up-submit");
  const turnstileContainer = document.getElementById("turnstile");
  const heading = document.getElementById("heading");
  const lead = document.getElementById("lead");
  const turnstileSiteKey = document.body?.dataset?.turnstileSiteKey?.trim() || "";

  let signingUp = false;
  let widgetId = null;
  let captchaToken = "";
  let captchaFailed = false;
  let turnstileLoading = null;

  function turnstileActive() {
    return Boolean(turnstileSiteKey || window.turnstile);
  }

  function syncSignUpSubmit() {
    if (!signUpSubmit) return;
    if (!signingUp) return;
    if (!signedDetails()) {
      signUpSubmit.disabled = true;
      return;
    }
    if (turnstileActive() && !captchaToken) {
      signUpSubmit.disabled = true;
      return;
    }
    signUpSubmit.disabled = false;
  }

  function removeTurnstileWidget() {
    captchaToken = "";
    if (widgetId !== null && window.turnstile) {
      try {
        window.turnstile.remove(widgetId);
      } catch {
        // Ignore removal errors from already-detached widgets.
      }
    }
    widgetId = null;
  }

  function loadTurnstileScript() {
    if (window.turnstile) return Promise.resolve(window.turnstile);
    if (!turnstileSiteKey) return Promise.resolve(null);
    if (turnstileLoading) return turnstileLoading;
    turnstileLoading = new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = TURNSTILE_SCRIPT_URL;
      script.async = true;
      script.defer = true;
      script.onload = () => resolve(window.turnstile ?? null);
      script.onerror = () => {
        turnstileLoading = null;
        reject(new Error("Bot check could not load. Reload and try again."));
      };
      (document.head ?? document.body).append(script);
    });
    return turnstileLoading;
  }

  async function mountTurnstile() {
    removeTurnstileWidget();
    captchaFailed = false;
    if (!turnstileActive()) {
      if (turnstileContainer) turnstileContainer.hidden = true;
      syncSignUpSubmit();
      return;
    }
    if (turnstileContainer) turnstileContainer.hidden = false;
    syncSignUpSubmit();
    try {
      const api = await loadTurnstileScript();
      if (!signingUp) return;
      if (!api || typeof api.render !== "function") {
        captchaFailed = true;
        show("Bot check could not load. Reload and try again.", true);
        syncSignUpSubmit();
        return;
      }
      widgetId = api.render(turnstileContainer ?? "#turnstile", {
        sitekey: turnstileSiteKey || "1x00000000000000000000AA",
        action: "signup",
        callback: (token) => {
          captchaToken = typeof token === "string" ? token.trim() : "";
          captchaFailed = !captchaToken;
          syncSignUpSubmit();
        },
        "expired-callback": () => {
          captchaToken = "";
          syncSignUpSubmit();
          show("Bot check expired. Complete the check again.", true);
        },
        "error-callback": () => {
          captchaToken = "";
          captchaFailed = true;
          syncSignUpSubmit();
          show("Bot check failed. Reload and try again.", true);
        },
      });
      syncSignUpSubmit();
    } catch (error) {
      captchaToken = "";
      captchaFailed = true;
      syncSignUpSubmit();
      show(error instanceof Error ? error.message : String(error), true);
    }
  }

  function setMode(nextSignUp) {
    signingUp = nextSignUp;
    show("");
    twoFactorForm.hidden = true;
    passwordForm.hidden = signingUp;
    if (signUpForm) signUpForm.hidden = !signingUp;
    if (heading) {
      heading.textContent = signingUp ? "Create your AriadShift account" : "Sign in to AriadShift";
    }
    if (lead) {
      lead.textContent = signingUp
        ? "An application is asking to use your AriadShift account. Create an account to continue."
        : "An application is asking to use your AriadShift account. Sign in to continue.";
    }
    if (createAccountButton) {
      createAccountButton.textContent = signingUp
        ? "Already have an account? Sign in"
        : "Create an account";
    }
    if (signingUp) {
      if (!signedDetails()) {
        show("This authorization link is not valid. Start again from your application.", true);
        if (signUpSubmit) signUpSubmit.disabled = true;
        return;
      }
      void mountTurnstile();
    } else {
      removeTurnstileWidget();
      if (turnstileContainer) turnstileContainer.hidden = true;
    }
  }

  createAccountButton?.addEventListener("click", () => {
    setMode(!signingUp);
  });

  passwordForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = new FormData(passwordForm);
    run(async () => {
      const data = await call("POST", "/sign-in/email", {
        email: String(form.get("email")),
        password: String(form.get("password")),
      });
      if (data.twoFactorRedirect) {
        passwordForm.hidden = true;
        if (signUpForm) signUpForm.hidden = true;
        twoFactorForm.hidden = false;
        twoFactorForm.querySelector("input[name=code]").focus();
        return;
      }
      signedIn(data);
    });
  });

  signUpForm?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = new FormData(signUpForm);
    run(async () => {
      const requestDetails = signedDetails();
      const oauthQuery = signedQuery();
      if (!requestDetails || !oauthQuery) {
        throw new Error("This authorization link is not valid. Start again from your application.");
      }
      if (turnstileActive() && !captchaToken) {
        throw new Error(
          captchaFailed
            ? "Bot check failed. Reload and try again."
            : "Complete the bot check to create an account.",
        );
      }
      const token = captchaToken;
      try {
        const headers = token ? { "x-captcha-response": token } : {};
        const data = await call(
          "POST",
          "/sign-up/email",
          {
            name: String(form.get("name") ?? "").trim(),
            email: String(form.get("email") ?? "").trim(),
            password: String(form.get("password") ?? ""),
          },
          headers,
        );
        if (!followSignUpConsent(data, requestDetails)) {
          throw new Error(
            "Account created, but the authorization could not continue safely. Start again from your application.",
          );
        }
        show("Account created. Continuing…");
      } finally {
        if (signingUp && turnstileActive()) {
          await mountTurnstile();
        }
      }
    }).finally(() => {
      syncSignUpSubmit();
    });
  });

  twoFactorForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = new FormData(twoFactorForm);
    const path = form.get("backup") ? "/two-factor/verify-backup-code" : "/two-factor/verify-totp";
    run(async () => signedIn(await call("POST", path, { code: String(form.get("code")).trim() })));
  });

  document.getElementById("passkey")?.addEventListener("click", () => run(passkeySignIn));

  for (const button of document.querySelectorAll("[data-provider]")) {
    button.addEventListener("click", () =>
      run(async () => {
        // The provider stores the signed query server-side, so the social
        // callback resumes the authorization after the account is linked.
        const data = await call("POST", "/sign-in/social", {
          provider: button.dataset.provider,
          callbackURL: `${window.location.pathname}${window.location.search}`,
        });
        if (!follow(data)) throw new Error("The sign-in provider did not answer.");
      }),
    );
  }
}

// --- Consent page ------------------------------------------------------------

const SCOPE_TEXT = {
  openid: "Confirm who you are",
  profile: "See your name and picture",
  email: "See your email address",
  offline_access: "Stay connected for up to 30 days without signing in again",
};

function addItem(list, text) {
  const item = document.createElement("li");
  item.textContent = text;
  list.append(item);
}

async function setUpConsent() {
  const details = signedDetails();
  if (!details) {
    show("This authorization link is not valid. Start again from your application.", true);
    for (const id of ["allow", "deny"]) document.getElementById(id).disabled = true;
    return;
  }
  const scopes = document.getElementById("scopes");
  addItem(scopes, "Convert documents with your account, its plan and its quotas");
  for (const scope of (details.get("scope") ?? "").split(" ").filter(Boolean)) {
    addItem(scopes, SCOPE_TEXT[scope] ?? scope);
  }
  document.getElementById("resources").textContent =
    details.getAll("resource").join(", ") || "AriadShift";
  const redirect = details.get("redirect_uri");
  if (redirect) document.getElementById("redirect").textContent = redirectText(redirect);

  const answer = (accept) =>
    run(async () => {
      if (!follow(await call("POST", "/oauth2/consent", { accept }))) {
        throw new Error("The authorization could not continue. Start again from your application.");
      }
      show(accept ? "Access allowed. Returning to the application…" : "Access denied.");
    });
  document.getElementById("allow").addEventListener("click", () => answer(true));
  document.getElementById("deny").addEventListener("click", () => answer(false));

  try {
    const session = await call("GET", "/get-session");
    if (session?.user?.email) {
      document.getElementById("account").textContent = `Signed in as ${session.user.email}`;
    }
    const clientId = details.get("client_id");
    if (clientId) {
      const client = await call(
        "GET",
        `/oauth2/public-client?client_id=${encodeURIComponent(clientId)}`,
      );
      if (client.client_name) {
        document.getElementById("client-name").textContent = client.client_name;
      }
    }
  } catch (error) {
    show(error instanceof Error ? error.message : String(error), true);
  }
}

if (document.body.dataset.page === "sign-in") setUpSignIn();
if (document.body.dataset.page === "consent") setUpConsent();
