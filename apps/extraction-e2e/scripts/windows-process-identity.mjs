import { execFile } from "node:child_process";
import { fileURLToPath } from "node:url";

import { isWindowsCreationId } from "./windows-process-snapshot.mjs";

const IDENTITY_TERMINATION_TIMEOUT_MS = 5_000;
const TERMINATOR_PATH = fileURLToPath(
  new URL("./terminate-windows-process.ps1", import.meta.url),
);
const SUCCESS_STATUSES = new Set(["gone", "reused", "terminated"]);

export function terminateWindowsProcessIdentity(
  process,
  {
    clearTimer = clearTimeout,
    deadline = Date.now() + IDENTITY_TERMINATION_TIMEOUT_MS,
    execFileProcess = execFile,
    now = Date.now,
    setTimer = setTimeout,
  } = {},
) {
  requireProcessIdentity(process);
  const timeoutMs = remainingMilliseconds(deadline, now);
  return new Promise((resolve, reject) => {
    let helper = null;
    let settled = false;
    let timer = null;
    const finish = (error, status) => {
      if (settled) return;
      settled = true;
      if (timer !== null) clearTimer(timer);
      if (error !== null) reject(error);
      else resolve(status);
    };
    const onResult = (error, stdout) => {
      if (error !== null) {
        finish(
          new Error(`PowerShell 无法精确终止 Windows 进程 ${process.pid}`, {
            cause: error,
          }),
        );
        return;
      }
      const status = stdout.trim();
      if (!SUCCESS_STATUSES.has(status)) {
        finish(
          new Error(
            `PowerShell 精确终止 Windows 进程 ${process.pid} 返回未知状态：${status}`,
          ),
        );
        return;
      }
      finish(null, status);
    };

    try {
      helper = execFileProcess(
        "powershell.exe",
        [
          "-NoProfile",
          "-NonInteractive",
          "-File",
          TERMINATOR_PATH,
          "-OwnedPid",
          String(process.pid),
          "-ExpectedCreationId",
          process.creationId,
        ],
        {
          encoding: "utf8",
          killSignal: "SIGKILL",
          maxBuffer: 1024 * 1024,
          timeout: timeoutMs,
          windowsHide: true,
        },
        onResult,
      );
    } catch (error) {
      finish(
        new Error(`无法启动 PowerShell 精确终止 Windows 进程 ${process.pid}`, {
          cause: error,
        }),
      );
      return;
    }
    if (!settled) {
      timer = setTimer(() => {
        let killError = null;
        try {
          helper?.kill();
        } catch (error) {
          killError = error;
        }
        finish(
          new Error(`PowerShell 精确终止 Windows 进程 ${process.pid} 超时`, {
            cause: killError,
          }),
        );
      }, timeoutMs);
    }
  });
}

function requireProcessIdentity(process) {
  if (!Number.isSafeInteger(process.pid) || process.pid <= 1) {
    throw new Error("pid 必须是大于 1 的安全整数");
  }
  if (!isWindowsCreationId(process.creationId)) {
    throw new Error("creationId 必须是非空且不含空白的字符串");
  }
}

function remainingMilliseconds(deadline, now) {
  const remaining = deadline - now();
  if (!Number.isFinite(remaining)) throw new Error("deadline 必须是有限数值");
  if (remaining <= 0) throw new Error("PowerShell 精确终止超过截止时间");
  return Math.ceil(remaining);
}
