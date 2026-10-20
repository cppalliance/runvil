// Pins the CSS wiring end to end: the esbuild bundle must emit
// dist/app.css containing the style guide's design tokens
// (shared-ui/tokens.css) and the cascade layer order, and
// dist/index.html must link that stylesheet - a dropped import in
// main.ts or a dropped <link> would ship an unstyled UI without any
// build failure. Run after `npm run build` (a debug `cargo build` also
// produces dist/).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { bundledDeclarations, bundledRuleCount, squash } from "../css-support.mjs";

const distDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "dist");

test("the bundled stylesheet defines the design tokens and layer order", async () => {
  const css = await readFile(path.join(distDir, "app.css"), "utf8");
  // \s* tolerates minified output; /i tolerates hex case changes.
  assert.match(css, /--panel:\s*#131520/i, "the page surface is defined");
  assert.match(css, /--accent:\s*#ff8a1e/i, "the orange accent token is defined");
  assert.match(
    css,
    /@layer reset,\s*base,\s*components,\s*utilities/,
    "the cascade layer order is declared",
  );
  assert.match(
    css,
    /textarea\.input\s*\{[^}]*height:\s*auto[^}]*border-radius:\s*var\(--radius\)/,
    "multiline inputs keep the field radius and natural height",
  );
  assert.match(
    css,
    /\.split-list :focus-visible[^}]*outline-offset:\s*-2px/,
    "focus rings stay inset inside scrollable panes",
  );
});

test("the desk page links the bundled stylesheet", async () => {
  const html = await readFile(path.join(distDir, "index.html"), "utf8");
  assert.match(
    html,
    /<link rel="stylesheet" href="app\.css">/,
    "index.html pulls in app.css",
  );
});

// The palette is the Gateway UI style guide's, value for value. Hex
// literals compare without case.
const STYLE_GUIDE_TOKENS = {
  "--chrome": "#0f1118",
  "--panel": "#131520",
  "--panel-2": "#171a26",
  "--panel-3": "#1c2030",
  "--line": "#262b3a",
  "--line-soft": "#20242f",
  "--line-focus": "#33405e",
  "--text": "#eceef4",
  "--dim": "#9aa2b4",
  "--faint": "#697082",
  "--accent": "#ff8a1e",
  "--accent-dim": "#8a4f16",
  "--good": "#4bbf73",
  "--good-text": "#5fd08a",
  "--blue": "#4d8dfe",
  "--blue-text": "#6ea6ff",
  "--warn": "#e8b06a",
  "--danger": "#e06a6a",
  "--logo-tile": "#0c0e14",
  "--font-size-root": "14.5px",
  "--radius": "9px",
  "--radius-xl": "12px",
};

test("the bundled stylesheet carries the style guide's palette", async () => {
  const css = await readFile(path.join(distDir, "app.css"), "utf8");
  for (const [name, value] of Object.entries(STYLE_GUIDE_TOKENS)) {
    assert.match(css, new RegExp(`${name}:\\s*${value}\\s*;`, "i"), `${name} is ${value}`);
  }
  // The minifier writes 300ms as .3s, so the duration is pinned by value.
  assert.match(css, /--duration-slower:\s*(300ms|\.3s)\s*;/, "the slow transition is 300ms");
  assert.doesNotMatch(css, /--cursor-/, "no Cursor Dark token survives");
});

test("the root size is the style guide's 14.5px", async () => {
  const html = await bundledDeclarations("html");
  assert.equal(html.get("font-size"), "var(--font-size-root)");
});

test("the primary button is the orange wash, the rest neutral raised, danger a red tint", async () => {
  const button = await bundledDeclarations(".button");
  assert.equal(button.get("height"), "var(--height-sm)", "28px tall");
  assert.equal(button.get("padding-inline"), "var(--space-3)", "12px padding");
  assert.equal(button.get("border-radius"), "var(--radius)", "a 9px radius");
  assert.equal(button.get("color"), "var(--dim)");
  const small = await bundledDeclarations(".button-sm");
  assert.equal(small.get("height"), "var(--height-xs)", "24px tall");
  assert.equal(small.get("border-radius"), "var(--radius-sm)", "an 8px radius");
  const primary = await bundledDeclarations(".button-primary");
  assert.equal(squash(primary.get("border-color")), "var(--accent-dim)");
  assert.match(squash(primary.get("background")), /^linear-gradient\(180deg,color-mix\(insrgb,var\(--accent\)18%,transparent\)/);
  assert.equal(primary.get("color"), "var(--accent)");
  const neutral = await bundledDeclarations(".button-outline");
  assert.equal(neutral.get("border-color"), "var(--line)");
  assert.equal(neutral.get("background"), "var(--panel-2)");
  const neutralHover = await bundledDeclarations(".button-outline:hover");
  assert.equal(neutralHover.get("background"), "var(--panel-3)", "a hover steps the surface up");
  const danger = await bundledDeclarations(".button-danger");
  assert.equal(danger.get("color"), "var(--danger)");
  const disabled = await bundledDeclarations(".button:disabled");
  assert.match(disabled.get("opacity"), /^0?\.5$/, "the minifier may drop the leading zero");
  for (const selector of [".button-primary:disabled", ".button-danger:disabled"]) {
    const disabledAction = await bundledDeclarations(selector);
    assert.equal(disabledAction.get("border-color"), "var(--line)", `${selector} loses its family border`);
    assert.equal(disabledAction.get("background"), "var(--panel-2)", `${selector} loses its wash`);
    assert.equal(disabledAction.get("color"), "var(--faint)", `${selector} loses its text color`);
  }
});

test("fields are bordered and light their border on focus", async () => {
  const input = await bundledDeclarations(".input");
  assert.equal(squash(input.get("border")), "1pxsolidvar(--line)");
  assert.equal(input.get("border-radius"), "var(--radius)");
  assert.equal(input.get("background"), "var(--panel)");
  const focus = await bundledDeclarations(".input:focus");
  assert.equal(focus.get("border-color"), "var(--line-focus)");
});

test("the switch is the single shared 32x20 track with a 16px thumb that moves 12px", async () => {
  assert.equal(await bundledRuleCount(".switch"), 1, "one .switch rule: the gateway keeps no copy");
  const track = await bundledDeclarations(".switch");
  assert.equal(track.get("width"), "32px");
  assert.equal(track.get("height"), "20px");
  assert.equal(track.get("background"), "var(--line)", "the line color when off");
  assert.equal(squash(track.get("transition")), "background-colorvar(--duration-slow)ease");
  const on = await bundledDeclarations(".switch[aria-checked=true]");
  assert.equal(on.get("background"), "var(--good)");
  const thumb = await bundledDeclarations(".switch::after");
  assert.equal(thumb.get("width"), "16px");
  assert.equal(thumb.get("height"), "16px");
  assert.match(thumb.get("background"), /^(white|#fff|#ffffff)$/, "a white thumb");
  const moved = await bundledDeclarations(".switch[aria-checked=true]::after");
  assert.match(moved.get("transform"), /^translateX?\(12px\)$/, "the thumb moves 12px");
});

test("modals take the form modal values", async () => {
  const overlay = await bundledDeclarations(".modal-overlay");
  assert.match(squash(overlay.get("background")), /^(rgba\(0,0,0,\.5\)|#00000080)$/, "a 0.5 backdrop");
  const dialog = await bundledDeclarations(".modal-dialog");
  assert.equal(dialog.get("width"), "320px", "a 320px card");
  assert.equal(dialog.get("border-radius"), "var(--radius-xl)", "a 12px radius");
  assert.equal(squash(dialog.get("border")), "1pxsolidvar(--line)", "a line border");
  assert.equal(dialog.get("background"), "var(--panel-2)", "the raised surface");
  assert.equal(dialog.get("box-shadow"), "none");
  assert.match(dialog.get("animation") ?? "", /var\(--duration-slower\)/, "opens in 300ms");
  const title = await bundledDeclarations(".modal-dialog > h2");
  assert.equal(squash(title.get("padding")), "var(--space-3)var(--space-4)", "header 12px 16px");
  assert.equal(title.get("font-size"), "var(--font-size-lg)", "a 15px title");
  const actions = await bundledDeclarations(".modal-actions");
  assert.equal(actions.get("padding"), "10px", "footer padding 10px");
  assert.equal(squash(actions.get("border-top")), "1pxsolidvar(--line-soft)", "a soft top divider");
});

test("both modal families drop the scale-in under reduced motion", async () => {
  const reduced = { media: "(prefers-reduced-motion: reduce)" };
  for (const selector of [".modal-dialog", ".modal"]) {
    const open = await bundledDeclarations(selector);
    assert.match(open.get("animation") ?? "", /modal-form-open/, `${selector} opens with the scale-in`);
    const still = await bundledDeclarations(selector, reduced);
    assert.equal(still.get("animation"), "none", `${selector} has no animation under reduced motion`);
  }
});