import { expect, test } from "@playwright/test";

import { runExactTenCapacityScenario } from "../capacity-scenario";

import { createLiveCapacityActors } from "./capacity-actors";
import { loadLiveE2eConfig } from "./config";
import { runVisualReleaseGate } from "./visual-gate";

test.describe.configure({ mode: "serial" });

test("真实 2 浏览器 + 8 协议客户端容量门禁 @smoke", async ({
  browser,
}, testInfo) => {
  const config = loadLiveE2eConfig();
  test.setTimeout(config.scenarioTimeoutMs * 3 + 30_000);
  const topology = createLiveCapacityActors(browser, config);

  const result = await runExactTenCapacityScenario(topology.actors, {
    beforeDisconnect: async () =>
      runVisualReleaseGate({
        desktop: topology.browsers.desktop.visualContext(),
        mobile: topology.browsers.mobile.visualContext(),
        testInfo,
        timeoutMs: config.scenarioTimeoutMs,
      }),
  });

  expect(result.browserActors).toBe(2);
  expect(result.protocolActors).toBe(8);
  expect(result.admittedActorIds).toHaveLength(10);
  expect(
    topology.actors.find(({ actorId }) => actorId === result.rejectedActorId)
      ?.kind,
  ).toBe("protocol");
  expect(result.admittedActorIds).not.toContain(result.rejectedActorId);
  expect(result.matchId).toMatch(/^[0-9a-f-]{36}$/iu);
  expect(result.worldName).toBe(`match-${result.matchId.replaceAll("-", "")}`);
});
