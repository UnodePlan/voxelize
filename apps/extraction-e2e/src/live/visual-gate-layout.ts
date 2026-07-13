import type { Page } from "@playwright/test";

import type { BrowserActorViewport } from "./browser-actor";

interface LayoutBox {
  bottom: number;
  height: number;
  left: number;
  name: string;
  right: number;
  top: number;
  width: number;
}

export interface VisualLayoutReport {
  assetCounts: { images: number; svg: number };
  boxes: LayoutBox[];
  counts: { equipment: number; hearts: number; inventory: number };
  errors: string[];
  viewport: { height: number; width: number };
}

export async function inspectVisualLayout(
  page: Page,
  expectedViewport: BrowserActorViewport,
): Promise<VisualLayoutReport> {
  return page.evaluate((expected) => {
    interface Box {
      bottom: number;
      height: number;
      left: number;
      name: string;
      right: number;
      top: number;
      width: number;
    }

    const errors: string[] = [];
    const boxes: Box[] = [];
    const visible = (element: Element): boolean => {
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return (
        !element.hasAttribute("hidden") &&
        style.display !== "none" &&
        style.visibility !== "hidden" &&
        Number(style.opacity) !== 0 &&
        rect.width > 0 &&
        rect.height > 0
      );
    };
    const box = (name: string, element: Element): Box => {
      const rect = element.getBoundingClientRect();
      const result = {
        bottom: rect.bottom,
        height: rect.height,
        left: rect.left,
        name,
        right: rect.right,
        top: rect.top,
        width: rect.width,
      };
      boxes.push(result);
      return result;
    };
    const required = (name: string, selector: string): Element | null => {
      const element = document.querySelector(selector);
      if (element === null) errors.push(`${name} missing (${selector})`);
      else if (!visible(element)) errors.push(`${name} is not visible`);
      return element;
    };
    const collect = (name: string, selector: string): Box[] =>
      Array.from(document.querySelectorAll(selector))
        .filter(visible)
        .map((element, index) => box(`${name}-${index + 1}`, element));
    const overlap = (left: Box, right: Box): boolean => {
      const width =
        Math.min(left.right, right.right) - Math.max(left.left, right.left);
      const height =
        Math.min(left.bottom, right.bottom) - Math.max(left.top, right.top);
      return width > 0.5 && height > 0.5 && width * height > 1;
    };
    const assertPairwise = (group: string, items: Box[]): void => {
      for (let left = 0; left < items.length; left += 1) {
        for (let right = left + 1; right < items.length; right += 1) {
          if (overlap(items[left], items[right])) {
            errors.push(
              `${group} overlap: ${items[left].name} / ${items[right].name}`,
            );
          }
        }
      }
    };
    const assertContained = (outer: Box, inner: Box): void => {
      const tolerance = 1;
      if (
        inner.left < outer.left - tolerance ||
        inner.top < outer.top - tolerance ||
        inner.right > outer.right + tolerance ||
        inner.bottom > outer.bottom + tolerance
      ) {
        errors.push(`${inner.name} escapes ${outer.name}`);
      }
    };

    if (innerWidth !== expected.width || innerHeight !== expected.height) {
      errors.push(
        `viewport expected ${expected.width}x${expected.height}, received ${innerWidth}x${innerHeight}`,
      );
    }
    const shellElement = required(
      "match shell",
      '[data-product-shell][data-screen="match"]',
    );
    const canvasElement = required(
      "live canvas",
      '[data-world-canvas][data-scene-mode="match"][data-live-world="ready"]',
    );
    const hudElement = required("HUD layer", "[data-hud-layer]");
    required("online state", '[data-connection="online"]');
    const topbarElement = required("topbar", ".game-topbar");
    const statusElement = required("match status", ".match-status-strip");
    const crosshairElement = required("crosshair", ".crosshair");
    const playerHudElement = required(
      "player HUD",
      ".player-hud:not(.player-hud-loading)",
    );
    if (document.querySelector(".player-hud-loading") !== null) {
      errors.push("player HUD is still loading");
    }

    const notice = document.querySelector<HTMLElement>("[data-notice-bar]");
    if (
      notice !== null &&
      visible(notice) &&
      notice.textContent?.trim() !== ""
    ) {
      errors.push(`notice bar is visible: ${notice.textContent?.trim()}`);
    }

    const shell =
      shellElement === null ? null : box("match-shell", shellElement);
    const canvas =
      canvasElement === null ? null : box("live-canvas", canvasElement);
    const topbar = topbarElement === null ? null : box("topbar", topbarElement);
    const status =
      statusElement === null ? null : box("match-status", statusElement);
    const crosshair =
      crosshairElement === null ? null : box("crosshair", crosshairElement);
    const playerHud =
      playerHudElement === null ? null : box("player-hud", playerHudElement);
    if (hudElement !== null) box("hud-layer", hudElement);

    const action = document.querySelector(".action-progress");
    const topLevel = [topbar, status, crosshair, playerHud].filter(
      (item): item is Box => item !== null,
    );
    if (action !== null && visible(action)) {
      topLevel.push(box("action-progress", action));
    }
    assertPairwise("top-level HUD", topLevel);

    const statusChildren = collect(
      "status-item",
      ".match-status-strip > .connection-state, .match-status-strip > .phase-clock, .match-status-strip > .match-code",
    );
    assertPairwise("status items", statusChildren);
    assertPairwise("topbar items", collect("topbar-item", ".game-topbar > *"));

    const hearts = collect("heart", ".health-row .heart");
    const equipment = collect("equipment", ".loadout-row > .equipment-slot");
    const inventory = collect("inventory", ".inventory-grid > .inventory-slot");
    assertPairwise("hearts", hearts);
    assertPairwise("loadout slots", [...equipment, ...inventory]);
    if (hearts.length !== 10)
      errors.push(`expected 10 hearts, received ${hearts.length}`);
    if (equipment.length !== 2) {
      errors.push(`expected 2 equipment slots, received ${equipment.length}`);
    }
    if (inventory.length !== 12) {
      errors.push(`expected 12 inventory slots, received ${inventory.length}`);
    }

    const health = document.querySelector(".health-row");
    const loadout = document.querySelector(".loadout-row");
    if (
      health !== null &&
      loadout !== null &&
      visible(health) &&
      visible(loadout)
    ) {
      const healthBox = box("health-row", health);
      const loadoutBox = box("loadout-row", loadout);
      if (overlap(healthBox, loadoutBox))
        errors.push("health and loadout overlap");
    }

    if (shell !== null) {
      for (const item of [...topLevel, ...hearts, ...equipment, ...inventory]) {
        assertContained(shell, item);
      }
      if (canvas !== null) assertContained(shell, canvas);
    }

    const documentWidth = Math.max(
      document.documentElement.scrollWidth,
      document.body?.scrollWidth ?? 0,
    );
    const documentHeight = Math.max(
      document.documentElement.scrollHeight,
      document.body?.scrollHeight ?? 0,
    );
    if (documentWidth > innerWidth + 1 || documentHeight > innerHeight + 1) {
      errors.push(
        `document overflow ${documentWidth}x${documentHeight} in ${innerWidth}x${innerHeight}`,
      );
    }

    const textSelectors = [
      ".connection-state",
      ".phase-clock span",
      ".phase-clock strong",
      ".match-code",
      ".equipment-slot span",
      ".inventory-slot strong",
      ".network-chip",
      ".wallet-address",
    ];
    for (const element of Array.from(
      document.querySelectorAll(textSelectors.join(",")),
    )) {
      if (
        visible(element) &&
        (element.scrollWidth > element.clientWidth + 1 ||
          element.scrollHeight > element.clientHeight + 1)
      ) {
        errors.push(`text overflow: ${element.className}`);
      }
    }

    const images = Array.from(document.images);
    for (const image of images) {
      if (
        !image.complete ||
        image.naturalWidth <= 0 ||
        image.naturalHeight <= 0
      ) {
        errors.push(`image did not render: ${image.currentSrc || image.src}`);
      }
    }
    const svg = Array.from(document.querySelectorAll("[data-hud-layer] svg"));
    if (svg.length === 0) errors.push("HUD icons did not render");
    for (const icon of svg) {
      const rect = icon.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0)
        errors.push("zero-sized HUD icon");
    }

    return {
      assetCounts: { images: images.length, svg: svg.length },
      boxes,
      counts: {
        equipment: equipment.length,
        hearts: hearts.length,
        inventory: inventory.length,
      },
      errors,
      viewport: { height: innerHeight, width: innerWidth },
    };
  }, expectedViewport);
}
