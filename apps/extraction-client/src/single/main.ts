import "./styles.css";

import { SinglePlayerController } from "./controller";

declare global {
  interface Window {
    /** 单机浏览器调试：give / selectSlot / held */
    __singleDebug?: {
      give: (resource: string, quantity?: number) => void;
      selectSlot: (slot: number) => void;
      held: () => unknown;
    };
  }
}

export function startSinglePlayerClient(root: HTMLElement): void {
  document.body.classList.add("single-player-mode");
  const controller = new SinglePlayerController(root);
  controller.start();
  // 自动化验收 / 本地调试入口
  window.__singleDebug = {
    give: (resource, quantity) => {
      controller.debugGive(resource as never, quantity);
    },
    selectSlot: (slot) => controller.debugSelectSlot(slot),
    held: () => controller.debugHeldSnapshot(),
  };
  window.addEventListener(
    "beforeunload",
    () => {
      controller.dispose();
      delete window.__singleDebug;
    },
    { once: true },
  );
}
