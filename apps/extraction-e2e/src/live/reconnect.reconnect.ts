import { afterAll, beforeAll, describe, expect, test } from "vitest";

import { loadLiveE2eConfig } from "./config";
import { inventoryResourceCount } from "./gameplay-state";
import { runLiveReconnectAcceptance } from "./reconnect-scenario";
import { ControlledLiveServer } from "./server-process";

const SERVER_ORIGIN = "http://127.0.0.1:4214";
const PUBLIC_ORIGIN = "http://127.0.0.1:5214";
const DEFAULT_DATABASE_URL = "postgres://localhost/voxelize_extraction_e2e";
const SETTLEMENT_CRASH_ENV = "EXTRACTION_E2E_SETTLEMENT_CRASH";

describe.sequential("真实断线重连门禁", () => {
  let server: ControlledLiveServer;

  beforeAll(async () => {
    if ((process.env[SETTLEMENT_CRASH_ENV] ?? "") !== "") {
      throw new Error("重连门禁禁止启用结算 crash 注入");
    }
    server = await ControlledLiveServer.start({
      databaseUrl: process.env.DATABASE_URL ?? DEFAULT_DATABASE_URL,
      publicOrigin: PUBLIC_ORIGIN,
      serverOrigin: SERVER_ORIGIN,
    });
  });

  afterAll(async () => {
    await server?.stop();
  });

  test("同账号恢复且断线死亡与超时各自产生唯一终态", async () => {
    const config = loadLiveE2eConfig({
      ...process.env,
      EXTRACTION_E2E_CLIENT_URL: PUBLIC_ORIGIN,
      EXTRACTION_E2E_PUBLIC_ORIGIN: PUBLIC_ORIGIN,
      EXTRACTION_E2E_SERVER_URL: SERVER_ORIGIN,
      EXTRACTION_E2E_TIMEOUT_MS: "180000",
    });
    const walletOffsetBase = Date.now() * 100 + (process.pid % 100);
    const evidence = await runLiveReconnectAcceptance(
      config,
      server,
      walletOffsetBase,
    );

    expect(evidence.recovery.after.health.data.currentHalfHearts).toBe(18);
    expect(evidence.recovery.before.health.data.currentHalfHearts).toBe(18);
    expect(inventoryResourceCount(evidence.recovery.after, "dirt")).toBe(1);
    expect(inventoryResourceCount(evidence.recovery.before, "dirt")).toBe(1);
    expect(["MATCH_FULL", "MATCH_ROSTER_LOCKED"]).toContain(
      evidence.recovery.takeoverRejection,
    );

    expect(evidence.disconnectedKill.result).toMatchObject({
      status: "dead",
      terminalCause: "melee",
    });
    expect(evidence.disconnectedKill.lateAttack.resolution).toBe("miss");
    expect(evidence.timeout.result).toMatchObject({
      status: "timedOut",
      terminalCause: "reconnectTimeout",
    });
    expect(evidence.timeout.lateAttack.resolution).toBe("miss");
    expect(["MATCH_FULL", "MATCH_ROSTER_LOCKED"]).toContain(
      evidence.timeout.lateJoinRejection,
    );
  });
});
