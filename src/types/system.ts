/** 与 Rust 侧 DTO 一一对应的类型定义。
 *  Rust 端结构体加了 `#[serde(rename_all = "camelCase")]`。 */

export interface AppInfo {
  /** 应用名，取自 tauri.conf.json 的 productName。 */
  name: string;
  /** 应用版本。 */
  version: string;
  /** Tauri 框架版本。 */
  tauriVersion: string;
  /** 构建档位。 */
  buildProfile: "debug" | "release";
  /** 应用标识符，同时也是应用数据目录名。 */
  identifier: string;
}

export interface AppDataPath {
  dataDir: string;
  dbPath: string;
  logDir: string;
  coversDir: string;
  iconsDir: string;
}

/** 与 Rust `AppErrorCode` 对应。 */
export type AppErrorCode =
  | "NotFound"
  | "PermissionDenied"
  | "InvalidArgument"
  | "TargetMissing"
  | "LaunchFailed"
  | "AlreadyRunning"
  | "Duplicate"
  | "DatabaseError"
  | "Internal";

/** 后端返回给前端的结构化错误（不含堆栈）。 */
export interface AppError {
  code: AppErrorCode;
  message: string;
}
