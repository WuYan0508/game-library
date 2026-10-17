/** 前端访问后端的唯一出口。
 *  组件不得直接调用 invoke，也不得直接接触文件系统或数据库。 */

import { invoke } from "@tauri-apps/api/core";
import type {
  Game,
  GameSortField,
  RemoveGameResult,
  SortOrder,
  UpdateGameInput,
  ReextractReport,
} from "../types/game";
import type { ExeProbeResult, ShortcutProbeResult } from "../types/importer";
import type {
  DiscoveredImportItem,
  ImportDiscoveredResult,
  ScanOutcome,
} from "../types/discovery";
import type {
  ActiveSession,
  SessionStarted,
  StopTrackingResult,
} from "../types/session";
import type { AppError, AppDataPath, AppInfo } from "../types/system";

/** 后端结构化错误的类型守卫。invoke 失败时 reject 的是 `{ code, message }` 普通对象。 */
export function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" && typeof candidate.message === "string"
  );
}

/** 把任意异常转成可直接展示给用户的文案。 */
export function describeError(error: unknown): string {
  if (isAppError(error)) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "发生未知错误";
}

export interface ListArgs {
  sort: GameSortField;
  order: SortOrder;
}

export const gameApi = {
  getAppInfo(): Promise<AppInfo> {
    return invoke<AppInfo>("get_app_info");
  },

  getAppDataPath(): Promise<AppDataPath> {
    return invoke<AppDataPath>("get_app_data_path");
  },

  /** 用系统文件管理器打开日志目录（诊断入口）。返回实际打开的路径。 */
  openLogDir(): Promise<string> {
    return invoke<string>("open_log_dir");
  },

  listGames({ sort, order }: ListArgs): Promise<Game[]> {
    return invoke<Game[]>("list_games", { sort, order });
  },

  getGame(id: string): Promise<Game> {
    return invoke<Game>("get_game", { id });
  },

  searchGames({
    keyword,
    sort,
    order,
  }: ListArgs & { keyword: string }): Promise<Game[]> {
    return invoke<Game[]>("search_games", { keyword, sort, order });
  },

  updateGame(input: UpdateGameInput): Promise<Game> {
    return invoke<Game>("update_game", {
      id: input.id,
      name: input.name ?? null,
      launchPath: input.launchPath ?? null,
      launchArgs: input.launchArgs ?? null,
      workingDirectory: input.workingDirectory ?? null,
    });
  },

  removeGame(id: string): Promise<RemoveGameResult> {
    return invoke<RemoveGameResult>("remove_game", { id });
  },

  /** 只读探测：校验路径并返回默认名称、工作目录与重复提示。 */
  probeExe(path: string): Promise<ExeProbeResult> {
    return invoke<ExeProbeResult>("probe_exe", { path });
  },

  addGameFromExe(
    path: string,
    name: string,
    coverSourcePath?: string,
  ): Promise<Game> {
    return invoke<Game>("add_game_from_exe", {
      path,
      name,
      coverSourcePath: coverSourcePath ?? null,
    });
  },

  /** 只读探测 .lnk：解析目标/参数/工作目录，并检查目标是否存在与是否重复。 */
  probeShortcut(path: string): Promise<ShortcutProbeResult> {
    return invoke<ShortcutProbeResult>("probe_shortcut", { path });
  },

  /**
   * 导入快捷方式。
   * `launchPathOverride` 用于快捷方式目标缺失时，用户手动选定的启动目标。
   */
  addGameFromShortcut(
    path: string,
    name: string,
    launchPathOverride?: string,
    coverSourcePath?: string,
  ): Promise<Game> {
    return invoke<Game>("add_game_from_shortcut", {
      path,
      name,
      launchPathOverride: launchPathOverride ?? null,
      coverSourcePath: coverSourcePath ?? null,
    });
  },

  /** 导入自定义封面。后端只复制用户原图，绝不移动或删除它。 */
  importCover(gameId: string, sourcePath: string): Promise<{ coverPath: string | null }> {
    return invoke<{ coverPath: string | null }>("import_cover", {
      gameId,
      sourcePath,
    });
  },

  /** 重新提取图标。提不到会返回 null（前端回退占位图）。 */
  extractGameIcon(gameId: string): Promise<{ iconPath: string | null }> {
    return invoke<{ iconPath: string | null }>("extract_game_icon", { gameId });
  },

  /** 从选择的文件导入自定义图标（EXE 提取 / 图片转码）。 */
  importGameIcon(gameId: string, sourcePath: string): Promise<{ iconPath: string | null }> {
    return invoke<{ iconPath: string | null }>("import_game_icon", { gameId, sourcePath });
  },

  /** 自定义排序：按完整 id 顺序持久化。 */
  reorderGames(orderedIds: string[]): Promise<void> {
    return invoke<void>("reorder_games", { orderedIds });
  },

  /** 重新提取缺失的图标（维护）。 */
  reextractMissingIcons(): Promise<ReextractReport> {
    return invoke<ReextractReport>("reextract_missing_icons");
  },

  /** 读取「关闭时最小化到托盘」行为。 */
  getCloseToTray(): Promise<boolean> {
    return invoke<boolean>("get_close_to_tray");
  },

  /** 设置关闭按钮行为并持久化。 */
  setCloseToTray(enabled: boolean): Promise<void> {
    return invoke<void>("set_close_to_tray", { enabled });
  },

  /** 扫描本地来源（Steam / NVIDIA App / Epic / 用户指定目录）。 */
  scanGames(directories?: string[]): Promise<ScanOutcome> {
    return invoke<ScanOutcome>("scan_games", { directories });
  },

  /** 批量导入勾选的候选。单条失败不中断批次。 */
  importDiscoveredGames(
    items: DiscoveredImportItem[],
  ): Promise<ImportDiscoveredResult> {
    return invoke<ImportDiscoveredResult>("import_discovered_games", { items });
  },

  /** 从应用内启动游戏并开始跟踪。已有活动会话时返回 AlreadyRunning。 */
  launchGame(gameId: string): Promise<SessionStarted> {
    return invoke<SessionStarted>("launch_game", { gameId });
  },

  getActiveSessions(): Promise<ActiveSession[]> {
    return invoke<ActiveSession[]>("get_active_sessions");
  },

  /** 停止跟踪。**只停表，不会结束游戏进程。** */
  stopTracking(gameId: string): Promise<StopTrackingResult> {
    return invoke<StopTrackingResult>("stop_tracking", { gameId });
  },
};
