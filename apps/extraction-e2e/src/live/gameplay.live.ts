import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { afterAll, beforeAll, describe, expect, test } from "vitest";

import { loadLiveE2eConfig } from "./config";
import { prepareGameplayMatch } from "./gameplay-live-actors";
import { runLiveGameplayScenario } from "./gameplay-scenario";
import { advanceTo } from "./gameplay-scenario-support";
import {
  pollUntilFullyReleased,
  pollUntilMatchReleasedWithLobbyConnections,
  type LiveResourceSnapshot,
} from "./resource-snapshot";
import {
  assessMemoryTrend,
  assertStableLiveMemory,
  FINAL_GROWTH_LIMIT_BYTES,
  LINEAR_GROWTH_LIMIT_BYTES,
} from "./rss-gate";
import { ControlledLiveServer } from "./server-process";

const SERVER_ORIGIN = "http://127.0.0.1:4212";
const PUBLIC_ORIGIN = "http://127.0.0.1:5212";
const DEFAULT_DATABASE_URL = "postgres://localhost/voxelize_extraction_e2e";
const ROUNDS = 5;
const MATCH_SIZE = 10;
const HARD_DEADLINE_AFTER_MS = 720_000;
const RESOURCE_RELEASE_TIMEOUT_MS = 60_000;
describe("真实十人玩法闭环", () => {
  let server: ControlledLiveServer;

  beforeAll(async () => {
    server = await ControlledLiveServer.start({
      databaseUrl: process.env.DATABASE_URL ?? DEFAULT_DATABASE_URL,
      publicOrigin: PUBLIC_ORIGIN,
      serverOrigin: SERVER_ORIGIN,
    });
  });

  afterAll(async () => {
    await server?.stop();
  });

  test(`同一进程连续 ${ROUNDS} 局后释放玩法资源且 RSS 不呈线性增长`, async () => {
    const config = loadLiveE2eConfig({
      ...process.env,
      EXTRACTION_E2E_CLIENT_URL: PUBLIC_ORIGIN,
      EXTRACTION_E2E_PUBLIC_ORIGIN: PUBLIC_ORIGIN,
      EXTRACTION_E2E_SERVER_URL: SERVER_ORIGIN,
      EXTRACTION_E2E_TIMEOUT_MS: "180000",
    });
    const rssSamples: number[] = [];
    const liveMemorySamples: number[] = [];
    const resourceEvidence: RoundResourceEvidence[] = [];
    // 时间戳偏移确保每次真实运行生成全新的本地 EOA，避免复用历史账号状态。
    const walletOffsetBase = Date.now() * 100 + (process.pid % 100);

    for (let round = 0; round < ROUNDS; round += 1) {
      const match = await prepareGameplayMatch(
        config,
        walletOffsetBase + round * MATCH_SIZE,
      );
      const resourceTransport = match.players[0].http;
      let matchReleased: LiveResourceSnapshot | null = null;
      try {
        const evidence = await runScenarioWithDiagnostics(match);
        assertGameplayEvidence(evidence);
        await advanceTo(
          server,
          evidence.matchStartMonotonicMs + HARD_DEADLINE_AFTER_MS,
        );
        matchReleased = await pollUntilMatchReleasedWithLobbyConnections(
          resourceTransport,
          MATCH_SIZE,
          RESOURCE_RELEASE_TIMEOUT_MS,
          `第 ${round + 1} 局比赛资源清理后`,
        );
      } finally {
        // 比赛结束后连接留在 lobby；每轮显式断连才能验证连接级资源回收。
        await match.disconnect();
      }
      const fullyReleased = await pollUntilFullyReleased(
        resourceTransport,
        RESOURCE_RELEASE_TIMEOUT_MS,
        `第 ${round + 1} 局显式断连后`,
      );
      const rss = await server.sampleRssBytes();
      if (rss !== null) rssSamples.push(rss);
      liveMemorySamples.push(fullyReleased.memory.liveAllocatedBytes);
      if (matchReleased === null) {
        throw new Error("match resource evidence disappeared after cleanup");
      }
      resourceEvidence.push({
        round: round + 1,
        matchReleased,
        fullyReleased,
        rssBytes: rss,
      });
    }

    if (process.platform === "win32") expect(rssSamples).toHaveLength(0);
    else expect(rssSamples).toHaveLength(ROUNDS);
    expect(liveMemorySamples).toHaveLength(ROUNDS);
    const memoryAssessment = assessMemoryTrend(liveMemorySamples);
    const rssDiagnostic =
      rssSamples.length === 0 ? null : assessMemoryTrend(rssSamples);
    await writeResourceGateArtifact(
      config.artifactDir,
      resourceEvidence,
      memoryAssessment,
      rssDiagnostic,
    );
    assertStableLiveMemory(liveMemorySamples);
  });

  async function runScenarioWithDiagnostics(
    match: Awaited<ReturnType<typeof prepareGameplayMatch>>,
  ) {
    try {
      return await runLiveGameplayScenario({
        assertTenSockets: match.assertTenSockets,
        clock: server,
        matchId: match.matchId,
        players: match.players,
      });
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      throw new Error(
        `${message}\n最近的受控 Engine 日志：\n${server.diagnostics()}`,
        { cause },
      );
    }
  }
});

function assertGameplayEvidence(
  evidence: Awaited<ReturnType<typeof runLiveGameplayScenario>>,
): void {
  expect(evidence.attacks).toHaveLength(10);
  expect(evidence.attacks.map((hit) => hit.victimHalfHearts)).toEqual([
    18, 16, 14, 12, 10, 8, 6, 4, 2, 0,
  ]);
  expect(evidence.dirt.after - evidence.dirt.before).toBe(1);
  expect(evidence.death.data.lost.dirt).toBe(1);
  expect(evidence.pickup.after - evidence.pickup.before).toBe(1);
  expect(evidence.extractionZone.radiusBlocks).toBe(4);
  expect(evidence.extractionZone.halfHeightBlocks).toBe(3);
  expect(evidence.attackerResult.status).toBe("extracted");
  expect(evidence.victimResult.status).toBe("dead");
  expect(evidence.attackerResult.settlement?.resources.dirt).toBe(1);
  expect(evidence.lootId).not.toBe("");
}

interface RoundResourceEvidence {
  fullyReleased: LiveResourceSnapshot;
  matchReleased: LiveResourceSnapshot;
  round: number;
  rssBytes: number | null;
}

async function writeResourceGateArtifact(
  artifactDir: string,
  rounds: readonly RoundResourceEvidence[],
  memoryAssessment: ReturnType<typeof assessMemoryTrend>,
  rssDiagnostic: ReturnType<typeof assessMemoryTrend> | null,
): Promise<void> {
  await mkdir(artifactDir, { recursive: true });
  await writeFile(
    join(artifactDir, "live-gameplay-resource-gate.json"),
    `${JSON.stringify(
      {
        schema: "extraction.e2e.resource-gate.v2",
        createdAt: new Date().toISOString(),
        platform: process.platform,
        thresholds: {
          liveMemoryGrowthLimitBytes: LINEAR_GROWTH_LIMIT_BYTES,
          liveMemoryHardLimitBytes: FINAL_GROWTH_LIMIT_BYTES,
        },
        memoryAssessment,
        rssDiagnostic,
        rounds,
      },
      null,
      2,
    )}\n`,
    { encoding: "utf8", flag: "wx" },
  );
}
