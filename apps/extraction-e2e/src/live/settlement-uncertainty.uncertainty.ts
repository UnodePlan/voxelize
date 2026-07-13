import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { afterAll, describe, expect, test } from "vitest";

import { loadLiveE2eConfig } from "./config";
import {
  mineOneSurfaceDirt,
  moveToHorizontal,
  pollGameplayState,
} from "./gameplay-actions";
import { fetchLiveMatchResult, fetchLiveWarehouse } from "./gameplay-http";
import { prepareGameplayMatch } from "./gameplay-live-actors";
import {
  advanceTo,
  finishExtraction,
  pollMatchResult,
  requireControlledTime,
  requireInside,
  sameWarehouseSnapshot,
} from "./gameplay-scenario-support";
import { LiveApiError } from "./http";
import { ControlledLiveServer } from "./server-process";
import {
  fetchSettlementAudit,
  type SettlementAuditSnapshot,
} from "./settlement-audit";
import {
  applySettlementFault,
  fetchSettlementFaultSnapshot,
  type SettlementFaultSnapshot,
} from "./settlement-fault";

const SERVER_ORIGIN = "http://127.0.0.1:4216";
const PUBLIC_ORIGIN = "http://127.0.0.1:5216";
const DEFAULT_DATABASE_URL = "postgres://localhost/voxelize_extraction_e2e";
const EXTRACTION_OPENS_AFTER_MS = 480_000;
const HARD_DEADLINE_AFTER_MS = 720_000;
const SETTLEMENT_GRACE_MS = 30_000;
const POLL_TIMEOUT_MS = 30_000;
const ARTIFACT_DIR = loadLiveE2eConfig().artifactDir;
const ARTIFACT_PATH = join(ARTIFACT_DIR, "live-settlement-uncertainty.json");
let artifactEvidence: unknown = null;

afterAll(async () => {
  await mkdir(ARTIFACT_DIR, { recursive: true });
  await writeFile(
    ARTIFACT_PATH,
    `${JSON.stringify(
      {
        schema: "extraction.e2e.settlement-uncertainty.v1",
        createdAt: new Date().toISOString(),
        faultModel:
          "e2e-control repository boundary; the PostgreSQL service is never stopped or reconfigured",
        evidence: artifactEvidence,
      },
      null,
      2,
    )}\n`,
    { encoding: "utf8", flag: "wx" },
  );
});

describe.sequential("真实 PostgreSQL COMMIT 结果不确定与宽限核对", () => {
  test("十人局在宽限后只读恢复已提交的唯一结算", async () => {
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
      server = await ControlledLiveServer.start({
        databaseUrl,
        publicOrigin: PUBLIC_ORIGIN,
        serverOrigin: SERVER_ORIGIN,
      });
      match = await prepareGameplayMatch(
        config,
        Date.now() * 100 + (process.pid % 100),
      );
      await match.assertTenSockets();
      const player = match.players[0];
      const beforeWarehouse = await fetchLiveWarehouse(player.http);
      const matchStart = requireControlledTime(await server.elapse(0));
      const mined = await mineOneSurfaceDirt(player.driver, server);
      expect(mined.after - mined.before).toBe(1);

      await advanceTo(server, matchStart + EXTRACTION_OPENS_AFTER_MS);
      const open = await pollGameplayState(
        player.driver,
        (state) => state.extraction.data.status === "open",
        "结果不确定场景未开放撤离区",
      );
      if (open.extraction.data.status !== "open")
        throw new Error("unreachable");
      await moveToHorizontal(
        player.driver,
        open.extraction.data.zone.center,
        server,
        1,
      );
      await requireInside(player.driver);

      const armed = await applySettlementFault(
        player.http,
        "armCommitUnknownAndBlockReads",
      );
      expect(armed).toMatchObject({
        commitCalls: 0,
        outcomeUnknownArmed: true,
        readsBlocked: true,
      });
      const pendingGameplayState = await finishExtraction(
        player.driver,
        server,
      );
      expect(pendingGameplayState).toBe("pending");
      const uncertain = await pollFault(
        player.http,
        (snapshot) => snapshot.commitOutcomeUnknowns === 1,
        "真实 COMMIT 后没有触发 OutcomeUnknown",
      );
      expect(uncertain).toMatchObject({
        abortCalls: 0,
        commitCalls: 1,
        commitOutcomeUnknowns: 1,
        markCalls: 1,
        outcomeUnknownArmed: false,
        readsBlocked: true,
      });
      expect(server.diagnostics()).toContain(
        '"point":"after_commit_outcome_unknown"',
      );
      await expectResultUnavailable(player.http, match.matchId);

      const current = requireControlledTime(await server.elapse(0));
      const afterGrace =
        matchStart + HARD_DEADLINE_AFTER_MS + SETTLEMENT_GRACE_MS + 1;
      if (current >= afterGrace) {
        throw new Error("结果不确定场景在释放故障前已越过目标边界");
      }
      // ACK 后立即释放读取；即使恰逢一个 ticker，三次有界核对也不会被耗尽。
      await server.advance(afterGrace - current);
      await applySettlementFault(player.http, "releaseReads");
      const recoveredRead = await pollFault(
        player.http,
        (snapshot) => snapshot.settlementReadPassthroughs >= 1,
        "宽限期后协调器没有执行唯一键只读核对",
      );
      expect(recoveredRead.commitCalls).toBe(1);
      expect(recoveredRead.abortCalls).toBe(0);
      expect(recoveredRead.settlementReadUnavailable).toBeGreaterThan(0);

      const result = await pollMatchResult(player, match.matchId, "extracted");
      expect(result.settlement?.resources).toEqual({
        dirt: 1,
        gold: 0,
        diamond: 0,
      });
      const firstWarehouse = await fetchLiveWarehouse(player.http);
      const firstAudit = await pollTerminalAudit(player.http, match.matchId);
      assertExactlyOnceAudit(firstAudit, beforeWarehouse.resources.dirt + 1);
      expect(firstWarehouse.resources.dirt).toBe(
        beforeWarehouse.resources.dirt + 1,
      );
      expect(firstWarehouse.stats.successfulExtractions).toBe(
        beforeWarehouse.stats.successfulExtractions + 1,
      );

      const repeatedResult = await pollMatchResult(
        player,
        match.matchId,
        "extracted",
      );
      const secondWarehouse = await fetchLiveWarehouse(player.http);
      const secondAudit = requireAudit(
        await fetchSettlementAudit(player.http, match.matchId),
      );
      const finalFault = await fetchSettlementFaultSnapshot(player.http);
      expect(repeatedResult.settlement?.settlementId).toBe(
        result.settlement?.settlementId,
      );
      expect(secondAudit).toEqual(firstAudit);
      expect(sameWarehouseSnapshot(firstWarehouse, secondWarehouse)).toBe(true);
      expect(finalFault).toMatchObject({
        abortCalls: 0,
        commitCalls: 1,
        commitOutcomeUnknowns: 1,
        markCalls: 1,
        readsBlocked: false,
      });
      expect(finalFault.resultReadUnavailable).toBe(1);
      expect(finalFault.resultReadPassthroughs).toBeGreaterThanOrEqual(2);

      artifactEvidence = {
        matchId: match.matchId,
        pendingGameplayState,
        resultStatus: result.status,
        beforeWarehouse,
        firstWarehouse,
        secondWarehouse,
        audit: firstAudit,
        fault: finalFault,
      };
    } finally {
      await match?.disconnect();
      await server?.stop();
    }
  });
});

async function expectResultUnavailable(
  transport: Parameters<typeof fetchLiveMatchResult>[0],
  matchId: string,
): Promise<void> {
  try {
    await fetchLiveMatchResult(transport, matchId);
  } catch (error) {
    if (!(error instanceof LiveApiError)) throw error;
    expect(error.status).toBe(503);
    expect(error.retryable).toBe(true);
    return;
  }
  throw new Error("读取故障生效时结果 API 意外成功");
}

async function pollFault(
  transport: Parameters<typeof fetchSettlementFaultSnapshot>[0],
  predicate: (snapshot: SettlementFaultSnapshot) => boolean,
  message: string,
): Promise<SettlementFaultSnapshot> {
  const deadline = Date.now() + POLL_TIMEOUT_MS;
  for (;;) {
    const snapshot = await fetchSettlementFaultSnapshot(transport);
    if (predicate(snapshot)) return snapshot;
    if (Date.now() >= deadline) throw new Error(message);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

function requireAudit(
  value: SettlementAuditSnapshot | null,
): SettlementAuditSnapshot {
  if (value === null) throw new Error("本人结算审计记录不存在");
  return value;
}

async function pollTerminalAudit(
  transport: Parameters<typeof fetchSettlementAudit>[0],
  matchId: string,
): Promise<SettlementAuditSnapshot> {
  const deadline = Date.now() + POLL_TIMEOUT_MS;
  for (;;) {
    const audit = await fetchSettlementAudit(transport, matchId);
    if (
      audit !== null &&
      audit.participantState === "extracted" &&
      (audit.matchState === "finished" || audit.matchState === "aborted")
    ) {
      return audit;
    }
    if (Date.now() >= deadline) {
      throw new Error("结算恢复后比赛审计没有进入稳定终态");
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

function assertExactlyOnceAudit(
  audit: SettlementAuditSnapshot,
  expectedWarehouseDirt: number,
): void {
  expect(audit).toMatchObject({
    participantState: "extracted",
    settlementCount: 1,
    settlementItemCount: 1,
    ledgerCount: 1,
    warehouseRowCount: 1,
    settlementResources: { dirt: 1, gold: 0, diamond: 0 },
    ledgerResources: { dirt: 1, gold: 0, diamond: 0 },
    warehouseResources: {
      dirt: expectedWarehouseDirt,
      gold: 0,
      diamond: 0,
    },
  });
}
