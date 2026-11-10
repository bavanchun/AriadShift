import path from "node:path";
import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";
import { routeHeaders } from "./route-headers.mjs";
import { REDIRECT_STATUS, routeRedirects } from "./route-redirects.mjs";

const repositoryRoot = path.join(import.meta.dirname, "../..");
const staticExport = process.env.ARIADSHIFT_WEB_OUTPUT === "export";

const nextConfig: NextConfig = {
  // `ARIADSHIFT_WEB_OUTPUT=export` writes a static site to out/; otherwise `next start` serves it.
  output: staticExport ? "export" : undefined,
  poweredByHeader: false,
  // Both workspace packages export TypeScript source.
  transpilePackages: ["@ariadshift/sdk", "@ariadshift/ui"],
  images: { unoptimized: true },
  turbopack: { root: repositoryRoot },
  outputFileTracingRoot: repositoryRoot,
  // Static exports cannot send headers or redirects; scripts/write-static-headers.mjs
  // writes out/_headers and out/_redirects.
  ...(staticExport
    ? {}
    : {
        headers: async () => routeHeaders,
        redirects: async () =>
          routeRedirects.map(({ source, destination }) => ({
            source,
            destination,
            statusCode: REDIRECT_STATUS,
          })),
      }),
};

const withNextIntl = createNextIntlPlugin("./src/i18n/request.ts");

export default withNextIntl(nextConfig);
