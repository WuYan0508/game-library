/** 与 Rust 侧 `commands::import` 的 DTO 一一对应。 */

export interface ExeProbe {
  /** 用户选择的原始路径。 */
  sourcePath: string;
  /** 规范化后的绝对路径，也是实际启动目标。 */
  launchPath: string;
  /** 文件名去掉 .exe。 */
  defaultName: string;
  /** EXE 所在目录。 */
  workingDirectory: string;
  sizeBytes: number;
}

/** 已在库中的等价记录。探测阶段就会返回，用户确认前即可看到提示。 */
export interface DuplicateRef {
  id: string;
  name: string;
}

export interface ExeProbeResult extends ExeProbe {
  duplicate: DuplicateRef | null;
}

export interface ShortcutProbe {
  /** 用户选择的 .lnk 路径。 */
  sourcePath: string;
  /** 解析出的启动目标。目标缺失时仍是「原本应指向的路径」，供用户判断。 */
  launchPath: string;
  /** 快捷方式里的原样参数串。 */
  launchArgs: string;
  workingDirectory: string;
  /** .lnk 文件名 → 目标文件名。 */
  defaultName: string;
  /** 目标疑似平台启动器，计时可能不准确。 */
  isLauncher: boolean;
  /** 目标文件当前是否存在。false 时必须由用户「取消 / 手动选择启动目标」。 */
  targetExists: boolean;
}

export interface ShortcutProbeResult extends ShortcutProbe {
  duplicate: DuplicateRef | null;
}
