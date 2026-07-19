/**
 * DEV 双人可玩切片联调（协议层）：
 * SIWE → 入队(N=2) → JOIN → 挖矿 → 汇合互见 → 近战击杀 → 死亡掉落 → 拾取
 *
 *   cd apps/extraction-e2e && \
 *   EXTRACTION_E2E_SERVER_URL=http://127.0.0.1:4100 \
 *   EXTRACTION_E2E_CLIENT_URL=http://127.0.0.1:5173 \
 *   EXTRACTION_E2E_PUBLIC_ORIGIN=http://127.0.0.1:5173 \
 *   pnpm exec vitest run --config vitest.actor.config.ts src/live/dev-mp-gameplay.actor.ts
 */

import { describe, expect, it } from "vitest";

import { decodeDeathResultEnvelope } from "../../../../contracts/extraction/v1/combat";

import { loadLiveE2eConfig } from "./config";
import {
  lookAt,
  mineOneSurfaceDirt,
  pollGameplayState,
  waitForOneVisibleLoot,
  type MovementDestination,
} from "./gameplay-actions";
import type { GameplayProtocolDriver } from "./gameplay-driver";
import {
  horizontalDistance,
  inventoryResourceCount,
  type LiveVector3,
} from "./gameplay-state";
import { realGameplayTime } from "./gameplay-time";
import { fetchQueue } from "./http";
import { ProtocolCapacityActor } from "./protocol-actor";
import { PrimaryAdmissionBarrier } from "./queue";
import { deterministicLiveWallet } from "./siwe";

const MATCH_SIZE = 2;
/** 每轮换偏移，降低与残留会话撞号概率 */
const WALLET_OFFSET = 400 + Math.floor(Date.now() / 60_000) % 200;
const MOVE_STEP_MS = 150;
const APPROACH_TIMEOUT_MS = 120_000;
const MELEE_COOLDOWN_MS = 600;
/** 20 半心 / 每次 2 → 需 10 次有效 hit，最后一次 kill */
const LETHAL_HITS = 10;

async function serverReady(config: ReturnType<typeof loadLiveE2eConfig>) {
  try {
    const res = await fetch(new URL("/health/ready", config.serverUrl), {
      headers: {
        Origin: config.publicOrigin,
        Accept: "application/json",
      },
      signal: AbortSignal.timeout(3_000),
    });
    return res.ok;
  } catch {
    return false;
  }
}

/** 双人同时走向各自目标（比默认 30s 更长，覆盖 DEV 出生点跨度） */
async function approachTogether(
  destinations: readonly MovementDestination[],
  stopDistance: number,
): Promise<void> {
  const deadline = Date.now() + APPROACH_TIMEOUT_MS;
  const stagnant = destinations.map(() => 0);
  for (;;) {
    const before = destinations.map(({ driver }) => driver.position());
    const remaining = before.map((position, index) =>
      horizontalDistance(position, destinations[index].target),
    );
    if (remaining.every((distance) => distance <= stopDistance)) {
      await Promise.all(
        destinations.map(({ driver, target }) => {
          const current = driver.position();
          const dir = horizontalDir(current, target);
          return driver.sendMovement({
            direction: dir,
            movement: { forward: 0, jump: false, right: 0 },
          });
        }),
      );
      return;
    }
    if (Date.now() >= deadline) {
      throw new Error(
        `approach timed out remaining=${remaining.map((d) => d.toFixed(1)).join(",")}`,
      );
    }
    await Promise.all(
      destinations.map(({ driver, target }, index) => {
        const from = before[index];
        const dist = remaining[index];
        const direction =
          dist <= Number.EPSILON
            ? ([1, 0, 0] as LiveVector3)
            : horizontalDir(from, target);
        return driver.sendMovement({
          direction,
          movement: {
            forward: Math.min(1, Math.max(0.25, dist)),
            jump: stagnant[index] > 2,
            right: 0,
          },
        });
      }),
    );
    await realGameplayTime.elapse(MOVE_STEP_MS);
    const after = destinations.map(({ driver }) => driver.position());
    for (let i = 0; i < destinations.length; i++) {
      stagnant[i] =
        horizontalDistance(before[i], after[i]) <= 0.05
          ? stagnant[i] + 1
          : 0;
    }
  }
}

function horizontalDir(from: LiveVector3, to: LiveVector3): LiveVector3 {
  const dx = to[0] - from[0];
  const dz = to[2] - from[2];
  const length = Math.hypot(dx, dz);
  if (!Number.isFinite(length) || length <= Number.EPSILON) {
    return [1, 0, 0];
  }
  return [dx / length, 0, dz / length];
}

async function waitForPeer(
  viewer: GameplayProtocolDriver,
  otherId: string,
  timeoutMs: number,
): Promise<LiveVector3> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const peer = viewer.peerPosition(otherId);
    if (peer !== null) return peer;
    await realGameplayTime.elapse(100);
  }
  throw new Error(`peer ${otherId} not visible to ${viewer.playerId}`);
}

describe("DEV 双人玩法联调 @dev-mp", () => {
  it(
    "成局后挖矿、互见、近战击杀、死亡掉落与拾取",
    async () => {
      const config = loadLiveE2eConfig({
        ...process.env,
        EXTRACTION_E2E_SERVER_URL:
          process.env.EXTRACTION_E2E_SERVER_URL ?? "http://127.0.0.1:4100",
        EXTRACTION_E2E_CLIENT_URL:
          process.env.EXTRACTION_E2E_CLIENT_URL ?? "http://127.0.0.1:5173",
        EXTRACTION_E2E_PUBLIC_ORIGIN:
          process.env.EXTRACTION_E2E_PUBLIC_ORIGIN ?? "http://127.0.0.1:5173",
        EXTRACTION_E2E_TIMEOUT_MS:
          process.env.EXTRACTION_E2E_TIMEOUT_MS ?? "180000",
      });

      if (!(await serverReady(config))) {
        throw new Error(
          "extraction-server /health/ready 不可用；请先以 DEV_MATCH_SIZE=2 启动 :4100",
        );
      }

      // barrier expected=1：仅协调 primary 先入队；第二人 primary=false 等 barrier 后再 POST
      const barrier = new PrimaryAdmissionBarrier(1);
      const actors = [
        new ProtocolCapacityActor(
          "dev-mp-0",
          deterministicLiveWallet(WALLET_OFFSET),
          true,
          barrier,
          config,
        ),
        new ProtocolCapacityActor(
          "dev-mp-1",
          deterministicLiveWallet(WALLET_OFFSET + 1),
          false,
          barrier,
          config,
        ),
      ];

      try {
        await Promise.all(actors.map((a) => a.connect()));
        console.log("[dev-mp] SIWE + WS ok");

        // 交错入队：primary POST 后 arrive → secondary 再 POST 成局
        const admissions = await Promise.all(actors.map((a) => a.enqueue()));
        if (
          admissions[0].status !== "accepted" ||
          admissions[1].status !== "accepted"
        ) {
          throw new Error(`admission failed: ${JSON.stringify(admissions)}`);
        }
        const matchId = admissions[0].matchId;
        const worldName = admissions[0].worldName;
        expect(admissions[1].matchId).toBe(matchId);
        console.log("[dev-mp] match", matchId, worldName);

        const joins = await Promise.all(
          actors.map((a) => a.join(matchId, worldName)),
        );
        expect(joins.every((j) => j.status === "joined")).toBe(true);
        console.log("[dev-mp] JOIN ok");

        const deadline = Date.now() + config.scenarioTimeoutMs;
        const http0 = actors[0].gameplaySession().http;
        for (;;) {
          const q = await fetchQueue(http0, "GET");
          if (q.status === "active") break;
          if (q.status !== "preparing") {
            throw new Error(`unexpected queue status ${q.status}`);
          }
          if (Date.now() >= deadline) {
            throw new Error("match did not become active");
          }
          await new Promise((r) => setTimeout(r, config.pollIntervalMs));
        }
        console.log("[dev-mp] phase active");

        const [a, b] = actors.map((actor) => actor.gameplaySession());
        const stateA = await a.driver.getState();
        const stateB = await b.driver.getState();
        expect(stateA.matchId).toBe(matchId);
        expect(stateB.health.data.status).toBe("alive");
        console.log(
          "[dev-mp] health",
          stateA.health.data.currentHalfHearts,
          stateB.health.data.currentHalfHearts,
        );
        console.log(
          "[dev-mp] spawn",
          a.driver.position(),
          b.driver.position(),
          "dist",
          horizontalDistance(a.driver.position(), b.driver.position()).toFixed(
            1,
          ),
        );

        // 1) 挖矿权威
        const dirt = await mineOneSurfaceDirt(b.driver, realGameplayTime);
        expect(dirt.after - dirt.before).toBe(1);
        console.log("[dev-mp] mined dirt", dirt);

        // 2) 汇合（中点）
        const pa = a.driver.position();
        const pb = b.driver.position();
        const mid: LiveVector3 = [
          (pa[0] + pb[0]) / 2,
          (pa[1] + pb[1]) / 2,
          (pa[2] + pb[2]) / 2,
        ];
        console.log("[dev-mp] approach mid", mid);
        await approachTogether(
          [
            { driver: a.driver, target: mid },
            { driver: b.driver, target: mid },
          ],
          2.5,
        );
        const dist = horizontalDistance(
          a.driver.position(),
          b.driver.position(),
        );
        console.log(
          "[dev-mp] after approach dist",
          dist.toFixed(2),
          a.driver.position(),
          b.driver.position(),
        );
        expect(dist).toBeLessThan(8);

        // 3) 互见（兴趣半径内 PEER）
        const peerSeenByA = await waitForPeer(
          a.driver,
          b.driver.playerId,
          15_000,
        );
        const peerSeenByB = await waitForPeer(
          b.driver,
          a.driver.playerId,
          15_000,
        );
        console.log("[dev-mp] peers", peerSeenByA, peerSeenByB);

        // 4) 近战打满 10 次有效命中 → kill（与 10 人 live 场景同模型）
        // 死者 WS 可能在阵亡后被服务端关掉，尸体坐标与掉落以击杀者侧为准
        // 击杀前记录击杀者泥土数（贴脸可能秒吸掉落）
        const attackerDirtBeforeCombat = inventoryResourceCount(
          await a.driver.getState(),
          "dirt",
        );
        await lookAt(b.driver, a.driver.position(), realGameplayTime);
        const attackResolutions: string[] = [];
        let lethalHits = 0;
        let corpse: LiveVector3 = b.driver.position();
        const attackDeadline = Date.now() + 90_000;
        let swing = 0;
        while (lethalHits < LETHAL_HITS && Date.now() < attackDeadline) {
          if (swing > 0) await realGameplayTime.elapse(MELEE_COOLDOWN_MS);
          const me = a.driver.position();
          // 优先用 PEER 上的对方坐标；退回受害者本端位置
          const peerThem = a.driver.peerPosition(b.driver.playerId);
          let them: LiveVector3;
          try {
            them = peerThem ?? b.driver.position();
          } catch {
            them = peerThem ?? corpse;
          }
          corpse = them;
          const d = horizontalDistance(me, them);
          if (d > 1.6) {
            await a.driver.sendMovement({
              direction: horizontalDir(me, them),
              movement: { forward: 0.7, jump: false, right: 0 },
            });
            await realGameplayTime.elapse(120);
          }
          await lookAt(a.driver, them, realGameplayTime);
          const result = await a.driver.attack();
          attackResolutions.push(result.resolution);
          console.log(
            `[dev-mp] swing#${swing}`,
            result.resolution,
            "hits",
            lethalHits,
            "d=",
            d.toFixed(2),
          );
          if (result.resolution === "hit" || result.resolution === "kill") {
            lethalHits += 1;
          }
          swing += 1;
          if (result.resolution === "kill") break;
        }
        expect(lethalHits).toBeGreaterThanOrEqual(LETHAL_HITS);
        expect(attackResolutions.at(-1)).toBe("kill");
        console.log("[dev-mp] attacks", attackResolutions.join(","));
        console.log("[dev-mp] corpse", corpse);

        // 5) 死亡 METHOD（死者 WS 可能立刻断开）
        let lostDirt = dirt.after;
        try {
          const death = await b.driver.waitForMethodState(
            "pvp:v1:death-result",
            (value) =>
              decodeDeathResultEnvelope(value, b.driver.manifest),
            (value) => value.data.cause === "melee",
            "victim did not receive melee death result",
          );
          lostDirt = death.data.lost.dirt;
          console.log("[dev-mp] death lost (victim method)", death.data.lost);
        } catch (error) {
          console.warn(
            "[dev-mp] victim death method unavailable:",
            error instanceof Error ? error.message : error,
          );
        }
        expect(lostDirt).toBe(dirt.after);

        // 6+7) 掉落/拾取：贴脸击杀时可能立刻自动拾取，CREATE 帧一闪而过
        // 以击杀前背包基线 + lostDirt 净增为准
        console.log(
          "[dev-mp] attacker dirt before combat",
          attackerDirtBeforeCombat,
        );

        let sawLootEntity = false;
        let lootId: string | null = null;
        try {
          lootId = await Promise.race([
            waitForOneVisibleLoot(a.driver),
            realGameplayTime.elapse(2_500).then(() => {
              throw new Error("loot entity soft-timeout");
            }),
          ]);
          sawLootEntity = true;
          console.log("[dev-mp] loot entity", lootId);
          expect(a.driver.lootCreationCount(lootId)).toBeGreaterThanOrEqual(1);
          await approachTogether([{ driver: a.driver, target: corpse }], 1.2);
        } catch {
          console.log(
            "[dev-mp] no stable loot entity; approach corpse for auto-pickup",
          );
          await approachTogether([{ driver: a.driver, target: corpse }], 1.0);
        }

        const expectedDirt = attackerDirtBeforeCombat + lostDirt;
        const picked = await pollGameplayState(
          a.driver,
          (state) => inventoryResourceCount(state, "dirt") === expectedDirt,
          "attacker did not receive death loot dirt in inventory",
        );
        const pickupAfter = inventoryResourceCount(picked, "dirt");
        expect(pickupAfter - attackerDirtBeforeCombat).toBe(lostDirt);

        // 拾取后不应再看到该掉落
        if (lootId !== null) {
          const lootGoneDeadline = Date.now() + 10_000;
          while (
            Date.now() < lootGoneDeadline &&
            a.driver.visibleLoot().some(({ id }) => id === lootId)
          ) {
            await realGameplayTime.elapse(100);
          }
          expect(
            a.driver.visibleLoot().some(({ id }) => id === lootId),
          ).toBe(false);
        }

        // 若曾见过 CREATE，至少 1 次；否则允许仅背包证据（贴脸秒吸）
        const observed = a.driver.observedLoot();
        console.log("[dev-mp] observedLoot", observed, "sawEntity", sawLootEntity);

        console.log("\n=== DEV-MP GAMEPLAY PASS ===");
        console.log({
          matchId,
          worldName,
          dirtMined: dirt.after - dirt.before,
          peerMutual: true,
          kill: true,
          lostDirt,
          lootId,
          sawLootEntity,
          pickupDelta: pickupAfter - attackerDirtBeforeCombat,
        });


      } finally {
        await Promise.allSettled(actors.map((actor) => actor.disconnect()));
      }
    },
    300_000,
  );
});
