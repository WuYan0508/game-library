/** 游戏详情弹窗：点卡片封面进入。
 *
 * 提供：改名（保存走现有 update_game 命令）、文件位置展示、
 * 打开所在位置（opener 插件在资源管理器中定位文件）。
 */

import { useEffect, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";
import { describeError, gameApi } from "../services/gameApi";
import { toUserMessage } from "../services/errorMessages";
import { pushToast } from "../services/toastStore";
import { formatPlayTime } from "../utils/format";
import Cover from "./Cover";
import { useGamepadDialog } from "../hooks/useGamepadDialog";
import type { Game } from "../types/game";
const EXE_FILTERS = [{ name: "可执行文件", extensions: ["exe"] }];
const ICON_SOURCE_FILTERS = [
  { name: "图标来源（EXE / 图片）", extensions: ["exe", "ico", "png", "jpg", "jpeg", "webp"] },
];

import styles from "./GameDetailDialog.module.css";

interface GameDetailDialogProps {
  game: Game;
  onClose: () => void;
  /** 保存成功后把更新后的记录交给外层刷新列表。 */
  onUpdated: (updated: Game) => void;
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    pushToast("已复制到剪贴板", "info");
  } catch (err) {
    pushToast(toUserMessage(err));
  }
}

export default function GameDetailDialog({
  game,
  onClose,
  onUpdated,
}: GameDetailDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const [name, setName] = useState(game.name);
  const [saving, setSaving] = useState(false);
  const [retargeting, setRetargeting] = useState(false);
  const [iconBusy, setIconBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 换了游戏（或外部刷新）时重置输入与错误态
  useEffect(() => {
    setName(game.name);
    setError(null);
  }, [game.id, game.name]);

  const trimmed = name.trim();
  const dirty = trimmed !== game.name && trimmed !== "";

  async function save() {
    if (trimmed === "") {
      setError("名称不能为空");
      return;
    }
    if (trimmed === game.name) {
      onClose();
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const updated = await gameApi.updateGame({ id: game.id, name: trimmed });
      onUpdated(updated);
      pushToast("名称已更新", "info");
      onClose();
    } catch (err) {
      setError(describeError(err));
    } finally {
      setSaving(false);
    }
  }

  async function reveal() {
    try {
      await revealItemInDir(game.launchPath);
    } catch (err) {
      pushToast(toUserMessage(err));
    }
  }

  /** 更改启动目标：选择新的 EXE，探测后更新启动路径与工作目录（名称与参数保持不变）。 */
  async function chooseTarget() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: EXE_FILTERS,
      });
      if (typeof selected !== "string") {
        return;
      }
      setRetargeting(true);
      const probe = await gameApi.probeExe(selected);
      const updated = await gameApi.updateGame({
        id: game.id,
        launchPath: probe.launchPath,
        workingDirectory: probe.workingDirectory,
      });
      onUpdated(updated);
      pushToast("启动目标已更新", "info");
    } catch (err) {
      setError(describeError(err));
    } finally {
      setRetargeting(false);
    }
  }

  /** 更改图标：EXE 提取其图标，图片转码存入应用数据目录。 */
  async function chooseIcon() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: ICON_SOURCE_FILTERS,
      });
      if (typeof selected !== "string") {
        return;
      }
      setIconBusy(true);
      await gameApi.importGameIcon(game.id, selected);
      const updated = await gameApi.getGame(game.id);
      onUpdated(updated);
      pushToast("图标已更新", "info");
    } catch (err) {
      setError(describeError(err));
    } finally {
      setIconBusy(false);
    }
  }

  useGamepadDialog(dialogRef, {
    onBack: onClose,
    hints: [
      { key: "A", label: "确认" },
      { key: "B", label: "关闭" },
    ],
  });

  return (
    <div className={styles.backdrop} role="presentation">
      <div ref={dialogRef} className={styles.dialog} role="dialog" aria-modal="true" aria-label="游戏详情">
        <header className={styles.header}>
          <h2 className={styles.title}>游戏详情</h2>
          <button
            className={styles.close}
            type="button"
            aria-label="关闭"
            onClick={onClose}
          >
            ✕
          </button>
        </header>

        <div className={styles.body}>
          <div className={styles.summary}>
            <div className={styles.coverBox}>
              <Cover game={game} />
            </div>
            <div className={styles.summaryText}>
              <span className={styles.gameName} title={game.name}>
                {game.name}
              </span>
              <span className={styles.muted}>
                累计时长 {formatPlayTime(game.totalPlaySeconds)}
              </span>
              {game.isLauncher && (
                <span className={styles.muted}>通过平台启动器启动，计时可能不准确</span>
              )}
            </div>
          </div>

          {error !== null && (
            <div className={styles.error} role="alert">
              {error}
            </div>
          )}

          <label className={styles.field}>
            <span className={styles.fieldLabel}>名称</span>
            <input
              className={styles.nameInput}
              type="text"
              value={name}
              maxLength={80}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void save();
                }
              }}
            />
          </label>

          <div className={styles.field}>
            <span className={styles.fieldLabel}>启动目标</span>
            <p className={styles.path} title={game.launchPath}>
              {game.launchPath}
            </p>
            <div className={styles.pathActions}>
              <button
                className={styles.smallButton}
                type="button"
                onClick={() => void reveal()}
                title="在资源管理器中定位该文件"
              >
                打开所在位置
              </button>
              <button
                className={styles.smallButton}
                type="button"
                onClick={() => void copyText(game.launchPath)}
              >
                复制路径
              </button>
              <button
                className={styles.smallButton}
                type="button"
                disabled={retargeting}
                title="选择新的 EXE，启动参数保持不变"
                onClick={() => void chooseTarget()}
              >
                {retargeting ? "更改中…" : "更改目标"}
              </button>
            </div>
          </div>

          <div className={styles.field}>
            <span className={styles.fieldLabel}>图标</span>
            <div className={styles.pathActions}>
              <button
                className={styles.smallButton}
                type="button"
                disabled={iconBusy}
                onClick={() => void chooseIcon()}
              >
                {iconBusy ? "导入中…" : "更改图标"}
              </button>
              <span className={styles.muted}>
                支持 EXE（提取其图标）与 ICO / PNG / JPG / WebP 图片
              </span>
            </div>
          </div>

          <div className={styles.field}>
            <span className={styles.fieldLabel}>工作目录</span>
            {game.workingDirectory ? (
              <>
                <p className={styles.path} title={game.workingDirectory}>
                  {game.workingDirectory}
                </p>
                <div className={styles.pathActions}>
                  <button
                    className={styles.smallButton}
                    type="button"
                    onClick={() => void copyText(game.workingDirectory ?? "")}
                  >
                    复制路径
                  </button>
                </div>
              </>
            ) : (
              <p className={styles.pathMuted}>未设置（默认使用启动目标所在目录）</p>
            )}
          </div>
        </div>

        <footer className={styles.footer}>
          <button className={styles.ghost} type="button" onClick={onClose}>
            取消
          </button>
          <button
            className={styles.primary}
            type="button"
            disabled={saving || !dirty}
            title={dirty ? "保存名称修改" : "名称未修改"}
            onClick={() => void save()}
          >
            {saving ? "保存中…" : "保存"}
          </button>
        </footer>
      </div>
    </div>
  );
}
