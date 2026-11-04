import { expect, test, vi } from "vitest";
import worker from "../../deploy/edge";

function assets() {
  return {
    fetch: vi.fn(async (request: Request) => {
      const pathname = new URL(request.url).pathname;
      if (pathname === "/index.html")
        return new Response("<main>SOC Minis</main>", {
          headers: { "Content-Type": "text/html", ETag: "release" },
        });
      if (pathname === "/assets/app.js")
        return new Response("export {}", {
          headers: { "Cache-Control": "public, max-age=31536000" },
        });
      return new Response("missing", { status: 404 });
    }),
  };
}

test("the SOC Worker serves scoped assets and deep links without an origin server", async () => {
  const binding = assets();
  const deepLink = await worker.fetch(new Request("https://uni.keysi.dev/soc/eventy"), {
    ASSETS: binding,
  });
  expect(deepLink.status).toBe(200);
  await expect(deepLink.text()).resolves.toBe("<main>SOC Minis</main>");
  expect(deepLink.headers.get("X-Frame-Options")).toBe("DENY");
  expect(deepLink.headers.get("Cache-Control")).toBe("no-cache");
  expect(new URL(binding.fetch.mock.calls[0][0].url).pathname).toBe("/index.html");

  const asset = await worker.fetch(new Request("https://uni.keysi.dev/soc/assets/app.js"), {
    ASSETS: binding,
  });
  expect(asset.status).toBe(200);
  expect(asset.headers.get("Cache-Control")).toBe("public, max-age=31536000");
  expect(new URL(binding.fetch.mock.calls[1][0].url).pathname).toBe("/assets/app.js");
});

test("the SOC Worker opens a stable private browser workspace", async () => {
  const binding = assets();
  const first = await worker.fetch(new Request("https://uni.keysi.dev/soc/api/status"), {
    ASSETS: binding,
  });
  expect(first.status).toBe(200);
  expect(first.headers.get("Cache-Control")).toBe("no-store");
  const cookie = first.headers.get("Set-Cookie");
  expect(cookie).toMatch(/^__Host-soc_workspace=[a-f0-9]{64};/);
  const account = await first.json();
  expect(account).toMatchObject({
    workspace: expect.stringMatching(/^[a-f0-9]{64}$/),
    microsoft_mailbox: null,
    microsoft_ready: false,
    forms_ready: false,
  });

  const second = await worker.fetch(
    new Request("https://uni.keysi.dev/soc/api/status", {
      headers: { Cookie: cookie!.split(";", 1)[0] },
    }),
    { ASSETS: binding },
  );
  expect(second.headers.get("Set-Cookie")).toBeNull();
  await expect(second.json()).resolves.toEqual(account);
  expect(binding.fetch).not.toHaveBeenCalled();
});

test("the SOC Worker keeps native operations explicit and publishes its installer", async () => {
  const binding = assets();
  const native = await worker.fetch(new Request("https://uni.keysi.dev/soc/api/event"), {
    ASSETS: binding,
  });
  expect(native.status).toBe(501);
  expect(native.headers.get("Cache-Control")).toBe("no-store");
  await expect(native.json()).resolves.toMatchObject({ status: "native" });

  const install = await worker.fetch(new Request("https://uni.keysi.dev/soc/install"), {
    ASSETS: binding,
  });
  expect(install.status).toBe(200);
  expect(install.headers.get("Content-Type")).toContain("text/x-shellscript");
  await expect(install.text()).resolves.toContain('"${CARGO}" install --locked --force');

  const redirect = await worker.fetch(new Request("https://uni.keysi.dev/soc"), {
    ASSETS: binding,
  });
  expect(redirect.status).toBe(308);
  expect(redirect.headers.get("Location")).toBe("https://uni.keysi.dev/soc/");
});
