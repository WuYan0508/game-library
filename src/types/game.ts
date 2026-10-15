/** 与 Rust 侧 `database::models::Game` 一一对应的类型定义。 */

export type GameSourceType = "exe" | "shortcut";

export interface Game {
  id: string;
  /** 用户可编辑的显示名称。 */
  name: string;
  /** 导入时解析出的原始名称。 */
  originalName: string;
  sourceType: GameSourceType;
  /** 用户选择的原始 EXE / .lnk 路径。 */
  sourcePath: string;
  /** 实际启动目标。 */
  launchPath: string;
  /** 原样参数串，不做分词。 */
  launchArgs: string;
  workingDirectory: string | null;
  /** 相对应用数据目录的路径。 */
  coverPath: string | null;
  iconPath: string | null;
  /** 目标疑似平台启动器（会影响计时准确性的提示）。 */
  isLauncher: boolean;
  totalPlaySeconds: number;
  lastPlayedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export type GameSortField = "name" | "playTime" | "createdAt" | "custom";
export type SortOrder = "asc" | "desc";

export interface RemoveGameResult {
  /** 被一并清理的会话记录数。 */
  removedSessions: number;
  /** 被回收的封面 / 图标文件数。原始游戏文件不受影响。 */
  removedAssets: number;
}

export interface UpdateGameInput {
  id: string;
  name?: string;
  launchPath?: string;
  launchArgs?: string;
  /** 传空串表示清空工作目录。 */
  workingDirectory?: string;
}

/** 重新提取缺失图标的报告。 */
export interface ReextractFailure {
  name: string;
  reason: string;
}

export interface ReextractReport {
  fixed: number;
  alreadyOk: number;
  failed: ReextractFailure[];
}
