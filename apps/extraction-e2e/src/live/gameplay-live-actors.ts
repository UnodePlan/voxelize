import type { CapacityAdmission } from "../capacity-scenario";
import { settleConcurrentPhase } from "../concurrent-phase";

import type { LiveE2eConfig } from "./config";
import type { GameplayScenarioPlayer } from "./gameplay-scenario";
import { fetchQueue, type LiveHttpTransport } from "./http";
import { ProtocolCapacityActor } from "./protocol-actor";
import { PrimaryAdmissionBarrier } from "./queue";
import { deterministicLiveWallet } from "./siwe";

const MATCH_SIZE = 10;
const GAMEPLAY_WALLET_OFFSET = 100;

export interface PreparedGameplayMatch {
  assertTenSockets(): Promise<void>;
  disconnect(): Promise<void>;
  matchId: string;
  players: GameplayScenarioPlayer[];
  worldName: string;
}

export async function prepareGameplayMatch(
  config: LiveE2eConfig,
  walletOffset = GAMEPLAY_WALLET_OFFSET,
): Promise<PreparedGameplayMatch> {
  if (
    !Number.isSafeInteger(walletOffset) ||
    walletOffset < 0 ||
    !Number.isSafeInteger(walletOffset + MATCH_SIZE - 1)
  ) {
    throw new Error(
      "gameplay wallet offset must be a non-negative safe integer",
    );
  }
  const barrier = new PrimaryAdmissionBarrier(MATCH_SIZE);
  const actors = Array.from(
    { length: MATCH_SIZE },
    (_, index) =>
      new ProtocolCapacityActor(
        `gameplay-${String(index + 1).padStart(2, "0")}`,
        deterministicLiveWallet(walletOffset + index),
        true,
        barrier,
        config,
      ),
  );
  try {
    await settleConcurrentPhase(
      "gameplay actors connect",
      actors.map((actor) => actor.connect()),
    );
    const admissions = await settleConcurrentPhase(
      "gameplay actors enqueue",
      actors.map((actor) => actor.enqueue()),
    );
    const assignment = requireOneAssignment(admissions);
    const joins = await settleConcurrentPhase(
      "gameplay actors join",
      actors.map((actor) =>
        actor.join(assignment.matchId, assignment.worldName),
      ),
    );
    if (joins.some((join) => join.status !== "joined")) {
      throw new Error("one of the ten gameplay actors failed to JOIN");
    }
    const players = actors.map((actor) => ({
      actorId: actor.actorId,
      ...actor.gameplaySession(),
    }));
    await waitForActiveMatch(
      players[0].http,
      assignment.matchId,
      assignment.worldName,
      config,
    );
    return {
      matchId: assignment.matchId,
      players,
      worldName: assignment.worldName,
      assertTenSockets: async () => {
        const states = await Promise.all(
          players.map(({ driver }) => driver.getState()),
        );
        if (
          states.length !== MATCH_SIZE ||
          states.some((state) => state.matchId !== assignment.matchId)
        ) {
          throw new Error("ten live sockets did not remain in one match");
        }
      },
      disconnect: () => disconnectActors(actors),
    };
  } catch (error) {
    await disconnectActors(actors);
    throw error;
  }
}

async function waitForActiveMatch(
  http: LiveHttpTransport,
  matchId: string,
  worldName: string,
  config: LiveE2eConfig,
): Promise<void> {
  const deadline = Date.now() + config.scenarioTimeoutMs;
  for (;;) {
    const queue = await fetchQueue(http, "GET");
    if (queue.matchId !== matchId || queue.worldName !== worldName) {
      throw new Error("gameplay activation changed the frozen assignment");
    }
    if (queue.status === "active") return;
    if (queue.status !== "preparing") {
      throw new Error(`gameplay match entered ${queue.status} before active`);
    }
    if (Date.now() >= deadline) {
      throw new Error("gameplay match did not activate after ten JOINs");
    }
    await new Promise((resolve) => setTimeout(resolve, config.pollIntervalMs));
  }
}

function requireOneAssignment(admissions: readonly CapacityAdmission[]): {
  matchId: string;
  worldName: string;
} {
  if (admissions.length !== MATCH_SIZE) {
    throw new Error("gameplay match requires exactly ten admissions");
  }
  const first = admissions[0];
  if (first.status !== "accepted") {
    throw new Error("first gameplay actor was not admitted");
  }
  if (
    admissions.some(
      (admission) =>
        admission.status !== "accepted" ||
        admission.matchId !== first.matchId ||
        admission.worldName !== first.worldName,
    )
  ) {
    throw new Error("gameplay actors did not converge on one assignment");
  }
  return { matchId: first.matchId, worldName: first.worldName };
}

async function disconnectActors(
  actors: readonly ProtocolCapacityActor[],
): Promise<void> {
  const results = await Promise.allSettled(
    actors.map((actor) => actor.disconnect()),
  );
  if (results.some((result) => result.status === "rejected")) {
    throw new Error("one or more gameplay actors failed to disconnect");
  }
}
