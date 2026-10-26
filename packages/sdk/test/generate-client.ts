// Vitest global setup: generates src/generated/ from the committed OpenAPI document.
import { generate } from "../scripts/generate.mjs";

export default async function setup(): Promise<void> {
  await generate();
}
