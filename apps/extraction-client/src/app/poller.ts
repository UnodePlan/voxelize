export class RepeatingTask {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private active = false;
  private generation = 0;

  constructor(private readonly intervalMs: number) {
    if (!Number.isSafeInteger(intervalMs) || intervalMs <= 0) {
      throw new Error("intervalMs: expected positive safe integer");
    }
  }

  get running(): boolean {
    return this.active;
  }

  start(
    task: () => void | Promise<void>,
    options: { immediate?: boolean } = {},
  ): void {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    if (options.immediate === true) {
      this.run(generation, task);
    } else {
      this.schedule(generation, task);
    }
  }

  stop(): void {
    this.active = false;
    this.generation += 1;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private run(generation: number, task: () => void | Promise<void>): void {
    if (!this.isCurrent(generation)) return;
    let outcome: void | Promise<void>;
    try {
      outcome = task();
    } catch {
      this.schedule(generation, task);
      return;
    }
    void Promise.resolve(outcome).then(
      () => this.schedule(generation, task),
      () => this.schedule(generation, task),
    );
  }

  private schedule(generation: number, task: () => void | Promise<void>): void {
    if (!this.isCurrent(generation)) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.run(generation, task);
    }, this.intervalMs);
  }

  private isCurrent(generation: number): boolean {
    return this.active && this.generation === generation;
  }
}

export function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
