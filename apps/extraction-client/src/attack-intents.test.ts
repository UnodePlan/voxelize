import { describe, expect, it } from "vitest";

import { decodeAttackIntent } from "../../../contracts/extraction/v1/typescript";

import { AttackIntentFactory } from "./attack-intents";

describe("AttackIntentFactory", () => {
  it("只生成服务端可判定目标与伤害的近战意图", () => {
    const factory = new AttackIntentFactory(1);

    expect(decodeAttackIntent(factory.attack())).toMatchObject({
      sequence: 0,
      payload: { weaponSlot: "melee" },
    });
    expect(decodeAttackIntent(factory.attack()).sequence).toBe(1);
  });

  it("从服务端最后接受的 sequence 继续", () => {
    const factory = new AttackIntentFactory(1, 41);

    expect(factory.attack().sequence).toBe(42);
  });

  it("不环绕已耗尽的 u32 sequence", () => {
    const factory = new AttackIntentFactory(1, 4_294_967_295);

    expect(() => factory.attack()).toThrow("sequence 已耗尽");
  });

  it("拒绝无效的初始化游标", () => {
    expect(() => new AttackIntentFactory(-1)).toThrow();
    expect(() => new AttackIntentFactory(1, 4_294_967_296)).toThrow();
  });
});
