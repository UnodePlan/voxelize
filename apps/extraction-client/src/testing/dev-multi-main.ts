/**
 * DEV 多人入口：`?mode=dev-mp&seat=0` / `seat=1`
 * 自动 SIWE + 假钱包，自动点「加入匹配」，专注局内多人表现联调。
 */

import { ProductController } from "../app/controller";

import {
  authenticateDevSeat,
  createDevConnectedWallet,
  parseDevMpSeat,
} from "./dev-multi-auth";

export async function startDevMultiplayerClient(
  root: HTMLElement,
): Promise<void> {
  document.body.classList.add("dev-multiplayer-mode");
  const seat = parseDevMpSeat();
  const banner = document.createElement("div");
  banner.className = "dev-mp-banner";
  banner.textContent = `DEV 多人 · seat ${seat} · 无钱包 UI`;
  document.body.prepend(banner);

  let address: string;
  try {
    address = await authenticateDevSeat(seat);
  } catch (error) {
    root.textContent = `DEV 多人登录失败：${error instanceof Error ? error.message : String(error)}。请确认 extraction-server 已启动且 DEV 成局已开。`;
    return;
  }

  const controller = new ProductController(root, async () =>
    createDevConnectedWallet(address),
  );
  await controller.start();

  // 稍等大厅渲染后自动入队
  window.setTimeout(() => {
    const join = document.querySelector<HTMLElement>(
      '[data-action="join-queue"]',
    );
    if (join !== null && !join.hasAttribute("disabled")) {
      join.click();
    }
  }, 800);
}
