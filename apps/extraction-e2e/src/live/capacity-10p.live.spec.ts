import { expect, test } from "@playwright/test";

import { runExactTenCapacityScenario } from "../capacity-scenario";

import { createTenBrowserCapacityActors } from "./capacity-actors";
import { loadLiveE2eConfig } from "./config";
import {
  runBrowserRuntimeCanvasGate,
  runVisualReleaseGate,
} from "./visual-gate";

test.describe.configure({ mode: "serial" });

test("真实十浏览器容量与渲染发布门禁 @10p", async ({ browser }, testInfo) => {
  const config = loadLiveE2eConfig();
  test.setTimeout(config.scenarioTimeoutMs * 3 + 30_000);
  const topology = createTenBrowserCapacityActors(browser, config);

  const result = await runExactTenCapacityScenario(topology.actors, {
    beforeDisconnect: async () => {
      // 十个 World 保持同时在线，但逐页读取 Worker/WebGL，避免诊断本身制造资源峰值。
      for (const actor of topology.browsers.slice(2)) {
        const evidence = await runBrowserRuntimeCanvasGate(
          actor.visualContext(),
          config.scenarioTimeoutMs,
        );
        await testInfo.attach(`${actor.actorId}-runtime-evidence`, {
          body: JSON.stringify(evidence, null, 2),
          contentType: "application/json",
        });
      }
      await runVisualReleaseGate({
        desktop: topology.browsers[0].visualContext(),
        mobile: topology.browsers[1].visualContext(),
        testInfo,
        timeoutMs: config.scenarioTimeoutMs,
      });
    },
  });

  expect(result.browserActors).toBe(10);
  expect(result.protocolActors).toBe(0);
  expect(result.admittedActorIds).toHaveLength(10);
  expect(result.rejectedActorId).toBe("protocol-overflow");
  expect(result.matchId).toMatch(/^[0-9a-f-]{36}$/iu);
  expect(result.worldName).toBe(`match-${result.matchId.replaceAll("-", "")}`);
});
