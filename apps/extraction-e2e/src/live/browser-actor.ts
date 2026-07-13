import type { Browser, BrowserContext, Page } from "@playwright/test";
import type { Wallet } from "ethers";

import type {
  CapacityActor,
  CapacityAdmission,
  CapacityJoin,
} from "../capacity-scenario";

import type { LiveE2eConfig } from "./config";
import { fetchQueue, PlaywrightHttpTransport } from "./http";
import { observeBrowserAdmission, type PrimaryAdmissionBarrier } from "./queue";
import { authenticateEoa } from "./siwe";
import { LiveRuntimeDiagnostics } from "./visual-gate-diagnostics";

const LIVE_WALLET_STORAGE_KEY = "__VOXEL_EXTRACTION_LIVE_E2E__";

export interface BrowserActorViewport {
  height: number;
  width: number;
}

export interface BrowserActorVisualContext {
  actorId: string;
  diagnostics: LiveRuntimeDiagnostics;
  label: string;
  page: Page;
  viewport: BrowserActorViewport;
}

export interface BrowserCapacityActorOptions {
  label: string;
  viewport: BrowserActorViewport;
}

export class BrowserCapacityActor implements CapacityActor {
  readonly kind = "browser" as const;
  private context: BrowserContext | null = null;
  private page: Page | null = null;
  private transport: PlaywrightHttpTransport | null = null;
  private admission: CapacityAdmission | null = null;
  private diagnostics: LiveRuntimeDiagnostics | null = null;
  private barrier: PrimaryAdmissionBarrier;

  constructor(
    readonly actorId: string,
    private readonly browser: Browser,
    private readonly wallet: Wallet,
    barrier: PrimaryAdmissionBarrier,
    private readonly config: LiveE2eConfig,
    private readonly options: BrowserCapacityActorOptions,
  ) {
    this.barrier = barrier;
  }

  async connect(): Promise<void> {
    if (this.context !== null)
      throw new Error(`${this.actorId} already connected`);
    const context = await this.browser.newContext({
      deviceScaleFactor: 1,
      viewport: this.options.viewport,
    });
    this.context = context;
    try {
      await context.addInitScript(
        ({ address, key, origin }) => {
          if (window.location.origin === origin) {
            localStorage.setItem(key, address);
          }
        },
        {
          address: this.wallet.address,
          key: LIVE_WALLET_STORAGE_KEY,
          origin: this.config.clientUrl,
        },
      );
      const transport = new PlaywrightHttpTransport(
        context.request,
        this.config.clientUrl,
        this.config.publicOrigin,
        this.config.scenarioTimeoutMs,
      );
      this.transport = transport;
      await authenticateEoa(transport, this.wallet, this.config);
      const page = await context.newPage();
      this.page = page;
      this.diagnostics = await LiveRuntimeDiagnostics.create(
        page,
        this.config.clientUrl,
      );
      await page.goto(this.config.clientUrl, {
        timeout: this.config.scenarioTimeoutMs,
        waitUntil: "domcontentloaded",
      });
      await page
        .locator('[data-product-shell][data-screen="lobby"]')
        .waitFor({ state: "attached", timeout: this.config.scenarioTimeoutMs });
      this.requireNoRuntimeFailure();
    } catch (error) {
      this.diagnostics?.markClosing();
      await context.close();
      this.context = null;
      this.page = null;
      this.transport = null;
      this.diagnostics = null;
      throw error;
    }
  }

  async enqueue(): Promise<CapacityAdmission> {
    const page = this.requirePage();
    const transport = this.requireTransport();
    this.requireNoRuntimeFailure();
    await page.locator('[data-action="join-queue"]').click({
      timeout: this.config.scenarioTimeoutMs,
    });
    this.admission = await observeBrowserAdmission(
      this.actorId,
      transport,
      this.barrier,
      this.config,
    );
    if (this.admission.status === "accepted") {
      await page
        .locator('[data-product-shell][data-screen="match"]')
        .waitFor({ state: "attached", timeout: this.config.scenarioTimeoutMs });
      this.requireNoRuntimeFailure();
    }
    return this.admission;
  }

  async join(matchId: string, worldName: string): Promise<CapacityJoin> {
    if (this.admission?.status !== "accepted") {
      throw new Error(`${this.actorId} cannot join without an assignment`);
    }
    const page = this.requirePage();
    await page
      .locator(
        '[data-product-shell][data-screen="match"] [data-connection="online"]',
      )
      .waitFor({ state: "attached", timeout: this.config.scenarioTimeoutMs });
    const queue = await fetchQueue(this.requireTransport(), "GET");
    if (queue.matchId !== matchId || queue.worldName !== worldName) {
      throw new Error(`${this.actorId} product client joined the wrong match`);
    }
    this.requireNoRuntimeFailure();
    return { status: "joined", matchId, worldName };
  }

  visualContext(): BrowserActorVisualContext {
    const diagnostics = this.diagnostics;
    if (diagnostics === null) {
      throw new Error(`${this.actorId} runtime diagnostics are unavailable`);
    }
    return {
      actorId: this.actorId,
      diagnostics,
      label: this.options.label,
      page: this.requirePage(),
      viewport: this.options.viewport,
    };
  }

  prepareNextRound(barrier: PrimaryAdmissionBarrier): void {
    if (this.context === null) {
      throw new Error(`${this.actorId} cannot prepare a round before connect`);
    }
    this.barrier = barrier;
    this.admission = null;
    this.diagnostics?.beginExpectedMatch();
  }

  liveHttpTransport(): PlaywrightHttpTransport {
    return this.requireTransport();
  }

  async disconnect(): Promise<void> {
    const context = this.context;
    this.diagnostics?.markClosing();
    this.context = null;
    this.page = null;
    this.transport = null;
    this.diagnostics = null;
    if (context !== null) await context.close();
  }

  private requirePage(): Page {
    if (this.page === null)
      throw new Error(`${this.actorId} page is unavailable`);
    return this.page;
  }

  private requireTransport(): PlaywrightHttpTransport {
    if (this.transport === null) {
      throw new Error(`${this.actorId} HTTP transport is unavailable`);
    }
    return this.transport;
  }

  private requireNoRuntimeFailure(): void {
    this.diagnostics?.assertNoFailures(this.actorId);
  }
}
