import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const stylesDir = dirname(fileURLToPath(import.meta.url));

/** 硬编码的深色面板底（非 token），若出现在产品浅色屏路径则判失败 */
const DARK_PANEL_HARDCODE =
  /background:\s*oklch\(\s*(1[0-9]|2[0-9]|3[0-5])%\s+[^)]*\/\s*(8[0-9]|9[0-9])%/u;

function readStyle(name: string): string {
  return readFileSync(join(stylesDir, name), "utf8");
}

/**
 * 去掉 match 作用域规则块，只检查浅色产品屏仍可能继承的样式。
 * 简化：按行扫描，跳过含 data-screen="match" 的规则及其块。
 */
function stripMatchScopedRules(css: string): string {
  const lines = css.split("\n");
  const kept: string[] = [];
  let depth = 0;
  let skipping = false;
  for (const line of lines) {
    if (!skipping && line.includes('[data-screen="match"]')) {
      skipping = true;
      depth = 0;
    }
    if (skipping) {
      for (const ch of line) {
        if (ch === "{") depth += 1;
        if (ch === "}") depth -= 1;
      }
      if (depth <= 0 && line.includes("}")) {
        skipping = false;
      }
      continue;
    }
    kept.push(line);
  }
  return kept.join("\n");
}

describe("product light theme consistency", () => {
  it("root tokens are light; match screen overrides to dark", () => {
    const tokens = readStyle("tokens.css");
    expect(tokens).toMatch(/color-scheme:\s*light/);
    expect(tokens).toMatch(/--color-void:\s*oklch\(96%/);
    expect(tokens).toMatch(/--color-ink:\s*oklch\(12%/);
    expect(tokens).toMatch(
      /\.game-shell\[data-screen="match"\][\s\S]*--color-void:\s*oklch\(8%/,
    );
  });

  it("access/lobby/queue/result panels use surface tokens not hard-coded dark fills", () => {
    const panels = stripMatchScopedRules(readStyle("panels.css"));
    const overlays = stripMatchScopedRules(readStyle("overlays.css"));
    const shell = stripMatchScopedRules(readStyle("shell.css"));

    for (const [name, css] of [
      ["panels.css", panels],
      ["overlays.css", overlays],
      ["shell.css", shell],
    ] as const) {
      const darkHits = css.match(DARK_PANEL_HARDCODE) ?? [];
      expect(darkHits, `${name} has hard-coded dark panel backgrounds`).toEqual(
        [],
      );
    }

    expect(panels).toContain("background: var(--color-surface)");
    expect(overlays).toContain("background: var(--color-surface)");
    expect(overlays).toMatch(/\.queue-panel[\s\S]*var\(--color-surface\)/);
    expect(overlays).toMatch(/\.result-panel[\s\S]*var\(--color-surface\)/);
  });

  it("queue/result decorative marks use ink (visible on light) not white action fill", () => {
    const overlays = stripMatchScopedRules(readStyle("overlays.css"));
    // 浅色主题 --color-action 为白；雷达/结果图标必须用 ink
    expect(overlays).toMatch(
      /\.radar-mark[\s\S]*?border:\s*1px solid var\(--color-ink\)/,
    );
    expect(overlays).toMatch(
      /\.activity-mark::after[\s\S]*?background:\s*var\(--color-ink\)/,
    );
    expect(overlays).toMatch(
      /\.result-icon[\s\S]*?border:\s*1px solid var\(--color-ink\)/,
    );
    expect(overlays).toMatch(/\.result-icon[\s\S]*?color:\s*var\(--color-ink\)/);
  });

  it("primary CTA keeps white fill / black ink tokens for light hero buttons", () => {
    const tokens = readStyle("tokens.css");
    // 默认（非 match）主 CTA：白底黑字
    const rootBlock = tokens.split('.game-shell[data-screen="match"]')[0] ?? "";
    expect(rootBlock).toMatch(/--color-action:\s*oklch\(100%/);
    expect(rootBlock).toMatch(/--color-action-ink:\s*oklch\(12%/);
    expect(rootBlock).toMatch(/--color-action-border:\s*oklch\(12%/);
  });
});
