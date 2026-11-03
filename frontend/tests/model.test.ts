import { describe, expect, test } from "vitest";
import { capacityBand, eventToml, initialDraft } from "../src/model";

describe("event source", () => {
  test("escapes person-entered TOML and retains selected outputs", () => {
    const source = eventToml(
      {
        ...initialDraft,
        club: 'Bird Club "North"',
        title: "Welcome flight",
        date: "2026-11-10",
      },
      "risk",
      "pack",
    );
    expect(source).toContain('name = "Bird Club \\"North\\""');
    expect(source).toContain('mode = "risk"');
    expect(source).toContain('risk_layout = "pack"');
  });

  test("maps room seats to the booking bands", () => {
    expect(capacityBand(20)).toBe("1-20");
    expect(capacityBand(21)).toBe("21-50");
    expect(capacityBand(201)).toBe("201+");
  });
});
