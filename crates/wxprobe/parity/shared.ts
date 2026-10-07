// 对拍用的 @zcode/shared 垫片：只提供被引用的类型与两个函数。
export type Locale = "zh-CN" | "en-US" | (string & {});
export type ZCodePermissionRequest = Record<string, unknown>;
export type ZCodeStreamEvent = Record<string, unknown>;
export type ZCodeTaskChangeSummary = Record<string, unknown>;
export type ZCodePersistedToolCall = Record<string, unknown>;
export type ZCodeSessionFile = Record<string, unknown>;
export type ZCodeTaskMeta = Record<string, unknown>;
export type BotCommand = Record<string, unknown>;

export * from "./tool-call-summary.ts";
export * from "./permission-request-preview.ts";
