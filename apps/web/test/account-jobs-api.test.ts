// `GET /v1/jobs` feature detection: a deployment without the route falls back,
// a listed page is validated, and real failures surface as API errors.
import { AriadApiError } from "@ariadshift/sdk";
import { JOB_PAGE_SIZE, listJobs } from "@/components/account/jobs-api";

type GetResult = { data?: unknown; error?: unknown; response?: Response };

function fakeClient(result: GetResult) {
  const get = vi.fn(async (_options: unknown) => result);
  return { client: { raw: { get } } as unknown as Parameters<typeof listJobs>[0], get };
}

const JOB = {
  id: "job-1",
  status: "succeeded",
  upload_id: "upload-1",
  from: "pdf",
  to: "docx",
  profile: "editable",
  pack: "docling",
  route: [],
  warnings: [],
  attempts: 1,
  created_at: "2026-10-25T12:00:00Z",
  expires_at: "2026-10-25T13:00:00Z",
  filename: "report.pdf",
};

describe("listJobs", () => {
  it("asks for the first page through the SDK client", async () => {
    const { client, get } = fakeClient({
      data: { jobs: [JOB], next_cursor: "next" },
      response: new Response(null, { status: 200 }),
    });
    expect(await listJobs(client)).toEqual({ supported: true, jobs: [JOB], nextCursor: "next" });
    expect(get).toHaveBeenCalledWith(
      expect.objectContaining({ url: "/v1/jobs", query: { limit: JOB_PAGE_SIZE } }),
    );
  });

  it("passes the cursor of a later page and reads the last page", async () => {
    const { client, get } = fakeClient({
      data: { jobs: [], next_cursor: null },
      response: new Response(null, { status: 200 }),
    });
    expect(await listJobs(client, { cursor: "next" })).toEqual({
      supported: true,
      jobs: [],
      nextCursor: null,
    });
    expect(get).toHaveBeenCalledWith(
      expect.objectContaining({ query: { limit: JOB_PAGE_SIZE, cursor: "next" } }),
    );
  });

  it.each([404, 405, 501])("reports a deployment without the route (%i)", async (status) => {
    const { client } = fakeClient({ error: {}, response: new Response(null, { status }) });
    expect(await listJobs(client)).toEqual({ supported: false });
  });

  it("throws the API error of a refused list", async () => {
    const { client } = fakeClient({
      error: { error: { code: "unauthorized", message: "credentials rejected" } },
      response: new Response(null, { status: 401 }),
    });
    const failure = await listJobs(client).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(AriadApiError);
    expect(failure).toMatchObject({ status: 401, code: "unauthorized" });
  });

  it("rejects a body that is not a job list", async () => {
    const { client } = fakeClient({
      data: { jobs: [{ id: 1 }] },
      response: new Response(null, { status: 200 }),
    });
    await expect(listJobs(client)).rejects.toThrow(/unexpected body/);
  });

  it("rethrows a request that got no response", async () => {
    const { client } = fakeClient({ error: new TypeError("Failed to fetch") });
    await expect(listJobs(client)).rejects.toThrow("Failed to fetch");
  });
});
