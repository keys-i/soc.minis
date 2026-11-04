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
  expect(new URL(binding.fetch.mock.calls[0][0].url).pathname).toBe("/eventy");
  expect(new URL(binding.fetch.mock.calls[1][0].url).pathname).toBe("/index.html");

  const asset = await worker.fetch(new Request("https://uni.keysi.dev/soc/assets/app.js"), {
    ASSETS: binding,
  });
  expect(asset.status).toBe(200);
  expect(asset.headers.get("Cache-Control")).toBe("public, max-age=31536000");
  expect(new URL(binding.fetch.mock.calls[2][0].url).pathname).toBe("/assets/app.js");
});

test("the SOC Worker keeps native workspace failures explicit and private", async () => {
  const binding = assets();
  const response = await worker.fetch(new Request("https://uni.keysi.dev/soc/api/status"), {
    ASSETS: binding,
  });
  expect(response.status).toBe(503);
  expect(response.headers.get("Cache-Control")).toBe("no-store");
  await expect(response.json()).resolves.toMatchObject({ status: "degraded" });
  expect(binding.fetch).not.toHaveBeenCalled();

  const redirect = await worker.fetch(new Request("https://uni.keysi.dev/soc"), {
    ASSETS: binding,
  });
  expect(redirect.status).toBe(308);
  expect(redirect.headers.get("Location")).toBe("https://uni.keysi.dev/soc/");
});
