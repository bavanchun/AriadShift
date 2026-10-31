// Open Graph image rendered at build time with the brand font and icon.
import { readFileSync } from "node:fs";
import path from "node:path";
import { ImageResponse } from "next/og";

export const OG_SIZE = { width: 1200, height: 630 };

export function brandImage(title: string, subtitle: string) {
  const font = readFileSync(
    path.join(
      process.cwd(),
      "node_modules/@fontsource/plus-jakarta-sans/files/plus-jakarta-sans-latin-700-normal.woff",
    ),
  );
  const iconPng = readFileSync(
    path.join(process.cwd(), "public/brand/ariadshift-app-icon-512.png"),
  );
  const icon = `data:image/png;base64,${iconPng.toString("base64")}`;
  return new ImageResponse(
    <div
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
        padding: 72,
        background: "linear-gradient(135deg, #f6f4ee 0%, #e4ebdf 100%)",
        color: "#232821",
        fontFamily: "Plus Jakarta Sans",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 24 }}>
        <img src={icon} width={96} height={96} alt="" />
        <span style={{ fontSize: 44, fontWeight: 700, letterSpacing: "-0.02em" }}>AriadShift</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        <span style={{ fontSize: 68, fontWeight: 700, letterSpacing: "-0.02em", lineHeight: 1.1 }}>
          {title}
        </span>
        <span style={{ fontSize: 32, color: "#2f5b43" }}>{subtitle}</span>
      </div>
    </div>,
    {
      ...OG_SIZE,
      fonts: [{ name: "Plus Jakarta Sans", data: font, weight: 700, style: "normal" }],
    },
  );
}
