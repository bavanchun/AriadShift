// Browser side of the OAuth sign-in and consent pages (see ../pages.ts).
//
// Better Auth's OAuth provider redirects here with a signed copy of the
// authorization request in the query string. Every POST below sends that
// signed query as `oauth_query`; once a request creates a session, the
// provider resumes the authorization and answers `{ redirect, url }`, which
// is the consent page or the client's redirect URI with the code.

const API = "/api/auth";
const query = new URLSearchParams(window.location.search);
const status = document.getElementById("status");

/** The signed parameters only, as Better Auth's own client sends them. */
function signedQuery() {
  if (!query.has("sig")) return undefined;
  const signed = new Set(query.getAll("ba_param"));
  if (signed.size === 0) return undefined;
  const out = new URLSearchParams();
  for (const [key, value] of query) {
    if (key === "sig" || key === "ba_param" || signed.has(key)) out.append(key, value);
  }
  return out.toString();
}

/** Display only the covered fields; the provider verifies their signature. */
function signedDetails() {
  const raw = signedQuery();
  if (!raw) return undefined;
  const details = new URLSearchParams(raw);
  // Resource indicators and the list of covered parameter names may repeat.
  for (const name of new Set(details.keys())) {
    if (name !== "resource" && name !== "ba_param" && details.getAll(name).length > 1) {
      return undefined;
    }
  }
  return details.get("sig") ? details : undefined;
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

async function call(method, path, body) {
  const init = {
    method,
    credentials: "same-origin",
    headers: { accept: "application/json" },
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
  if (!data || data.redirect !== true || typeof data.url !== "string") return false;
  const target = new URL(data.url, window.location.href);
  if (target.protocol === "javascript:" || target.protocol === "data:") return false;
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
  const twoFactorForm = document.getElementById("two-factor-form");

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
        twoFactorForm.hidden = false;
        twoFactorForm.querySelector("input[name=code]").focus();
        return;
      }
      signedIn(data);
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
