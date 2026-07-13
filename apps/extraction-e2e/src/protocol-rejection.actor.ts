import { describe, expect, test } from "vitest";

import { classifyJoinErrorText } from "./live/protocol-actor";

describe("协议 JOIN 拒绝分类", () => {
  test("只接受明确的容量或名单锁定错误", () => {
    expect(classifyJoinErrorText("World is full.")).toBe("MATCH_FULL");
    expect(classifyJoinErrorText("Client admission was denied.")).toBe(
      "MATCH_ROSTER_LOCKED",
    );
    expect(classifyJoinErrorText("World is not accepting clients.")).toBe(
      "MATCH_ROSTER_LOCKED",
    );
  });

  test.each([
    "",
    "Internal server error.",
    "World is busy, please reconnect.",
    "World is full but no stable contract",
  ])("拒绝把异常错误当作容量成功：%s", (message) => {
    expect(classifyJoinErrorText(message)).toBeNull();
  });
});
