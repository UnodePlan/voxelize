import { describe, expect, it, vi } from "vitest";

import stateFixtures from "../../../../contracts/extraction/v1/fixtures/get-state-results.json";
import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  decodeGameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

import {
  createMatchNetworkEvents,
  hasTerminalGameplay,
} from "./match-coordinator-support";
import { INITIAL_APP_STATE, type AppState } from "./state";

const manifest = decodeExtractionManifest(manifestJson);

describe("match coordinator support", () => {
  it("treats a closed extraction window as a result-polling boundary", () => {
    const deadFixture = stateFixtures.cases.find(
      ({ name }) => name === "valid-dead-gameplay-state",
    );
    const state = decodeGameplayStateData(deadFixture?.value, manifest);

    expect(state.extraction.data.status).toBe("closed");
    expect(hasTerminalGameplay({ ...state, deathResult: null })).toBe(true);
  });

  it("reports active protocol errors but ignores terminal eviction races", () => {
    const aliveFixture = stateFixtures.cases.find(
      ({ name }) => name === "valid-alive-gameplay-state",
    );
    const deadFixture = stateFixtures.cases.find(
      ({ name }) => name === "valid-dead-gameplay-state",
    );
    let state: AppState = {
      ...INITIAL_APP_STATE,
      screen: "match",
      gameplay: decodeGameplayStateData(aliveFixture?.value, manifest),
    };
    const dispatch = vi.fn();
    const events = createMatchNetworkEvents({
      dispatch,
      getState: () => state,
      isCurrent: () => true,
      onAuthenticationInvalidated: vi.fn(),
      onReconnectExpired: vi.fn(),
      startResultPoll: vi.fn(),
      stopResultPoll: vi.fn(),
    });

    events.onProtocolError("active rejection");
    expect(dispatch).toHaveBeenCalledWith({
      type: "NOTICE",
      message: "active rejection",
    });

    state = {
      ...state,
      gameplay: decodeGameplayStateData(deadFixture?.value, manifest),
    };
    events.onProtocolError("terminal rejection");
    state = { ...state, screen: "result", gameplay: null };
    events.onProtocolError("result rejection");
    expect(dispatch).toHaveBeenCalledTimes(1);
  });
});
