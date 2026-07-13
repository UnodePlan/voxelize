import type {
  ExtractionStateData,
  GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import type { AppState } from "../app/state";
import { projectHearts } from "../combat-state";

import { escapeHtml, formatDuration } from "./html";

export function renderHud(state: AppState, now = Date.now()): string {
  const gameplay = state.gameplay;
  const extraction = gameplay?.extraction.data;

  return `
    <div class="match-status-strip">
      <div class="connection-state" data-connection="${state.connection}">
        <span></span>${connectionLabel(state.connection)}
      </div>
      <div class="phase-clock">
        <i data-lucide="clock-3"></i>
        <span>${phaseLabel(extraction?.status)}</span>
        <strong>${phaseTime(extraction, now)}</strong>
      </div>
      <div class="match-code">${escapeHtml(shortMatchId(state.activeMatchId))}</div>
    </div>
    <div class="crosshair" aria-hidden="true"><span></span><span></span></div>
    ${renderProgress(state)}
    ${gameplay === null ? renderUnknownRuntime() : renderPlayerHud(gameplay)}
  `;
}

function renderPlayerHud(gameplay: GameplayStateData): string {
  const hearts = projectHearts({
    health: gameplay.health,
    deathResult: gameplay.deathResult,
  });
  const halfHearts = gameplay.health.data.currentHalfHearts;
  return `
    <div class="player-hud">
      <div class="health-row" role="img" aria-label="生命值 ${halfHearts}/20">
        ${hearts.map((fill) => renderHeart(fill)).join("")}
      </div>
      <div class="loadout-row">
        <div class="equipment-slot" title="基础镐"><i data-lucide="pickaxe"></i><span>镐</span></div>
        <div class="equipment-slot" title="近战武器"><i data-lucide="swords"></i><span>近战</span></div>
        <div class="inventory-grid" aria-label="12 格背包">
          ${gameplay.inventory.slots.map((slot, index) => renderSlot(slot, index)).join("")}
        </div>
      </div>
    </div>
  `;
}

function renderUnknownRuntime(): string {
  return `
    <div class="player-hud player-hud-loading" aria-busy="true">
      <span class="activity-mark"></span><span>同步权威状态</span>
    </div>
  `;
}

function renderHeart(fill: "full" | "half" | "empty"): string {
  const width = fill === "full" ? "100%" : fill === "half" ? "50%" : "0%";
  return `
    <span class="heart" data-fill="${fill}" aria-hidden="true">
      <i data-lucide="heart" class="heart-outline"></i>
      <span class="heart-fill" style="width:${width}"><i data-lucide="heart"></i></span>
    </span>
  `;
}

function renderSlot(
  slot: { resource: "dirt" | "gold" | "diamond"; quantity: number } | null,
  index: number,
): string {
  if (slot === null) {
    return `<div class="inventory-slot" aria-label="背包格 ${index + 1}，空"></div>`;
  }
  const label = { dirt: "泥土", gold: "黄金", diamond: "钻石" }[slot.resource];
  return `
    <div class="inventory-slot" data-resource="${slot.resource}" aria-label="背包格 ${index + 1}，${label} ${slot.quantity}">
      <span class="resource-cube"></span><strong>${slot.quantity}</strong>
    </div>
  `;
}

function renderProgress(state: AppState): string {
  const mining = state.gameplay?.mining.data;
  const extraction = state.gameplay?.extraction.data;
  if (extraction?.status === "open" && extraction.inside) {
    const ratio = Math.min(1, extraction.elapsedMs / extraction.requiredMs);
    return progressMarkup("正在撤离", ratio, "shield-check");
  }
  if (extraction?.status === "pending") {
    return progressMarkup("正在核对结算", 1, "refresh-cw");
  }
  if (mining?.status === "mining") {
    const ratio = Math.min(1, mining.elapsedMs / mining.requiredMs);
    return progressMarkup("正在挖掘", ratio, "pickaxe");
  }
  return "";
}

function progressMarkup(label: string, ratio: number, icon: string): string {
  return `
    <div class="action-progress" role="progressbar" aria-label="${label}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(ratio * 100)}">
      <div><i data-lucide="${icon}"></i><span>${label}</span><strong>${Math.round(ratio * 100)}%</strong></div>
      <span class="progress-track"><span style="width:${ratio * 100}%"></span></span>
    </div>
  `;
}

function phaseLabel(status: string | undefined): string {
  switch (status) {
    case undefined:
      return "同步中";
    case "hidden":
      return "搜集阶段";
    case "open":
      return "撤离开放";
    case "pending":
      return "结算中";
    case "closed":
      return "撤离关闭";
    default:
      return "同步中";
  }
}

function phaseTime(
  extraction: ExtractionStateData | undefined,
  now: number,
): string {
  if (extraction?.status === "hidden") {
    return formatDuration(extraction.extractionOpenAtUnixSeconds * 1000 - now);
  }
  if (extraction?.status === "open") {
    return formatDuration(extraction.hardDeadlineUnixSeconds * 1000 - now);
  }
  return "--:--";
}

function connectionLabel(connection: AppState["connection"]): string {
  return {
    offline: "离线",
    connecting: "连接中",
    online: "已连接",
    reconnecting: "重连中",
  }[connection];
}

function shortMatchId(matchId: string | null): string {
  return matchId === null
    ? "MATCH ----"
    : `MATCH ${matchId.slice(0, 8).toUpperCase()}`;
}
