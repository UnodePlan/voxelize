import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { afterAll, describe, expect, test } from "vitest";

import { loadLiveE2eConfig } from "./config";
import {
  mineOneSurfaceDirt,
  moveToHorizontal,
  pollGameplayState,
} from "./gameplay-actions";
import { fetchLiveWarehouse } from "./gameplay-http";
import { prepareGameplayMatch } from "./gameplay-live-actors";
import {
  advanceTo,
  finishExtraction,
  pollMatchResult,
  requireControlledTime,
  requireInside,
  sameWarehouseSnapshot,
} from "./gameplay-scenario-support";
import { ControlledLiveServer } from "./server-process";
import type { SettlementCrashPoint } from "./server-process-config";
import {
  fetchSettlementAudit,
  type SettlementAuditSnapshot,
} from "./settlement-audit";

const SERVER_ORIGIN = "http://127.0.0.1:4213";
const PUBLIC_ORIGIN = "http://127.0.0.1:5213";
const DEFAULT_DATABASE_URL = "postgres://localhost/voxelize_extraction_e2e";
const EXTRACTION_OPENS_AFTER_MS = 480_000;
const ARTIFACT_DIR = loadLiveE2eConfig().artifactDir;
const ARTIFACT_PATH = join(ARTIFACT_DIR, "live-crash-recovery.json");
const artifactCases: unknown[] = [];

afterAll(async () => {
  await mkdir(ARTIFACT_DIR, { recursive: true });
  await writeFile(
    ARTIFACT_PATH,
    `${JSON.stringify(
      {
        schema: "extraction.e2e.crash-recovery.v1",
        createdAt: new Date().toISOString(),
        cases: artifactCases,
      },
      null,
      2,
    )}\n`,
    { encoding: "utf8", flag: "wx" },
  );
});

describe.sequential("结算进程故障与重启幂等", () => {
  test.each([
    ["after_commit", "extracted"],
    ["before_commit", "aborted"],
  ] as const)("%s 后重启收敛为 %s", async (crashPoint, expectedStatus) => {
    const evidence = await runCrashRecovery(crashPoint);
    expect(evidence.result.status).toBe(expectedStatus);
    expect(evidence.firstAudit).toEqual(evidence.secondAudit);
    expect(evidence.firstWarehouse).toEqual(evidence.secondWarehouse);

    if (crashPoint === "after_commit") {
      expect(evidence.result.settlement?.resources).toEqual({
        dirt: 1,
        gold: 0,
        diamond: 0,
      });
      expect(evidence.firstWarehouse.resources.dirt).toBe(
        evidence.beforeWarehouse.resources.dirt + 1,
      );
      expect(evidence.firstWarehouse.stats.successfulExtractions).toBe(
        evidence.beforeWarehouse.stats.successfulExtractions + 1,
      );
      expect(evidence.firstAudit).toMatchObject({
        matchState: "aborted",
        participantState: "extracted",
        settlementCount: 1,
        settlementItemCount: 1,
        ledgerCount: 1,
        settlementResources: { dirt: 1, gold: 0, diamond: 0 },
        ledgerResources: { dirt: 1, gold: 0, diamond: 0 },
      });
    } else {
      expect(evidence.result.settlement).toBeNull();
      expect(
        sameWarehouseSnapshot(
          evidence.beforeWarehouse,
          evidence.firstWarehouse,
        ),
      ).toBe(true);
      expect(evidence.firstAudit).toMatchObject({
        matchState: "aborted",
        participantState: "aborted",
        settlementCount: 0,
        settlementItemCount: 0,
        ledgerCount: 0,
        settlementResources: { dirt: 0, gold: 0, diamond: 0 },
        ledgerResources: { dirt: 0, gold: 0, diamond: 0 },
      });
    }
    artifactCases.push({
      crashPoint,
      matchId: evidence.result.matchId,
      resultStatus: evidence.result.status,
      beforeWarehouse: evidence.beforeWarehouse,
      firstWarehouse: evidence.firstWarehouse,
      secondWarehouse: evidence.secondWarehouse,
      firstAudit: evidence.firstAudit,
      secondAudit: evidence.secondAudit,
    });
  });
});

async function runCrashRecovery(crashPoint: SettlementCrashPoint) {
  const databaseUrl = process.env.DATABASE_URL ?? DEFAULT_DATABASE_URL;
  const config = loadLiveE2eConfig({
    ...process.env,
    EXTRACTION_E2E_CLIENT_URL: PUBLIC_ORIGIN,
    EXTRACTION_E2E_PUBLIC_ORIGIN: PUBLIC_ORIGIN,
    EXTRACTION_E2E_SERVER_URL: SERVER_ORIGIN,
    EXTRACTION_E2E_TIMEOUT_MS: "180000",
  });
  let server: ControlledLiveServer | null = null;
  let match: Awaited<ReturnType<typeof prepareGameplayMatch>> | null = null;
  try {
    server = await startServer(databaseUrl, crashPoint);
    const walletOffset =
      Date.now() * 100 +
      (process.pid % 100) +
      (crashPoint === "after_commit" ? 0 : 10);
    match = await prepareGameplayMatch(config, walletOffset);
    const matchId = match.matchId;
    const player = match.players[0];
    const beforeWarehouse = await fetchLiveWarehouse(player.http);
    const startedAt = requireControlledTime(await server.elapse(0));
    const mined = await mineOneSurfaceDirt(player.driver, server);
    expect(mined.after - mined.before).toBe(1);

    await advanceTo(server, startedAt + EXTRACTION_OPENS_AFTER_MS);
    const open = await pollGameplayState(
      player.driver,
      (state) => state.extraction.data.status === "open",
      "结算故障场景未开放撤离区",
    );
    if (open.extraction.data.status !== "open") throw new Error("unreachable");
    await moveToHorizontal(
      player.driver,
      open.extraction.data.zone.center,
      server,
      1,
    );
    await requireInside(player.driver);
    await finishExtraction(player.driver, server);
    await server.expectSettlementCrash(crashPoint);
    await match.disconnect();
    match = null;

    server = await restartServer(server, databaseUrl);
    const result = await pollMatchResult(
      player,
      matchId,
      crashPoint === "after_commit" ? "extracted" : "aborted",
    );
    expect(result.matchId).toBe(matchId);
    const firstWarehouse = await fetchLiveWarehouse(player.http);
    const firstAudit = requireAudit(
      await fetchSettlementAudit(player.http, matchId),
    );

    server = await restartServer(server, databaseUrl);
    const secondWarehouse = await fetchLiveWarehouse(player.http);
    const secondAudit = requireAudit(
      await fetchSettlementAudit(player.http, matchId),
    );
    return {
      beforeWarehouse,
      firstAudit,
      firstWarehouse,
      result,
      secondAudit,
      secondWarehouse,
    };
  } finally {
    await match?.disconnect();
    await server?.stop();
  }
}

async function startServer(
  databaseUrl: string,
  settlementCrashPoint?: SettlementCrashPoint,
): Promise<ControlledLiveServer> {
  return ControlledLiveServer.start({
    databaseUrl,
    publicOrigin: PUBLIC_ORIGIN,
    serverOrigin: SERVER_ORIGIN,
    settlementCrashPoint,
  });
}

async function restartServer(
  current: ControlledLiveServer,
  databaseUrl: string,
): Promise<ControlledLiveServer> {
  await current.stop();
  return startServer(databaseUrl);
}

function requireAudit(
  value: SettlementAuditSnapshot | null,
): SettlementAuditSnapshot {
  if (value === null) throw new Error("本人结算审计记录在重启后消失");
  return value;
}
