import path from "node:path";
import type { NextConfig } from "next";
import { routeHeaders } from "./route-headers.mjs";

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
  // Security headers and CSP come from route-headers.mjs. Static exports cannot send headers;
  // scripts/write-static-headers.mjs writes them, with per-page script hashes, for the host.
  ...(staticExport ? {} : { headers: async () => routeHeaders }),
};

export default nextConfig;
