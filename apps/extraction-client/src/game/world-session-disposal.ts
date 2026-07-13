export type WorldSessionCleanup = () => void;

/**
 * 资源释放必须尽力执行完全部步骤，避免一个非关键清理错误阻断 Worker 终止。
 */
export function runWorldSessionCleanup(
  steps: readonly WorldSessionCleanup[],
): unknown | null {
  let firstError: unknown | null = null;
  for (const step of steps) {
    try {
      step();
    } catch (error) {
      firstError ??= error;
    }
  }
  return firstError;
}
