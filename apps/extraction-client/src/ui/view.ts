import type { MatchResult, ResourceCounts } from "../api/models";
import type { AppState } from "../app/state";

import { escapeHtml, formatAddress, formatDuration } from "./html";
import { renderHud } from "./hud";
import { hydrateIcons } from "./icons";
import { resultDetail, resultLabel } from "./result-copy";
import type { ProductShell } from "./shell";

const renderedHtml = new WeakMap<HTMLElement, string>();

export function renderProductView(
  state: AppState,
  elements: ProductShell,
): void {
  elements.shell.dataset.screen = state.screen;
  const accountChanged = replaceRenderedHtml(
    elements.account,
    renderAccount(state),
  );
  const contentChanged = replaceRenderedHtml(
    elements.content,
    renderScreen(state),
  );
  elements.hud.hidden = state.screen !== "match";
  const hudChanged = replaceRenderedHtml(
    elements.hud,
    state.screen === "match" ? renderHud(state) : "",
  );
  elements.notice.hidden = state.notice === null;
  elements.notice.textContent = state.notice ?? "";

  if (accountChanged) hydrateIcons(elements.account);
  if (contentChanged) hydrateIcons(elements.content);
  if (hudChanged) hydrateIcons(elements.hud);
}

export function replaceRenderedHtml(
  element: HTMLElement,
  nextHtml: string,
): boolean {
  if (renderedHtml.get(element) === nextHtml) return false;
  element.innerHTML = nextHtml;
  renderedHtml.set(element, nextHtml);
  return true;
}

function renderAccount(state: AppState): string {
  if (state.session === null) {
    return `<span class="network-chip"><i data-lucide="gem"></i>Ethereum Mainnet</span>`;
  }
  return `
    <span class="network-chip"><i data-lucide="shield-check"></i>已认证</span>
    <span class="wallet-address">${formatAddress(state.session.address)}</span>
    <button class="icon-button" type="button" data-action="logout" title="退出登录" aria-label="退出登录" ${disabled(state)}>
      <i data-lucide="log-out"></i>
    </button>
  `;
}

function renderScreen(state: AppState): string {
  switch (state.screen) {
    case "booting":
      return renderBooting();
    case "unauthenticated":
      return renderAuth(state);
    case "lobby":
      return renderLobby(state);
    case "queue":
      return renderQueue(state);
    case "match":
      return renderMatchOverlay(state);
    case "result":
      return state.result === null
        ? renderBooting()
        : renderResult(state.result, state);
  }
}

function renderBooting(): string {
  return `
    <section class="status-panel" aria-busy="true">
      <span class="activity-mark"></span>
      <p>正在连接服务</p>
    </section>
  `;
}

function renderAuth(state: AppState): string {
  const configured = state.wallet.configured;
  const wrongNetwork =
    state.wallet.connected && Number(state.wallet.chainId) !== 1;
  const buttonLabel = wrongNetwork
    ? "切换至 Ethereum Mainnet"
    : state.wallet.connected
      ? "使用钱包签名登录"
      : "连接钱包";
  return `
    <section class="access-panel side-rail" aria-labelledby="access-title">
      <div class="panel-heading">
        <span class="section-kicker">PVP EXTRACTION</span>
        <h1 id="access-title">Voxel Extraction</h1>
        <div class="rule-line">
          <span>10 人</span><span>300 x 300</span><span>12 分钟</span>
        </div>
      </div>
      <div class="auth-identity-row">
        <i data-lucide="wallet"></i>
        <span>身份</span>
        <strong>${authStatus(state)}</strong>
      </div>
      <button class="primary-command primary-command--hero" type="button" data-action="connect-wallet" ${configured ? disabled(state) : "disabled"}>
        <i data-lucide="wallet"></i><span>${buttonLabel}</span>
      </button>
      ${renderSinglePlayerEntry()}
      ${configured ? "" : `<p class="inline-error"><i data-lucide="circle-alert"></i>钱包连接尚未配置</p>`}
      <div class="server-status">
        <span class="status-dot" data-ready="${state.manifest !== null}"></span>
        <span>${state.manifest === null ? "服务不可用" : "游戏服务在线"}</span>
        <button class="text-command" type="button" data-action="retry-bootstrap"><i data-lucide="refresh-cw"></i>刷新</button>
      </div>
    </section>
  `;
}

function renderSinglePlayerEntry(): string {
  if (!import.meta.env.DEV) return "";
  return `
    <a class="secondary-command single-player-entry" href="?mode=single">
      <i data-lucide="gamepad-2"></i><span>进入单机调试</span>
    </a>
    <p class="single-player-note">不连接服务器，不保存资源，刷新即重置</p>
  `;
}

function renderLobby(state: AppState): string {
  return `
    <section class="lobby-panel side-rail" aria-labelledby="lobby-title">
      <div class="panel-heading compact-heading">
        <span class="section-kicker">READY ROOM</span>
        <h1 id="lobby-title">行动大厅</h1>
      </div>
      <div class="lobby-actions">
        <button class="primary-command primary-command--hero" type="button" data-action="join-queue" ${disabled(state)}>
          <i data-lucide="swords"></i><span>开始匹配</span>
        </button>
        <button class="secondary-command" type="button" data-action="refresh-lobby" ${disabled(state)}>
          <i data-lucide="refresh-cw"></i><span>刷新</span>
        </button>
      </div>
      ${renderWarehouse(state)}
      ${renderLatestResult(state)}
    </section>
  `;
}

export function renderWarehouse(state: AppState): string {
  const warehouse = state.warehouse;
  const resources = warehouse?.resources;
  return `
    <section class="warehouse-section" aria-labelledby="warehouse-title">
      <div class="section-title"><h2 id="warehouse-title">永久仓库</h2></div>
      <div class="resource-balances">
        ${resourceBalance("dirt", "泥土", resources?.dirt ?? "--")}
        ${resourceBalance("gold", "黄金", resources?.gold ?? "--")}
        ${resourceBalance("diamond", "钻石", resources?.diamond ?? "--")}
      </div>
      <dl class="warehouse-stats">
        <div><dt>累计撤离资源</dt><dd>${warehouse?.stats.totalResourcesExtracted ?? "--"}</dd></div>
        <div><dt>成功撤离</dt><dd>${warehouse?.stats.successfulExtractions ?? "--"}</dd></div>
        <div><dt>累计价值</dt><dd>${warehouse?.stats.totalExtractionValue ?? "--"}</dd></div>
        <div><dt>单局最高</dt><dd>${warehouse?.stats.highestSingleMatchValue ?? "--"}</dd></div>
      </dl>
    </section>
  `;
}

function renderLatestResult(state: AppState): string {
  if (state.result === null) {
    return `<section class="recent-result"><span>最近行动</span><strong>暂无记录</strong></section>`;
  }
  return `
    <section class="recent-result">
      <div><span>最近行动</span><strong>${resultLabel(state.result.status)}</strong></div>
      <button class="text-command" type="button" data-action="show-result">查看结果</button>
    </section>
  `;
}

function renderQueue(state: AppState): string {
  const queued = state.queue;
  const position =
    queued?.position === undefined ? "--" : String(queued.position);
  return `
    <section class="queue-panel" aria-labelledby="queue-title">
      <span class="radar-mark" aria-hidden="true"><span></span></span>
      <div>
        <span class="section-kicker">MATCHMAKING</span>
        <h1 id="queue-title">正在集结玩家</h1>
        <p class="queue-count"><strong>${position}</strong><span>当前队列位置</span></p>
      </div>
      <button class="secondary-command" type="button" data-action="leave-queue" ${disabled(state)}>
        <i data-lucide="x"></i><span>取消匹配</span>
      </button>
    </section>
  `;
}

function renderMatchOverlay(state: AppState): string {
  if (state.connection === "online") {
    return "";
  }
  return `
    <section class="connection-panel" aria-busy="true">
      <span class="activity-mark"></span>
      <h1>${state.connection === "reconnecting" ? "正在重连对局" : "正在进入对局"}</h1>
      <p>${escapeHtml(state.worldName ?? "等待 World 分配")}</p>
    </section>
  `;
}

function renderResult(result: MatchResult, state: AppState): string {
  const detail = resultDetail(result);
  return `
    <section class="result-panel" data-result="${result.status}" aria-labelledby="result-title">
      <div class="result-heading">
        <span class="result-icon"><i data-lucide="${detail.icon}"></i></span>
        <div><span class="section-kicker">MATCH RESULT</span><h1 id="result-title">${detail.title}</h1><p>${detail.subtitle}</p></div>
      </div>
      ${renderResultResources(result)}
      ${renderResultStats(result)}
      <div class="result-actions">
        <button class="primary-command" type="button" data-action="back-lobby" ${disabled(state)}><i data-lucide="user-round"></i><span>返回大厅</span></button>
        ${result.status === "pendingReconciliation" ? `<button class="secondary-command" type="button" data-action="refresh-result" ${disabled(state)}><i data-lucide="refresh-cw"></i><span>重新核对</span></button>` : ""}
      </div>
    </section>
  `;
}

export function renderResultResources(result: MatchResult): string {
  if (result.status === "pendingReconciliation") {
    return `<div class="settlement-line muted"><i data-lucide="refresh-cw"></i><span>永久入账</span><strong>待确认</strong></div>`;
  }
  if (result.status !== "extracted" || result.settlement === null) {
    return `<div class="settlement-line muted"><i data-lucide="box"></i><span>永久入账</span><strong>0</strong></div>`;
  }
  return `
    <section class="settlement-section">
      <div class="settlement-line"><i data-lucide="check"></i><span>永久入账</span><strong>${result.settlement.totalValue}</strong></div>
      <div class="resource-balances compact-balances">
        ${resourceBalance("dirt", "泥土", result.settlement.resources.dirt)}
        ${resourceBalance("gold", "黄金", result.settlement.resources.gold)}
        ${resourceBalance("diamond", "钻石", result.settlement.resources.diamond)}
      </div>
    </section>
  `;
}

function renderResultStats(result: MatchResult): string {
  const survived =
    result.survivedMs === null ? "--:--" : formatDuration(result.survivedMs);
  return `
    <dl class="result-stats">
      <div><dt>存活时间</dt><dd>${survived}</dd></div>
      <div><dt>挖掘</dt><dd>${sumResources(result.stats.mined)}</dd></div>
      <div><dt>拾取</dt><dd>${sumResources(result.stats.pickedUp)}</dd></div>
      <div><dt>损失</dt><dd>${sumResources(result.stats.lost)}</dd></div>
    </dl>
  `;
}

function resourceBalance(
  resource: keyof ResourceCounts,
  label: string,
  value: number | string,
): string {
  return `<div class="resource-balance" data-resource="${resource}"><span class="resource-cube"></span><span>${label}</span><strong>${value}</strong></div>`;
}

function authStatus(state: AppState): string {
  if (!state.wallet.configured) return "未配置";
  if (!state.wallet.connected) return "未连接";
  if (Number(state.wallet.chainId) !== 1) return "网络不匹配";
  return state.wallet.address === null
    ? "等待钱包"
    : formatAddress(state.wallet.address);
}

function sumResources(resources: ResourceCounts): number {
  return resources.dirt + resources.gold + resources.diamond;
}

function disabled(state: AppState): string {
  return state.busy === null ? "" : 'disabled aria-disabled="true"';
}
