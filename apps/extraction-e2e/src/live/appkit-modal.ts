import type { Page, TestInfo } from "@playwright/test";

const PRODUCT_SHELL = '[data-product-shell][data-screen="unauthenticated"]';
const CONNECT_WALLET = '[data-action="connect-wallet"]';
const WALLETCONNECT_SELECTOR = '[data-testid="wallet-selector-walletconnect"]';
const WALLETCONNECT_QR = '[data-testid="wui-qr-code"]';
const SIWE_SIGN = '[data-testid="w3m-connecting-siwe-sign"]';

export async function openProductWalletConnect(
  page: Page,
  testInfo: TestInfo,
  timeoutMs: number,
): Promise<string> {
  requireTimeout(timeoutMs);
  await page.locator(PRODUCT_SHELL).waitFor({
    state: "attached",
    timeout: timeoutMs,
  });
  await page.locator(CONNECT_WALLET).click({ timeout: timeoutMs });

  const walletConnect = page.locator(WALLETCONNECT_SELECTOR);
  await walletConnect.waitFor({ state: "visible", timeout: timeoutMs });
  await attachSafeScreenshot(testInfo, page, "appkit-wallet-selection");
  await walletConnect.click({ timeout: timeoutMs });

  const qrCode = page.locator(WALLETCONNECT_QR);
  await qrCode.waitFor({ state: "visible", timeout: timeoutMs });
  const uri = await qrCode.evaluate((element) => {
    const property = (element as HTMLElement & { uri?: unknown }).uri;
    return typeof property === "string"
      ? property
      : element.getAttribute("uri");
  });
  if (typeof uri !== "string" || !isWalletConnectV2Uri(uri)) {
    throw new Error("AppKit 未通过产品 WalletConnect UI 生成有效配对 URI");
  }
  return uri;
}

export async function approveProductSiwe(
  page: Page,
  testInfo: TestInfo,
  timeoutMs: number,
): Promise<void> {
  requireTimeout(timeoutMs);
  const sign = page.locator(SIWE_SIGN);
  await sign.waitFor({ state: "visible", timeout: timeoutMs });

  // 配对 URI 包含对称密钥；确认二维码组件已经卸载后才允许截图。
  await page.locator(WALLETCONNECT_QR).waitFor({
    state: "detached",
    timeout: timeoutMs,
  });
  await attachSafeScreenshot(testInfo, page, "appkit-siwe-consent");
  await sign.click({ timeout: timeoutMs });
}

export async function attachSafeScreenshot(
  testInfo: TestInfo,
  page: Page,
  name: string,
): Promise<void> {
  if (await page.evaluate(domContainsWalletConnectUri)) {
    throw new Error("安全截图被拒绝：页面仍包含 WalletConnect 配对 URI");
  }
  await testInfo.attach(name, {
    body: await page.screenshot({ animations: "disabled", fullPage: true }),
    contentType: "image/png",
  });
}

function isWalletConnectV2Uri(value: string): boolean {
  if (!/^wc:[^@\s]+@2\?/u.test(value)) return false;
  const query = value.slice(value.indexOf("?") + 1);
  const parameters = new URLSearchParams(query);
  return (
    parameters.get("relay-protocol") === "irn" &&
    (parameters.get("symKey")?.length ?? 0) > 0
  );
}

function domContainsWalletConnectUri(): boolean {
  const roots: Array<Document | ShadowRoot> = [document];
  while (roots.length > 0) {
    const root = roots.pop();
    if (root === undefined) break;
    if (root.textContent?.includes("wc:") === true) return true;
    for (const element of Array.from(root.querySelectorAll("*"))) {
      for (const attribute of element.getAttributeNames()) {
        if (element.getAttribute(attribute)?.includes("wc:") === true) {
          return true;
        }
      }
      const uri = (element as Element & { uri?: unknown }).uri;
      if (typeof uri === "string" && uri.includes("wc:")) return true;
      if (element.shadowRoot !== null) roots.push(element.shadowRoot);
    }
  }
  return false;
}

function requireTimeout(value: number): void {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error("AppKit UI timeout 必须是正安全整数");
  }
}
