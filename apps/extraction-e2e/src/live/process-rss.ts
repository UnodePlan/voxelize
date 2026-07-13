import { execFile } from "node:child_process";

const KIBIBYTE = 1_024;

/** 返回整个受控进程组的 RSS；Windows 缺少同等 ps 接口时明确返回 null。 */
export async function sampleProcessGroupRssBytes(
  processGroupId: number,
): Promise<number | null> {
  if (process.platform === "win32") return null;
  if (!Number.isSafeInteger(processGroupId) || processGroupId <= 0) {
    throw new Error("process group id must be a positive safe integer");
  }
  const table = await readProcessTable();
  let rssKib = 0;
  let matched = 0;
  for (const line of table.split("\n")) {
    const fields = /^\s*(\d+)\s+(\d+)\s+(\d+)\s*$/u.exec(line);
    if (fields === null || Number(fields[2]) !== processGroupId) continue;
    rssKib += Number(fields[3]);
    matched += 1;
  }
  if (matched === 0 || !Number.isSafeInteger(rssKib)) {
    throw new Error(`process group ${processGroupId} is absent from ps output`);
  }
  const bytes = rssKib * KIBIBYTE;
  if (!Number.isSafeInteger(bytes)) {
    throw new Error("process group RSS exceeds JavaScript safe integer range");
  }
  return bytes;
}

function readProcessTable(): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(
      "ps",
      ["-axo", "pid=,pgid=,rss="],
      { encoding: "utf8", maxBuffer: 4 * 1_024 * 1_024 },
      (error, stdout) => {
        if (error !== null) {
          reject(
            new Error("failed to sample process group RSS", { cause: error }),
          );
          return;
        }
        resolve(stdout);
      },
    );
  });
}
