import type {
  LiveResourceSnapshot,
  MemoryResourceCounts,
} from "./resource-snapshot";

export interface ReleaseLifecycleProfile {
  coordinator: LiveResourceSnapshot["coordinator"];
  memory: Pick<
    MemoryResourceCounts,
    "liveWorldInstances" | "worldBackgroundTasks"
  >;
  server: LiveResourceSnapshot["server"];
  worlds: LiveResourceSnapshot["worlds"];
}

export function releaseLifecycleProfile(
  snapshot: LiveResourceSnapshot,
): ReleaseLifecycleProfile {
  return {
    coordinator: snapshot.coordinator,
    memory: {
      liveWorldInstances: snapshot.memory.liveWorldInstances,
      worldBackgroundTasks: snapshot.memory.worldBackgroundTasks,
    },
    server: snapshot.server,
    worlds: snapshot.worlds,
  };
}
