import "./styles.css";

import { SinglePlayerController } from "./controller";

export function startSinglePlayerClient(root: HTMLElement): void {
  document.body.classList.add("single-player-mode");
  const controller = new SinglePlayerController(root);
  controller.start();
  window.addEventListener("beforeunload", () => controller.dispose(), {
    once: true,
  });
}
