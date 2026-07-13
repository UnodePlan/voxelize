export const CAPACITY_MATCH_SIZE = 10;
export const CAPACITY_SCENARIO_ACTORS = CAPACITY_MATCH_SIZE + 1;

export type CapacityActorKind = "browser" | "protocol";

export interface CapacityAdmissionAccepted {
  status: "accepted";
  matchId: string;
  worldName: string;
}

export interface CapacityAdmissionRejected {
  status: "rejected";
  code: "MATCH_FULL";
}

export type CapacityAdmission =
  | CapacityAdmissionAccepted
  | CapacityAdmissionRejected;

export interface CapacityJoinAccepted {
  status: "joined";
  matchId: string;
  worldName: string;
}

export interface CapacityJoinRejected {
  status: "rejected";
  code: "MATCH_FULL" | "MATCH_ROSTER_LOCKED";
}

export type CapacityJoin = CapacityJoinAccepted | CapacityJoinRejected;

export interface CapacityActor {
  readonly actorId: string;
  readonly kind: CapacityActorKind;
  connect(): Promise<void>;
  enqueue(): Promise<CapacityAdmission>;
  join(matchId: string, worldName: string): Promise<CapacityJoin>;
  disconnect(): Promise<void>;
}

export interface CapacityScenarioResult {
  matchId: string;
  worldName: string;
  admittedActorIds: string[];
  rejectedActorId: string;
  browserActors: number;
  protocolActors: number;
}

/**
 * 编排器只验证真实驱动返回的协议结果，不在客户端复制服务端容量算法。
 * 驱动可以是 Playwright 页面、轻量 WebSocket 客户端或确定性的服务端 fake harness。
 * 仅用 scripted driver 运行本函数属于编排器单测，不能作为真实网络发布门禁。
 */
export async function runExactTenCapacityScenario(
  actors: readonly CapacityActor[],
): Promise<CapacityScenarioResult> {
  validateActors(actors);
  try {
    await Promise.all(actors.map((actor) => actor.connect()));
    const admissions = await Promise.all(
      actors.map((actor) => actor.enqueue()),
    );
    const accepted = admissions.flatMap((admission, index) =>
      admission.status === "accepted"
        ? [{ actor: actors[index], admission }]
        : [],
    );
    const rejected = admissions.flatMap((admission, index) =>
      admission.status === "rejected"
        ? [{ actor: actors[index], admission }]
        : [],
    );

    requireScenario(
      accepted.length === CAPACITY_MATCH_SIZE,
      `expected ${CAPACITY_MATCH_SIZE} accepted actors, received ${accepted.length}`,
    );
    requireScenario(
      rejected.length === 1,
      `expected one rejected actor, received ${rejected.length}`,
    );
    const first = accepted[0].admission;
    requireScenario(
      first.matchId.length > 0,
      "accepted matchId must not be empty",
    );
    requireScenario(
      first.worldName.length > 0,
      "accepted worldName must not be empty",
    );
    requireScenario(
      accepted.every(
        ({ admission }) =>
          admission.matchId === first.matchId &&
          admission.worldName === first.worldName,
      ),
      "accepted actors did not converge on one match and world",
    );

    const joins = await Promise.all(
      accepted.map(({ actor }) => actor.join(first.matchId, first.worldName)),
    );
    requireScenario(
      joins.every(
        (join) =>
          join.status === "joined" &&
          join.matchId === first.matchId &&
          join.worldName === first.worldName,
      ),
      "an admitted actor failed to join the frozen world",
    );
    const lateJoin = await rejected[0].actor.join(
      first.matchId,
      first.worldName,
    );
    requireScenario(
      lateJoin.status === "rejected" &&
        (lateJoin.code === "MATCH_FULL" ||
          lateJoin.code === "MATCH_ROSTER_LOCKED"),
      "the eleventh actor was able to join the frozen world",
    );

    return {
      matchId: first.matchId,
      worldName: first.worldName,
      admittedActorIds: accepted.map(({ actor }) => actor.actorId),
      rejectedActorId: rejected[0].actor.actorId,
      browserActors: accepted.filter(({ actor }) => actor.kind === "browser")
        .length,
      protocolActors: accepted.filter(({ actor }) => actor.kind === "protocol")
        .length,
    };
  } finally {
    const cleanup = await Promise.allSettled(
      actors.map((actor) => actor.disconnect()),
    );
    requireScenario(
      cleanup.every(({ status }) => status === "fulfilled"),
      "one or more actors failed to disconnect",
    );
  }
}

function validateActors(actors: readonly CapacityActor[]): void {
  requireScenario(
    actors.length === CAPACITY_SCENARIO_ACTORS,
    `capacity scenario requires ${CAPACITY_SCENARIO_ACTORS} actors`,
  );
  const ids = new Set(actors.map(({ actorId }) => actorId));
  requireScenario(
    ids.size === actors.length,
    "capacity actor IDs must be unique",
  );
}

function requireScenario(
  condition: boolean,
  message: string,
): asserts condition {
  if (!condition) {
    throw new Error(`capacity scenario failed: ${message}`);
  }
}
