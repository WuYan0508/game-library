/** 与 Rust 侧 `discovery` 模块的 DTO 一一对应。 */

export type DiscoverySource =
  | "nvidia"
  | "steam"
  | "epic"
  | "ubisoft"
  | "battlenet"
  | "amd"
  | "directory";

export type DiscoveryConfidence = "high" | "medium" | "low";

export interface DiscoveredGame {
  name: string;
  launchPath: string;
  launchArgs: string | null;
  workingDirectory: string | null;
  /** 发现该候选的全部来源（合并后可能多于一个，第一个是主来源）。 */
  sources: DiscoverySource[];
  confidence: DiscoveryConfidence;
  sourceId: string | null;
  /** 已在库中：默认不勾选，且导入时会被后端跳过。 */
  alreadyInLibrary: boolean;
  warnings: string[];
}

export type SourceState =
  | { kind: "available" }
  | { kind: "notInstalled" }
  | { kind: "unavailable"; reason: string };

export interface SourceScanResult {
  source: DiscoverySource;
  state: SourceState;
  /** 去重前的候选数（前端展示用，实际列表在 ScanOutcome.games）。 */
  candidates: DiscoveredGame[];
  skipped: number;
}

export interface ScanOutcome {
  sources: SourceScanResult[];
  games: DiscoveredGame[];
}

export interface DiscoveredImportItem {
  name: string | null;
  launchPath: string;
  launchArgs: string | null;
  workingDirectory: string | null;
}

export interface ImportFailure {
  name: string;
  launchPath: string;
  reason: string;
}

export interface ImportDiscoveredResult {
  imported: number;
  skippedExisting: number;
  failures: ImportFailure[];
}

export const SOURCE_LABELS: Record<DiscoverySource, string> = {
  nvidia: "NVIDIA App",
  steam: "Steam",
  epic: "Epic",
  ubisoft: "Ubisoft Connect",
  battlenet: "Battle.net",
  amd: "AMD Software",
  directory: "指定目录",
};

export const CONFIDENCE_LABELS: Record<DiscoveryConfidence, string> = {
  high: "高置信度",
  medium: "中置信度",
  low: "需要确认",
};
