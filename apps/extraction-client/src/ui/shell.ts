import logoUrl from "../../../../examples/client/src/assets/logo-circle.png";

export interface ProductShell {
  canvas: HTMLCanvasElement;
  content: HTMLElement;
  hud: HTMLElement;
  account: HTMLElement;
  notice: HTMLElement;
  shell: HTMLElement;
}

export function mountProductShell(root: HTMLElement): ProductShell {
  root.innerHTML = `
    <main class="game-shell" data-product-shell data-screen="booting">
      <canvas class="world-canvas" data-world-canvas aria-label="体素世界场景"></canvas>
      <div class="scene-shade" aria-hidden="true"></div>
      <header class="game-topbar">
        <div class="product-identity">
          <img src="${logoUrl}" alt="" width="36" height="36" />
          <div>
            <span>VOXELIZE</span>
            <strong>Voxel Extraction</strong>
          </div>
        </div>
        <div class="account-controls" data-account-controls></div>
      </header>
      <section class="screen-content" data-screen-content aria-live="polite"></section>
      <section class="hud-layer" data-hud-layer hidden></section>
      <div class="notice-bar" data-notice-bar role="status" hidden></div>
    </main>
  `;

  const shell = required<HTMLElement>(root, "[data-product-shell]");
  return {
    shell,
    canvas: required<HTMLCanvasElement>(root, "[data-world-canvas]"),
    content: required<HTMLElement>(root, "[data-screen-content]"),
    hud: required<HTMLElement>(root, "[data-hud-layer]"),
    account: required<HTMLElement>(root, "[data-account-controls]"),
    notice: required<HTMLElement>(root, "[data-notice-bar]"),
  };
}

function required<T extends Element>(root: Element, selector: string): T {
  const element = root.querySelector<T>(selector);
  if (element === null) {
    throw new Error(`缺少界面节点 ${selector}`);
  }
  return element;
}
