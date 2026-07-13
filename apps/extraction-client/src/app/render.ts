import type { VoxelBackdrop } from "../game/scene";
import type { ProductShell } from "../ui/shell";
import { renderProductView } from "../ui/view";

import type { AppState } from "./state";

export function renderControllerState(
  state: AppState,
  elements: ProductShell,
  scene: VoxelBackdrop,
): void {
  renderProductView(state, elements);
  scene.setMode(
    state.screen === "match"
      ? "match"
      : state.screen === "result"
        ? "result"
        : "lobby",
  );
}
