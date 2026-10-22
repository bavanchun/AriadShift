import { brandImage, OG_SIZE } from "@/lib/og-image";

export const alt = "AriadShift: private document conversion in your browser";
export const size = OG_SIZE;
export const contentType = "image/png";
export const dynamic = "force-static";

export default function Image() {
  return brandImage(
    "Convert documents on your own device",
    "Private by default · Measured quality · Ready for AI agents",
  );
}
