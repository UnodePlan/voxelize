import { startE2eClient } from "./e2e-main";

/**
 * 开发专用单机入口：复用本地场景和 HUD，不创建 HTTP、WebSocket 或钱包会话。
 * 所有状态只存在于当前页面，刷新后即丢弃。
 */
export function startSinglePlayerClient(root: HTMLElement): void {
  const parameters = new URLSearchParams(window.location.search);
  parameters.set("single", "1");
  parameters.set("screen", "match");
  const previous = window.history.replaceState;
  previous.call(window.history, null, "", `?${parameters.toString()}`);
  startE2eClient(root);
}
