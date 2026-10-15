/** 按 `AppErrorCode` 归类的用户文案兜底表。
 *
 * 后端给的 `AppError.message` 已经是面向用户的中文，这里是**兜底与审计**：
 * - 后端 message 为空时用它；
 * - 收到未知 code 时统一给通用文案，**绝不把内部细节/堆栈漏给用户**。
 * 新增错误码时必须同步补这张表（见 architecture.md §7.3）。
 */

import type { AppErrorCode } from "../types/system";
import { isAppError } from "./gameApi";

const FALLBACK_MESSAGES: Record<AppErrorCode, string> = {
  NotFound: "找不到对应的文件或记录，可能已被移动或删除。",
  PermissionDenied: "没有足够的权限完成这个操作。",
  InvalidArgument: "提供的信息不完整或格式不对，请检查后重试。",
  TargetMissing: "快捷方式指向的目标不存在，可取消导入或手动选择启动目标。",
  LaunchFailed: "无法启动该程序，请检查文件是否存在或是否被系统阻止。",
  AlreadyRunning: "这个游戏已经在运行中了。",
  Duplicate: "库中已有相同的启动目标，不会再创建一条记录。",
  DatabaseError: "读写本地数据失败，请重试；若反复出现请到设置页打开日志目录。",
  Internal: "出了点内部问题，请重试；若反复出现请到设置页打开日志目录。",
};

const GENERIC_MESSAGE = "发生了预料之外的问题，请重试。";

/** 是否是后端会返回、且前端已认识错误码。 */
export function isKnownErrorCode(code: string): code is AppErrorCode {
  return code in FALLBACK_MESSAGES;
}

/** 把任意异常转成可直接展示给用户的文案，绝不暴露堆栈与内部细节。 */
export function toUserMessage(error: unknown): string {
  if (isAppError(error)) {
    if (error.message.trim() !== "") {
      return error.message;
    }
    if (isKnownErrorCode(error.code)) {
      return FALLBACK_MESSAGES[error.code];
    }
    return GENERIC_MESSAGE;
  }
  if (error instanceof Error && error.message.trim() !== "") {
    return error.message;
  }
  return GENERIC_MESSAGE;
}
