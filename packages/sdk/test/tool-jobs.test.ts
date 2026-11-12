import { describe, expect, it } from "vitest";
import { createAriadClient, type Job } from "../src/index";
import { BASE_URL, jobBody, json, mockFetch, UPLOAD_ID } from "./mock-fetch";

describe("document tool jobs", () => {
  it("preserves ordered input identities and the existing job response fields", async () => {
    const uploadIds = [UPLOAD_ID, "00000000-0000-0000-0000-000000000002"];
    const response: Job = {
      ...jobBody("queued"),
      status: "queued",
      profile: "editable",
      pack: "core",
      from: "pdf",
      to: "pdf",
      tool: "pdf.merge",
      upload_ids: uploadIds,
      params: { outline: "keep" },
    };
    const mock = mockFetch(() => json(response, 202));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    expect(
      await client.createJob({ tool: "pdf.merge", uploadIds, params: { outline: "keep" } }),
    ).toEqual(response);
    expect(await mock.requests[0]?.json()).toEqual({
      tool: "pdf.merge",
      upload_ids: uploadIds,
      params: { outline: "keep" },
    });
  });

  it("supports a single upload and input-free tools without inventing a target", async () => {
    const mock = mockFetch(
      () => json(jobBody("queued"), 202),
      () => json(jobBody("queued"), 202),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    await client.createJob({ tool: "pdf.rotate", uploadId: UPLOAD_ID, params: { angle: 90 } });
    await client.createJob({ tool: "ai.recipe_from_text", params: { prompt: "Split pages" } });
    expect(await mock.requests[0]?.json()).toEqual({
      tool: "pdf.rotate",
      upload_id: UPLOAD_ID,
      params: { angle: 90 },
    });
    expect(await mock.requests[1]?.json()).toEqual({
      tool: "ai.recipe_from_text",
      params: { prompt: "Split pages" },
    });
  });
});
