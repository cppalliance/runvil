// The tab bar: a header strip with the medallion (standalone only), the
// profile switcher, and the right cluster holding the connection dot
// [Adapted: llama-swap] plus the container the Apply/Revert pair mounts
// into; and, as its own element, the sidebar - seven icon+label links in
// three groups under a collapse toggle, set in a full-height left rail that
// collapses to its icons on request and always in a narrow window. The
// caller mounts the strip above the desk and the sidebar beside the
// content.

import {
  ChevronLeft,
  ChevronRight,
  Cloud,
  Cpu,
  Folder,
  Globe,
  Key,
  Search,
  Settings,
  createElement as lucideElement,
} from "lucide";
import type { IconNode } from "lucide";

import type { PageId } from "../router";
import { programIcon } from "./program-icon";

// The crate version, substituted by the esbuild define in build.mjs; a
// bundle built without the define shows the "dev" fallback instead of
// breaking on a free identifier.
declare const __APP_VERSION__: string | undefined;
const APP_VERSION = typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "dev";

/** One tab: destination view, label, lucide icon, and hash target. */
type Tab = readonly [view: PageId, label: string, icon: IconNode, href: string];

/** The sidebar's icon size. */
const ICON_SIZE = 14;

/**
 * The tabs in nav order, grouped: the gateway's own settings, then the
 * model catalogs, then the profiles and the secrets they read. A divider
 * sits between groups.
 */
const TAB_GROUPS: ReadonlyArray<readonly Tab[]> = [
  [["settings", "Settings", Settings, "#/settings"]],
  [
    ["discover", "Discover", Search, "#/discover"],
    ["local", "Local", Cpu, "#/local"],
    ["remote", "Remote", Globe, "#/remote"],
    ["cloud", "Cloud", Cloud, "#/cloud"],
  ],
  [
    ["profiles", "Profiles", Folder, "#/profiles"],
    ["secrets", "Secrets", Key, "#/secrets"],
  ],
];

/** Construction options for the tab bar. */
export interface TabBarOptions {
  /** Standalone mode shows the medallion; the workshop panel hides it. */
  showMedallion: boolean;
  /** The profile switcher element (or its panel-mode placeholder). */
  switcher: HTMLElement;
  /** Fired when the Apply (N) button is pressed. */
  onApply?: () => void;
  /** Fired when the Revert All button is pressed. */
  onRevertAll?: () => void;
}

/** The mounted tab bar and its live-update handles. */
export interface TabBar {
  /** The `<header class="tab-bar">` strip: medallion, profile switcher, actions. */
  element: HTMLElement;
  /** The `<nav class="tab-nav">` sidebar of grouped links, mounted beside the content. */
  nav: HTMLElement;
  /** Moves `aria-current` (and the selected fill) to `view`. */
  setActivePage(view: PageId | null): void;
  /** Recolors the connection dot from the latest API outcome. */
  setConnected(ok: boolean): void;
  /**
   * Shows Apply (N) + Revert All when `count` is positive, hides the
   * pair at zero. `count` is the pending-file count from the dirty
   * report.
   */
  setPendingCount(count: number): void;
}

/** Builds the tab bar. */
export function createTabBar(options: TabBarOptions): TabBar {
  const element = document.createElement("header");
  element.className = "tab-bar";

  if (options.showMedallion) {
    const medallion = programIcon(24, "Runvil");
    medallion.className = "tab-medallion";
    element.append(medallion);
  }

  element.append(options.switcher);

  const nav = document.createElement("nav");
  nav.setAttribute("aria-label", "Primary");
  nav.id = "primary-nav";
  nav.className = "tab-nav";
  // The toggle collapses the sidebar to its icons; the state lasts for the
  // session only.
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "tab-nav-toggle";
  toggle.setAttribute("aria-controls", nav.id);
  const setCollapsed = (collapsed: boolean) => {
    nav.classList.toggle("is-collapsed", collapsed);
    toggle.setAttribute("aria-expanded", String(!collapsed));
    toggle.setAttribute("aria-label", collapsed ? "Expand sidebar" : "Collapse sidebar");
    toggle.title = toggle.getAttribute("aria-label") ?? "";
    toggle.replaceChildren(
      lucideElement(collapsed ? ChevronRight : ChevronLeft, {
        "aria-hidden": "true",
        width: ICON_SIZE,
        height: ICON_SIZE,
      }),
    );
  };
  setCollapsed(false);
  toggle.addEventListener("click", () => setCollapsed(!nav.classList.contains("is-collapsed")));
  nav.append(toggle);
  const tabByPage = new Map<PageId, HTMLAnchorElement>();
  TAB_GROUPS.forEach((group, index) => {
    if (index > 0) {
      const divider = document.createElement("div");
      divider.className = "tab-divider";
      divider.setAttribute("role", "separator");
      nav.append(divider);
    }
    for (const [view, label, icon, href] of group) {
      const tab = document.createElement("a");
      tab.className = "tab";
      tab.href = href;
      // The label is the link's name; a collapsed sidebar hides it, so a
      // tooltip keeps the icon-only rail readable.
      tab.title = label;
      const svg = lucideElement(icon, {
        "aria-hidden": "true",
        width: ICON_SIZE,
        height: ICON_SIZE,
      });
      const text = document.createElement("span");
      text.className = "tab-label";
      text.textContent = label;
      tab.append(svg, text);
      nav.append(tab);
      tabByPage.set(view, tab);
    }
  });

  const actions = document.createElement("div");
  actions.className = "tab-actions";
  const version = document.createElement("span");
  version.className = "tab-version";
  version.textContent = `v${APP_VERSION}`;
  const dot = document.createElement("span");
  dot.className = "status-dot";
  const dotText = document.createElement("span");
  dotText.className = "visually-hidden";
  dotText.textContent = "Gateway status unknown";
  dot.append(dotText);
  // The Apply/Revert pair [INVENTED] renders in here whenever the dirty
  // report says shadow files exist.
  const pending = document.createElement("div");
  pending.className = "apply-actions";
  actions.append(version, dot, pending);
  element.append(actions);

  return {
    element,
    nav,
    setPendingCount(count: number): void {
      if (count <= 0) {
        pending.replaceChildren();
        return;
      }
      const apply = document.createElement("button");
      apply.type = "button";
      apply.className = "button button-sm button-primary apply-button";
      apply.textContent = `Apply (${count})`;
      apply.addEventListener("click", () => options.onApply?.());
      const revert = document.createElement("button");
      revert.type = "button";
      revert.className = "button button-sm button-outline revert-button";
      revert.textContent = "Revert All";
      revert.addEventListener("click", () => options.onRevertAll?.());
      pending.replaceChildren(apply, revert);
    },
    setActivePage(view: PageId | null): void {
      for (const [tabView, tab] of tabByPage) {
        if (tabView === view) {
          tab.setAttribute("aria-current", "page");
        } else {
          tab.removeAttribute("aria-current");
        }
      }
    },
    setConnected(ok: boolean): void {
      dot.classList.toggle("is-ok", ok);
      dot.classList.toggle("is-bad", !ok);
      dotText.textContent = ok ? "Gateway reachable" : "Gateway unreachable";
    },
  };
}
