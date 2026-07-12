import type {
  ExtractionManifest,
  ResourceDefinition,
} from "../../../contracts/extraction/v1/typescript";
import logoUrl from "../../../examples/client/src/assets/logo-circle.png";

import { fetchExtractionManifest } from "./api";
import "./styles.css";

const root = getRoot();

root.innerHTML = `
  <section class="lobby-shell" aria-labelledby="product-name">
    <header class="product-header">
      <img class="product-mark" src="${logoUrl}" alt="" width="56" height="56" />
      <div>
        <p class="product-label">VOXELIZE</p>
        <h1 id="product-name">Voxel Extraction</h1>
      </div>
    </header>
    <div class="status-row" aria-live="polite">
      <span class="status-indicator" data-status-indicator></span>
      <span data-status-text>正在连接服务</span>
      <button type="button" data-retry hidden>重试</button>
    </div>
    <dl class="version-grid" data-versions hidden></dl>
    <div class="resource-list" data-resources hidden></div>
  </section>
`;

const statusText = requiredElement<HTMLElement>("[data-status-text]");
const statusIndicator = requiredElement<HTMLElement>("[data-status-indicator]");
const retryButton = requiredElement<HTMLButtonElement>("[data-retry]");
const versions = requiredElement<HTMLDListElement>("[data-versions]");
const resources = requiredElement<HTMLElement>("[data-resources]");

retryButton.addEventListener("click", () => void refreshBootstrap());
void refreshBootstrap();

async function refreshBootstrap(): Promise<void> {
  setStatus("loading", "正在连接服务");
  retryButton.hidden = true;

  try {
    const manifest = await fetchExtractionManifest();
    renderVersions(manifest);
    renderResources(manifest.resources);
    versions.hidden = false;
    resources.hidden = false;
    setStatus("ready", "服务已就绪");
  } catch {
    versions.hidden = true;
    resources.hidden = true;
    retryButton.hidden = false;
    setStatus("error", "服务暂不可用");
  }
}

function renderVersions(manifest: ExtractionManifest): void {
  const entries = [
    ["协议", `v${manifest.protocolVersion}`],
    ["玩法", manifest.gameplayVersion],
    ["配置", manifest.configVersion],
  ] as const;

  versions.replaceChildren(
    ...entries.map(([term, description]) => {
      const row = document.createElement("div");
      const label = document.createElement("dt");
      const value = document.createElement("dd");
      label.textContent = term;
      value.textContent = description;
      row.append(label, value);
      return row;
    }),
  );
}

function renderResources(definitions: ResourceDefinition[]): void {
  resources.replaceChildren(
    ...definitions.map((resource) => {
      const item = document.createElement("div");
      item.className = "resource-item";

      const swatch = document.createElement("span");
      swatch.className = `resource-swatch resource-${resource.key}`;
      swatch.setAttribute("aria-hidden", "true");

      const label = document.createElement("span");
      label.textContent = resource.key;

      const stack = document.createElement("span");
      stack.className = "resource-stack";
      stack.textContent = `x${resource.maxStack}`;

      item.append(swatch, label, stack);
      return item;
    }),
  );
}

function setStatus(state: "loading" | "ready" | "error", text: string): void {
  statusIndicator.dataset.state = state;
  statusText.textContent = text;
}

function requiredElement<T extends Element>(selector: string): T {
  const element = root.querySelector<T>(selector);
  if (element === null) {
    throw new Error(`缺少界面节点 ${selector}`);
  }
  return element;
}

function getRoot(): HTMLElement {
  const element = document.querySelector<HTMLElement>("#app");
  if (element === null) {
    throw new Error("缺少应用根节点 #app");
  }
  return element;
}
