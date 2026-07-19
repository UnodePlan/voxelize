/**
 * 双独立浏览器 context 的 DEV 多人局内冒烟（避免同 profile 共享 cookie）。
 * 依赖：server :4100 DEV_MATCH=2 + vite :5173
 *
 *   node apps/extraction-e2e/scripts/dev-two-browser-smoke.mjs
 */

import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "@playwright/test";

const ORIGIN = process.env.EXTRACTION_E2E_PUBLIC_ORIGIN ?? "http://127.0.0.1:5173";
const TIMEOUT_MS = 120_000;
const OUT_DIR = join(
  dirname(fileURLToPath(import.meta.url)),
  "../test-results/dev-two-browser-smoke",
);

function seatUrl(seat) {
  return `${ORIGIN}/?mode=dev-mp&seat=${seat}`;
}

async function waitForConnected(page, label) {
  const deadline = Date.now() + TIMEOUT_MS;
  let last = "";
  while (Date.now() < deadline) {
    const probe = await page.evaluate(() => {
      const body = document.body?.innerText ?? "";
      const banner = document.querySelector(".dev-mp-banner")?.textContent ?? "";
      const connection = document.querySelector(".connection-state")?.textContent ?? "";
      const health = document.querySelector(".health-row") !== null;
      const hotbar = document.querySelector(".inventory-grid") !== null;
      const matchCode = document.querySelector(".match-code")?.textContent ?? "";
      const heading = document.querySelector("h1")?.textContent ?? "";
      const notice = document.querySelector("[data-notice], .notice")?.textContent ?? "";
      return {
        banner,
        connection: connection.trim(),
        health,
        hotbar,
        matchCode: matchCode.trim(),
        heading: heading.trim(),
        bodySnippet: body.replace(/\s+/g, " ").slice(0, 240),
        notice: notice.trim(),
      };
    });
    last = JSON.stringify(probe);
    // 局内：有生命条 + 热栏，且不再「同步权威状态」
    if (probe.health && probe.hotbar) {
      console.log(`[${label}] IN-MATCH`, probe.connection, probe.matchCode);
      return probe;
    }
    // 失败路径
    if (
      probe.bodySnippet.includes("DEV 多人登录失败") ||
      probe.bodySnippet.includes("游戏服务暂不可用")
    ) {
      throw new Error(`${label} failed early: ${probe.bodySnippet}`);
    }
    await page.waitForTimeout(500);
  }
  throw new Error(`${label} never entered match HUD. last=${last}`);
}

async function main() {
  mkdirSync(OUT_DIR, { recursive: true });
  console.log("origin", ORIGIN, "out", OUT_DIR);

  const browser = await chromium.launch({ headless: true });
  try {
    const ctxA = await browser.newContext();
    const ctxB = await browser.newContext();
    const pageA = await ctxA.newPage();
    const pageB = await ctxB.newPage();

    // 交错打开，给 seat0 先入队再 seat1 成局
    await pageA.goto(seatUrl(0), { waitUntil: "domcontentloaded" });
    await pageA.waitForTimeout(1_200);
    await pageB.goto(seatUrl(1), { waitUntil: "domcontentloaded" });

    const [stateA, stateB] = await Promise.all([
      waitForConnected(pageA, "A"),
      waitForConnected(pageB, "B"),
    ]);

    await pageA.screenshot({
      path: join(OUT_DIR, "seat0.png"),
      fullPage: true,
    });
    await pageB.screenshot({
      path: join(OUT_DIR, "seat1.png"),
      fullPage: true,
    });

    // 粗检：两边都在局内；match code 可不同展示格式但都应有
    if (!stateA.health || !stateB.health) {
      throw new Error("missing health HUD");
    }

    console.log("\n=== BROWSER SMOKE PASS ===");
    console.log("A", stateA.banner, stateA.connection, stateA.matchCode);
    console.log("B", stateB.banner, stateB.connection, stateB.matchCode);
    console.log("screenshots", OUT_DIR);
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error("BROWSER SMOKE FAIL", err);
  process.exit(1);
});
