export async function settleConcurrentPhase<T>(
  scope: string,
  operations: Array<Promise<T>>,
): Promise<T[]> {
  const settled = await Promise.allSettled(operations);
  const rejected = settled.find(
    (result): result is PromiseRejectedResult => result.status === "rejected",
  );
  if (rejected !== undefined) {
    const cause = rejected.reason;
    const detail = cause instanceof Error ? cause.message : String(cause);
    throw new Error(`${scope} failed: ${detail}`, { cause });
  }
  return settled.map((result) => (result as PromiseFulfilledResult<T>).value);
}
