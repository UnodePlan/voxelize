export type SettlementCrashPoint = "after_commit" | "before_commit";

export interface LiveServerOptions {
  databaseUrl: string;
  publicOrigin: string;
  serverOrigin: string;
  settlementCrashPoint?: SettlementCrashPoint;
}

export function validateLiveServerOptions(options: LiveServerOptions): void {
  for (const [name, value] of [
    ["publicOrigin", options.publicOrigin],
    ["serverOrigin", options.serverOrigin],
  ] as const) {
    const url = new URL(value);
    if (url.protocol !== "http:" || url.pathname !== "/") {
      throw new Error(`${name} 必须是本机 HTTP origin`);
    }
    if (url.hostname !== "127.0.0.1" && url.hostname !== "localhost") {
      throw new Error(`${name} 只能绑定本机`);
    }
  }
  const database = new URL(options.databaseUrl);
  if (
    !["postgres:", "postgresql:"].includes(database.protocol) ||
    !["127.0.0.1", "localhost"].includes(database.hostname) ||
    database.pathname !== "/voxelize_extraction_e2e"
  ) {
    throw new Error("真实 E2E 只能使用本机 voxelize_extraction_e2e 数据库");
  }
}

export function bindAddress(origin: string): string {
  const url = new URL(origin);
  if (url.port === "") throw new Error("serverOrigin 必须显式指定端口");
  return `${url.hostname}:${url.port}`;
}
