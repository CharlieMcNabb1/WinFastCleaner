import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// Checks the color tokens in app.css (see docs/design.md).

const css = readFileSync(new URL("../app.css", import.meta.url), "utf8");

function block(selector: string): Map<string, string> {
  const start = css.indexOf(`${selector} {`);
  assert.ok(start >= 0, `missing ${selector}`);
  const body = css.slice(start, css.indexOf("}", start));
  return new Map([...body.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1]!, m[2]!.trim()]));
}

const light = block(":root");
const darkMedia = block(':root:not([data-theme="light"])');
const darkPicked = block(':root[data-theme="dark"]');

function luminance(hex: string): number {
  assert.match(hex, /^#[0-9a-f]{6}$/i);
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [16, 8, 0].map((s) => {
    const c = ((n >> s) & 255) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

test("both dark blocks declare the same tokens", () => {
  assert.deepEqual(darkMedia, darkPicked);
});

for (const [name, tokens] of [
  ["light", light],
  ["dark", new Map([...light, ...darkMedia])],
] as const) {
  test(`${name} text contrast is at least 4.5:1`, () => {
    const get = (t: string) => tokens.get(`--${t}`) ?? assert.fail(`missing --${t}`);
    const pairs: [string, string][] = [
      ["accent-text", "accent"],
      ["accent", "accent-2"],
      ["tier-green", "tier-green-bg"],
      ["tier-yellow", "tier-yellow-bg"],
      ["tier-red", "tier-red-bg"],
    ];
    for (const fg of ["text", "text-2", "accent", "tier-green", "tier-yellow", "tier-red"])
      for (const bg of ["bg", "surface", "surface-2"]) pairs.push([fg, bg]);
    for (const [fg, bg] of pairs) {
      const ratio = contrast(get(fg), get(bg));
      assert.ok(ratio >= 4.5, `${name}: --${fg} on --${bg} is ${ratio.toFixed(2)}:1`);
    }
  });
}
