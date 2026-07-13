export function waitForChildExit(child, { label, timeoutMs = null } = {}) {
  return new Promise((resolve, reject) => {
    const timer =
      timeoutMs === null
        ? null
        : setTimeout(
            () => finish(new Error(`${label}超过 ${timeoutMs}ms 截止时间`)),
            timeoutMs,
          );
    const onError = (error) => finish(error);
    const onExit = (code, signal) => finish(null, { code, signal });
    const finish = (error, result) => {
      if (timer !== null) clearTimeout(timer);
      child.off("error", onError);
      child.off("exit", onExit);
      if (error !== null) reject(error);
      else resolve(result);
    };
    child.once("error", onError);
    child.once("exit", onExit);
  });
}

export function isChildRunning(child) {
  return (
    child.pid !== undefined &&
    child.exitCode === null &&
    child.signalCode === null
  );
}
