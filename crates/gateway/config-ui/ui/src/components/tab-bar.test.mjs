// Pins the tab bar: the right-side actions cluster shows the baked crate
// version (or the dev fallback) as a muted `vX.Y.Z` span, and the seven
// tabs are the sidebar - a full-height left rail of grouped links that
// marks the current route, keeps a label for every icon, collapses to its
// icons from its toggle, and always shows only the icons under 710px.
import assert from "node:assert/strict";
import test from "node:test";

import { bundledDeclarations, squash } from "../css-support.mjs";
import { bootApp, gatewayStub, modelsFixture, navigate, settle } from "../test-support.mjs";

const NARROW = "(max-width: 709.98px)";

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

test("the primary nav is a column beside the content, with a divider between each of three groups", async () => {
  const { root } = await openDesk();
  const nav = root.querySelector(".desk-layout > nav.tab-nav");
  assert.ok(nav, "the nav sits in the desk layout, not in the header strip");
  assert.equal(nav.getAttribute("aria-label"), "Primary");
  assert.ok(
    root.querySelector(".desk-layout > .desk-content > main#main"),
    "the content column holds main",
  );
  assert.ok(nav.firstElementChild.matches("button.tab-nav-toggle"), "the toggle leads the sidebar");
  const sequence = [...nav.querySelectorAll(":scope > .tab, :scope > [role='separator']")].map((child) =>
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

test("every nav cell has a 14px icon and a label that survives the icon-only rail", async () => {
  const { root } = await openDesk();
  const tabs = [...root.querySelectorAll(".tab-nav .tab")];
  assert.equal(tabs.length, 7);
  for (const tab of tabs) {
    const icon = tab.querySelector("svg");
    assert.ok(icon, `${tab.textContent} has an icon`);
    assert.equal(icon.getAttribute("width"), "14");
    assert.equal(icon.getAttribute("height"), "14");
    assert.equal(icon.getAttribute("aria-hidden"), "true");
    const label = tab.querySelector(".tab-label");
    assert.ok(label && label.textContent !== "", "the label stays in the DOM for assistive tech");
    assert.equal(tab.getAttribute("title"), label.textContent, "the icon-only column keeps a tooltip");
  }
});

test("the toggle collapses the sidebar to its icons and expands it again", async () => {
  const { root } = await openDesk();
  const nav = root.querySelector("nav.tab-nav");
  const toggle = nav.querySelector(".tab-nav-toggle");
  assert.equal(toggle.getAttribute("aria-controls"), nav.id, "the toggle names the sidebar it controls");
  assert.equal(toggle.getAttribute("aria-expanded"), "true", "the sidebar starts expanded");
  assert.equal(toggle.getAttribute("aria-label"), "Collapse sidebar");
  assert.equal(nav.classList.contains("is-collapsed"), false);

  toggle.click();
  assert.equal(nav.classList.contains("is-collapsed"), true, "a press collapses the sidebar");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(toggle.getAttribute("aria-label"), "Expand sidebar");
  assert.equal(nav.querySelectorAll(".tab .tab-label").length, 7, "the labels stay as link names");

  toggle.click();
  assert.equal(nav.classList.contains("is-collapsed"), false, "a second press expands it");
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
});

test("the sidebar is a full-height chrome rail under the header strip", async () => {
  const header = await bundledDeclarations(".tab-bar");
  assert.equal(header.get("position"), "sticky");
  assert.equal(header.get("height"), "var(--desk-header-height)");

  const layout = await bundledDeclarations(".desk-layout");
  assert.equal(layout.get("display"), "flex");
  assert.equal(squash(layout.get("min-height")), "calc(100svh-var(--desk-header-height))");

  const nav = await bundledDeclarations(".tab-nav");
  assert.equal(nav.get("position"), "sticky");
  assert.equal(nav.get("inset-block-start"), "var(--desk-header-height)", "pinned under the header");
  assert.equal(squash(nav.get("height")), "calc(100svh-var(--desk-header-height))", "full height");
  assert.equal(nav.get("width"), "var(--sidebar-width)");
  assert.equal(nav.get("background"), "var(--chrome)");
  assert.equal(squash(nav.get("border-right")), "1pxsolidvar(--line)");

  const cell = await bundledDeclarations(".tab");
  assert.equal(cell.get("border-radius"), "var(--radius)", "a 9px radius");
  assert.equal(cell.get("font-size"), "var(--font-size-base)");
  assert.equal(cell.get("color"), "var(--dim)");
  const hover = await bundledDeclarations(".tab:hover");
  const selected = await bundledDeclarations(".tab[aria-current=page]");
  assert.equal(hover.get("background"), "var(--panel-2)", "a hover raises the cell one step");
  assert.equal(hover.get("color"), "var(--text)");
  assert.equal(selected.get("background"), "var(--panel-3)", "the current page two steps");
  assert.equal(selected.get("color"), "var(--text)");

  const divider = await bundledDeclarations(".tab-divider");
  assert.equal(squash(divider.get("border-top")), "1pxsolidvar(--line-soft)");

  const collapsed = await bundledDeclarations(".tab-nav.is-collapsed");
  assert.equal(collapsed.get("width"), "var(--sidebar-collapsed-width)");
  const toggleRule = await bundledDeclarations(".tab-nav-toggle");
  assert.equal(toggleRule.get("align-self"), "flex-end", "the toggle hugs its chevron at the rail's end");
  const collapsedToggle = await bundledDeclarations(".tab-nav.is-collapsed .tab-nav-toggle");
  assert.equal(collapsedToggle.get("align-self"), "center", "and centers over the icon column");
  const collapsedLabel = await bundledDeclarations(".tab-nav.is-collapsed .tab-label");
  assert.equal(squash(collapsedLabel.get("clip-path")), "inset(50%)", "the label hides but stays readable");

  const narrow = await bundledDeclarations(".tab-nav", { media: NARROW });
  assert.equal(narrow.get("width"), "var(--sidebar-collapsed-width)", "icons only in a narrow window");
  const narrowToggle = await bundledDeclarations(".tab-nav-toggle", { media: NARROW });
  assert.equal(narrowToggle.get("display"), "none", "the narrow rail has nothing to toggle");
  const label = await bundledDeclarations(".tab-label", { media: NARROW });
  assert.equal(squash(label.get("clip-path")), "inset(50%)", "the label hides but stays readable");
});
