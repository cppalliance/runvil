// Pins the tab bar: the right-side actions cluster shows the baked crate
// version (or the dev fallback) as a muted `vX.Y.Z` span, and the seven
// tabs are the primary nav - a row of grouped links in the header strip
// that marks the current route, keeps a label for every icon, and shows
// only the icons under 710px.
import assert from "node:assert/strict";
import test from "node:test";

import { bundledDeclarations, squash } from "../css-support.mjs";
import { bootApp, gatewayStub, modelsFixture, navigate, settle } from "../test-support.mjs";

const NARROW = "(max-width: 709.98px)";
const MID = "(max-width: 1023.98px)";

async function openDesk() {
  const stub = gatewayStub({ key: "k", config: modelsFixture() });
  return bootApp({ key: "k", stub });
}

test("the tab bar shows the baked version in the actions cluster", async () => {
  const { root } = await openDesk();
  const label = root.querySelector(".tab-bar .tab-actions .tab-version");
  assert.ok(label, "the version label renders in the tab-actions cluster");
  assert.match(
    label.textContent,
    /^v(\d+\.\d+\.\d+|dev)$/,
    "the label shows the baked crate version (or the dev fallback)",
  );
});

test("the primary nav sits in the header strip, with a divider between each of three groups", async () => {
  const { root } = await openDesk();
  const nav = root.querySelector("header.tab-bar > nav.tab-nav");
  assert.ok(nav, "the nav sits in the header strip");
  assert.equal(nav.getAttribute("aria-label"), "Primary");
  assert.equal(root.querySelector(".desk-layout nav"), null, "no nav beside the content");
  assert.ok(nav.nextElementSibling.matches(".tab-actions"), "the right cluster follows the nav");
  assert.ok(
    root.querySelector(".desk-layout > .desk-content > main#main"),
    "the content column holds main",
  );
  const sequence = [...nav.children].map((child) =>
    child.getAttribute("role") === "separator" ? "|" : child.textContent,
  );
  assert.deepEqual(
    sequence,
    ["Settings", "|", "Discover", "Local", "Remote", "Cloud", "|", "Profiles", "Secrets"],
    "dividers fall between groups, never at either end",
  );
});

test("the nav marks exactly the current route and follows the hash", async () => {
  const { dom, root } = await openDesk();
  for (const [hash, title] of [
    ["#/secrets", "Secrets"],
    ["#/cloud", "Cloud"],
    ["#/settings", "Settings"],
  ]) {
    navigate(dom, hash);
    await settle();
    const current = [...root.querySelectorAll(".tab-nav .tab[aria-current='page']")];
    assert.deepEqual(
      current.map((tab) => tab.textContent),
      [title],
      `${hash} marks only ${title}`,
    );
  }
});

test("every nav cell has a 12px icon and a label that survives the icon-only row", async () => {
  const { root } = await openDesk();
  const tabs = [...root.querySelectorAll(".tab-nav .tab")];
  assert.equal(tabs.length, 7);
  for (const tab of tabs) {
    const icon = tab.querySelector("svg");
    assert.ok(icon, `${tab.textContent} has an icon`);
    assert.equal(icon.getAttribute("width"), "12");
    assert.equal(icon.getAttribute("height"), "12");
    assert.equal(icon.getAttribute("aria-hidden"), "true");
    const label = tab.querySelector(".tab-label");
    assert.ok(label && label.textContent !== "", "the label stays in the DOM for assistive tech");
    assert.equal(tab.getAttribute("title"), label.textContent, "the icon-only row keeps a tooltip");
  }
});

test("the nav is a row with vertical dividers that takes its own row under 1024px and is icon-only under 710px", async () => {
  const nav = await bundledDeclarations(".tab-nav");
  assert.equal(nav.get("display"), "flex");
  assert.equal(nav.get("flex-direction"), undefined, "a row, the flex default");
  assert.equal(nav.get("position"), undefined, "no longer a sticky column");
  assert.equal(nav.get("overflow-x"), "auto", "a row that still runs out of room scrolls");
  assert.equal(nav.get("min-width"), "0", "the row can shrink inside the strip");

  const cell = await bundledDeclarations(".tab");
  assert.equal(squash(cell.get("padding")), "4px6px", "cells are padded 4px 6px");
  assert.equal(cell.get("border-radius"), "var(--radius)", "a 6px radius");
  assert.equal(cell.get("font-size"), "var(--font-size-sm)", "12px text");
  assert.equal(cell.get("line-height"), "var(--line-height-sm)", "16px lines");
  assert.equal(cell.get("color"), "var(--cursor-text-secondary)");
  const hover = await bundledDeclarations(".tab:hover");
  const selected = await bundledDeclarations(".tab[aria-current=page]");
  assert.equal(hover.get("background"), "var(--cursor-bg-quaternary)", "6% hover fill");
  assert.equal(selected.get("background"), "var(--cursor-bg-quaternary)", "6% selected fill");

  const cellRule = await bundledDeclarations(".tab");
  assert.equal(cellRule.get("white-space"), "nowrap", "a label never wraps inside its cell");
  const focus = await bundledDeclarations(".tab:focus-visible");
  assert.equal(focus.get("outline-offset"), "-2px", "the focus ring stays inside the scrolling row");

  const divider = await bundledDeclarations(".tab-divider");
  assert.equal(squash(divider.get("border-left")), "1pxsolidvar(--cursor-stroke-tertiary)");
  assert.equal(divider.get("align-self"), "stretch", "a vertical hairline the height of the row");
  const midBar = await bundledDeclarations(".tab-bar", { media: MID });
  assert.equal(midBar.get("flex-wrap"), "wrap", "under 1024px the strip wraps");
  const midNav = await bundledDeclarations(".tab-nav", { media: MID });
  assert.equal(midNav.get("order"), "1", "the nav moves after the actions");
  assert.equal(midNav.get("flex-basis"), "100%", "onto its own full-width row");

  const label = await bundledDeclarations(".tab-label", { media: NARROW });
  assert.equal(squash(label.get("clip-path")), "inset(50%)", "the label hides but stays readable");
});
