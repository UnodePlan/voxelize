import { execFile } from "node:child_process";

const PROCESS_SNAPSHOT_TIMEOUT_MS = 2_000;

export function parseWindowsProcessTable(output) {
  if (typeof output !== "string") throw new TypeError("进程表输出必须是字符串");
  const processes = [];
  const seen = new Set();
  for (const [index, line] of output.split(/\r?\n/u).entries()) {
    const value = line.trim();
    if (value === "") continue;
    const columns = value.split(/\s+/u);
    const pid = Number(columns[0]);
    const parentPid = Number(columns[1]);
    const creationId = columns[2];
    if (
      columns.length !== 3 ||
      !Number.isSafeInteger(pid) ||
      !Number.isSafeInteger(parentPid) ||
      pid <= 1 ||
      parentPid < 0 ||
      !isWindowsCreationId(creationId) ||
      seen.has(pid)
    ) {
      throw new Error(`无法解析 Windows 进程表第 ${index + 1} 行`);
    }
    seen.add(pid);
    processes.push({ creationId, parentPid, pid });
  }
  return processes;
}

export function readWindowsProcessTable() {
  return new Promise((resolve, reject) => {
    execFile(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -gt 1 } | ForEach-Object { '{0} {1} {2}' -f $_.ProcessId, $_.ParentProcessId, $_.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture) }",
      ],
      {
        encoding: "utf8",
        maxBuffer: 16 * 1024 * 1024,
        timeout: PROCESS_SNAPSHOT_TIMEOUT_MS,
        windowsHide: true,
      },
      (error, stdout) => {
        if (error !== null) reject(error);
        else resolve(stdout);
      },
    );
  });
}

export function windowsProcessIdentityKey({ creationId, pid }) {
  return `${pid}:${creationId}`;
}

export function isWindowsCreationId(value) {
  return typeof value === "string" && /^\S+$/u.test(value);
}
