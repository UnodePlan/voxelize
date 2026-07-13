import { protocol } from "@voxelize/protocol";
import type { MessageProtocol } from "@voxelize/protocol";

import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import { createIntent } from "./network-protocol";
import { IntentSequence } from "./network-sequence";

export interface MovementInput {
  direction: [number, number, number];
  movement: {
    forward: number;
    jump: boolean;
    right: number;
  };
}

type SendMessage = (message: protocol.IMessage) => void;

export class GameplayNetworkEgress {
  private readonly sequence = new IntentSequence();

  constructor(
    private readonly protocolVersion: ExtractionManifest["protocolVersion"],
    private readonly send: SendMessage,
  ) {}

  seed(state: Parameters<IntentSequence["seed"]>[0]): void {
    this.sequence.seed(state);
  }

  attack(): void {
    this.sendMethod(
      "pvp:v1:attack",
      createIntent(this.protocolVersion, this.sequence.next(), {
        weaponSlot: "melee",
      }),
    );
  }

  mining(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void {
    const payload = action === "start" ? { action, voxel } : { action };
    this.sendMethod(
      "pvp:v1:mining",
      createIntent(this.protocolVersion, this.sequence.next(), payload),
    );
  }

  dropSlot(slot: number, expectedInventoryRevision: number): void {
    this.sendMethod(
      "pvp:v1:drop-slot",
      createIntent(this.protocolVersion, this.sequence.next(), {
        slot,
        expectedInventoryRevision,
      }),
    );
  }

  movement(input: MovementInput): void {
    this.send({
      type: protocol.Message.Type.PEER,
      peers: [
        {
          id: "",
          username: "Extractor",
          metadata: JSON.stringify(input),
        },
      ],
    });
  }

  sendWorldPacket(message: MessageProtocol): void {
    if (
      (message.type !== "LOAD" && message.type !== "UNLOAD") ||
      message.json === null ||
      message.json === undefined
    ) {
      return;
    }
    this.send({
      type: protocol.Message.Type[message.type],
      json: JSON.stringify(message.json),
    });
  }

  sendMethod(name: string, payload: unknown): void {
    this.send({
      type: protocol.Message.Type.METHOD,
      method: { name, payload: JSON.stringify(payload) },
    });
  }
}
