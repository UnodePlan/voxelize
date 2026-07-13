import type { MatchResult } from "../api/models";

import { escapeHtml } from "./html";

export function resultDetail(result: MatchResult): {
  icon: string;
  title: string;
  subtitle: string;
} {
  switch (result.status) {
    case "extracted":
      return {
        icon: "shield-check",
        title: "成功撤离",
        subtitle: "本局资源已写入永久仓库",
      };
    case "dead":
      return {
        icon: "swords",
        title: "行动终止",
        subtitle:
          result.killerPublicPlayerId === null
            ? "你已阵亡"
            : `击杀者 ${escapeHtml(result.killerPublicPlayerId.slice(0, 8))}`,
      };
    case "timedOut":
      return {
        icon: "timer",
        title: "未能撤离",
        subtitle:
          result.terminalCause === "reconnectTimeout"
            ? "重连窗口已结束"
            : "硬截止时间已到",
      };
    case "aborted":
      return {
        icon: "circle-alert",
        title: "对局作废",
        subtitle: "本局未提交资源",
      };
    case "pendingReconciliation":
      return {
        icon: "refresh-cw",
        title: "正在核对",
        subtitle: "结算结果尚未最终确认",
      };
  }
}

export function resultLabel(status: MatchResult["status"]): string {
  return {
    pendingReconciliation: "正在核对",
    extracted: "成功撤离",
    dead: "行动终止",
    timedOut: "未能撤离",
    aborted: "对局作废",
  }[status];
}
