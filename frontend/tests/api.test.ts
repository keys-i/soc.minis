import { afterEach, describe, expect, test, vi } from "vitest";
import { status } from "../src/api";

afterEach(() => vi.restoreAllMocks());

describe("account status", () => {
  test("accepts a pseudonymous hosted workspace", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response({
      workspace: "0123456789abcdef01234567",
      microsoft_mailbox: null,
      microsoft_ready: false,
      forms_ready: true,
    })));

    await expect(status()).resolves.toMatchObject({ workspace: "0123456789abcdef01234567" });
  });

  test("rejects a workspace value that cannot safely scope storage", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response({
      workspace: "../../shared",
      microsoft_mailbox: null,
      microsoft_ready: false,
      forms_ready: true,
    })));

    await expect(status()).rejects.toThrow("unexpected data");
  });
});

function response(value: unknown) {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}
