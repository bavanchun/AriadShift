import { execFileSync } from "node:child_process";
import { shellQuote, toolHandoff } from "@/converter/handoff";
import { toolCatalog } from "@/converter/tool-catalog";

const catalog = toolCatalog();
const tool = (id: string) => {
  const found = catalog.find((entry) => entry.id === id);
  if (!found) throw new Error(`Missing ${id}`);
  return found;
};

describe("honest runtime hand-off", () => {
  it("prefers browser and only selects cloud with both runtime support and a configured URL", () => {
    expect(
      toolHandoff(tool("pdf.merge"), {}, [], { cloudApiUrl: "https://api.test" }).executionMode,
    ).toBe("browser");
    const cloud = {
      ...tool("pdf.encrypt"),
      runtimes: ["cloud", "local"] as (typeof catalog)[number]["runtimes"],
    };
    expect(toolHandoff(cloud, {}, []).executionMode).toBe("desktop");
    expect(toolHandoff(cloud, {}, [], { cloudApiUrl: "https://api.test" }).executionMode).toBe(
      "cloud",
    );
    expect(
      toolHandoff(tool("pdf.encrypt"), {}, [], { cloudApiUrl: "https://api.test" }).executionMode,
    ).toBe("desktop");
  });

  it("redacts secret params in display/copy commands and keeps URLs route-only even on reveal", () => {
    const params = {
      owner_password: "private value",
      user_password: "hidden",
      algorithm: "aes-256",
    };
    const result = toolHandoff(tool("pdf.encrypt"), params, ["private-file.pdf"], { locale: "vi" });
    expect(result.cliCommand).not.toContain("private value");
    expect(result.cliCommand).not.toContain("hidden");
    expect(result.cliCommand.match(/\*\*\*/g)).toHaveLength(2);
    expect(result.installUrl).toBe("/vi/download");
    expect(result.deepLinkUrl).toBe("ariadshift://tool/pdf.encrypt");
    const revealed = toolHandoff(tool("pdf.encrypt"), params, ["private-file.pdf"], {
      revealSecrets: true,
    });
    expect(revealed.cliCommand).toContain("private value");
    expect(revealed.deepLinkUrl).toBe(result.deepLinkUrl);
    expect(params.owner_password).toBe("private value");
  });

  it("roundtrips shell metacharacters, quotes, newlines and option-like filenames without execution", () => {
    const names = [
      "a'b.pdf",
      "$(echo hacked).pdf",
      "line\nfile.pdf",
      "-flag.pdf",
      "white space.pdf",
    ];
    const result = toolHandoff(tool("pdf.merge"), { outline: "a'b$()\n" }, names);
    const script = `ashift() { printf '%s\\0' "$@"; }; ${result.cliCommand}`;
    const args = execFileSync("sh", ["-c", script]).toString().split("\0").slice(0, -1);
    expect(args).toEqual([
      "tool",
      "pdf.merge",
      ...names.map((name) => (name.startsWith("-") ? `./${name}` : name)),
      "--params",
      JSON.stringify({ outline: "a'b$()\n" }),
    ]);
    expect(() => shellQuote("bad\0name")).toThrow();
  });
});
