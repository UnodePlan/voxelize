export type ProductCommand = () => Promise<void> | void;

export interface ProductCommands {
  commands: Record<string, ProductCommand>;
  onBusy(operation: string | null): void;
  onError(operation: string): void;
}

export function bindProductCommands(
  root: HTMLElement,
  options: ProductCommands,
): void {
  root.addEventListener("click", (event) => {
    const target =
      event.target instanceof Element
        ? event.target.closest<HTMLElement>("[data-action]")
        : null;
    const action = target?.dataset.action;
    if (
      action === undefined ||
      target?.hasAttribute("disabled") === true ||
      options.commands[action] === undefined
    ) {
      return;
    }
    void runCommand(action, options);
  });
}

export function commandError(operation: string): string {
  return operation.includes("wallet")
    ? "钱包操作未完成"
    : "操作未完成，请稍后重试";
}

async function runCommand(
  action: string,
  options: ProductCommands,
): Promise<void> {
  options.onBusy(action);
  try {
    await options.commands[action]();
  } catch {
    options.onError(action);
  } finally {
    options.onBusy(null);
  }
}
