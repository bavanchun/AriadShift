import { describe, expect, it } from "vitest";
import { createBetterAuthApi } from "@/components/account/auth-api";

describe("account signup challenge requests", () => {
  it("sends the challenge only on signup and keeps existing callers working", async () => {
    const calls: {
      path: string;
      headers: Headers;
      body: unknown;
      credentials?: RequestCredentials;
    }[] = [];
    const fetchImpl = (async (input, init) => {
      calls.push({
        path: new URL(String(input)).pathname,
        headers: new Headers(init?.headers),
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
        credentials: init?.credentials,
      });
      return Response.json({});
    }) as typeof fetch;
    const auth = createBetterAuthApi("http://localhost:3901", fetchImpl);
    await auth.signUpEmail("Ada", "ada@example.com", "password", "signup-proof");
    await auth.signInEmail("ada@example.com", "password");
    await auth.signUpEmail("Ada", "ada@example.com", "password");
    expect(calls[0]?.path).toBe("/api/auth/sign-up/email");
    expect(calls[0]?.headers.get("x-captcha-response")).toBe("signup-proof");
    expect(calls[0]?.body).toEqual({ name: "Ada", email: "ada@example.com", password: "password" });
    expect(calls[0]?.credentials).toBe("include");
    expect(calls[1]?.headers.has("x-captcha-response")).toBe(false);
    expect(calls[2]?.headers.has("x-captcha-response")).toBe(false);
  });
});
