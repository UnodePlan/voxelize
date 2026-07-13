const MEBIBYTE = 1_024 * 1_024;
export const LINEAR_GROWTH_LIMIT_BYTES = 16 * MEBIBYTE;
export const FINAL_GROWTH_LIMIT_BYTES = 64 * MEBIBYTE;

export type MemoryGateFailure = "hard-cap" | "linear-growth" | null;

export interface MemoryGateAssessment {
  failure: MemoryGateFailure;
  finalGrowthBytes: number;
  strictlyRising: boolean;
}

/** 首轮作为热身；存活分配净增超过 16 MiB 即失败，64 MiB 以上为硬上限。 */
export function assessMemoryTrend(
  samples: readonly number[],
): MemoryGateAssessment {
  if (samples.length < 3) {
    throw new Error(
      "memory gate requires at least three completed-round samples",
    );
  }
  for (const sample of samples) {
    if (!Number.isSafeInteger(sample) || sample < 0) {
      throw new Error("memory samples must be non-negative safe integers");
    }
  }
  const finalGrowthBytes = samples[samples.length - 1] - samples[0];
  const strictlyRising = samples
    .slice(1)
    .every((sample, index) => sample > samples[index]);
  const failure: MemoryGateFailure =
    finalGrowthBytes > FINAL_GROWTH_LIMIT_BYTES
      ? "hard-cap"
      : finalGrowthBytes > LINEAR_GROWTH_LIMIT_BYTES
        ? "linear-growth"
        : null;
  return { failure, finalGrowthBytes, strictlyRising };
}

export function assertStableLiveMemory(samples: readonly number[]): void {
  const assessment = assessMemoryTrend(samples);
  if (assessment.failure === null) return;
  throw new Error(
    `live server allocation gate failed (${assessment.failure}): ` +
      `${assessment.finalGrowthBytes} bytes across ${samples.length} rounds; ` +
      `samples=${JSON.stringify(samples)}`,
  );
}
