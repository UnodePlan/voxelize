import type { Browser } from "@playwright/test";

import type { CapacityActor } from "../capacity-scenario";

import { BrowserCapacityActor } from "./browser-actor";
import type { LiveE2eConfig } from "./config";
import { ProtocolCapacityActor } from "./protocol-actor";
import { PrimaryAdmissionBarrier } from "./queue";
import { deterministicLiveWallet } from "./siwe";

const MATCH_SIZE = 10;
const MIXED_BROWSER_ACTORS = 2;
const TEN_BROWSER_WALLET_OFFSET = 1_000;

export interface LiveCapacityActors {
  actors: CapacityActor[];
  browsers: {
    desktop: BrowserCapacityActor;
    mobile: BrowserCapacityActor;
  };
}

export interface TenBrowserCapacityActors {
  actors: CapacityActor[];
  browsers: BrowserCapacityActor[];
}

export function createLiveCapacityActors(
  browser: Browser,
  config: LiveE2eConfig,
): LiveCapacityActors {
  // 先固定两个浏览器席位，再让九个协议客户端并发竞争剩余八席。
  const barrier = new PrimaryAdmissionBarrier(MIXED_BROWSER_ACTORS);
  const desktop = new BrowserCapacityActor(
    "browser-01",
    browser,
    deterministicLiveWallet(0),
    barrier,
    config,
    { label: "desktop", viewport: { height: 900, width: 1440 } },
  );
  const mobile = new BrowserCapacityActor(
    "browser-02",
    browser,
    deterministicLiveWallet(1),
    barrier,
    config,
    { label: "mobile", viewport: { height: 844, width: 390 } },
  );
  const actors: CapacityActor[] = [desktop, mobile];
  for (let index = MIXED_BROWSER_ACTORS; index < MATCH_SIZE; index += 1) {
    actors.push(
      new ProtocolCapacityActor(
        `protocol-${String(index - 1).padStart(2, "0")}`,
        deterministicLiveWallet(index),
        false,
        barrier,
        config,
      ),
    );
  }
  actors.push(
    new ProtocolCapacityActor(
      "protocol-overflow",
      deterministicLiveWallet(MATCH_SIZE),
      false,
      barrier,
      config,
    ),
  );
  return { actors, browsers: { desktop, mobile } };
}

export function createTenBrowserCapacityActors(
  browser: Browser,
  config: LiveE2eConfig,
): TenBrowserCapacityActors {
  const barrier = new PrimaryAdmissionBarrier(MATCH_SIZE);
  const viewports = [
    { label: "desktop", viewport: { height: 900, width: 1440 } },
    { label: "mobile", viewport: { height: 844, width: 390 } },
  ] as const;
  const browsers = Array.from({ length: MATCH_SIZE }, (_, index) => {
    const display = viewports[index] ?? {
      label: `desktop-${index + 1}`,
      viewport: { height: 720, width: 1280 },
    };
    return new BrowserCapacityActor(
      `browser-${String(index + 1).padStart(2, "0")}`,
      browser,
      deterministicLiveWallet(TEN_BROWSER_WALLET_OFFSET + index),
      barrier,
      config,
      display,
    );
  });
  const overflow = new ProtocolCapacityActor(
    "protocol-overflow",
    deterministicLiveWallet(TEN_BROWSER_WALLET_OFFSET + MATCH_SIZE),
    false,
    barrier,
    config,
  );
  return { actors: [...browsers, overflow], browsers };
}
