// statusFormatting.ts 顶层会引 buildPerTurnChangeSummaries，
// 我们对拍的两个函数用不到它 —— 垫片保证模块可加载即可。
export function buildPerTurnChangeSummaries(..._args: unknown[]): unknown[] {
  throw new Error("parity harness 不应调用 buildPerTurnChangeSummaries");
}
