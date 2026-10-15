/** 「扫描游戏」弹窗：来源状态 → 预览勾选 → 确认导入。
 *
 * 边界（需求 §5 / §6）：扫描只读；已在库中的候选默认不勾选且不可勾；
 * 用户可以取消，结果不落库；导入复用现有新增游戏 API。
 */

import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { describeError, gameApi } from "../services/gameApi";
import { toUserMessage } from "../services/errorMessages";
import { pushToast } from "../services/toastStore";
import type {
  DiscoveredGame,
  DiscoverySource,
  ScanOutcome,
  SourceScanResult,
} from "../types/discovery";
import {
  CONFIDENCE_LABELS,
  SOURCE_LABELS,
} from "../types/discovery";
import styles from "./ScanDialog.module.css";

interface ScanDialogProps {
  onClose: () => void;
  /** 有游戏导入成功后通知外层刷新列表。 */
  onImported: () => void;
}

type Phase =
  | { status: "choose" }
  | { status: "scanning" }
  | { status: "ready"; outcome: ScanOutcome }
  | { status: "importing"; outcome: ScanOutcome }
  | { status: "done"; outcome: ScanOutcome; summary: string };

export default function ScanDialog({ onClose, onImported }: ScanDialogProps) {
  const [phase, setPhase] = useState<Phase>({ status: "choose" });
  const [directories, setDirectories] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());

  const outcome = phase.status === "scanning" || phase.status === "choose" ? null : phase.outcome;

  async function chooseDirectory() {
    setError(null);
    try {
      const selected = await open({
        multiple: true,
        directory: true,
        title: "选择要扫描的游戏目录",
      });
      if (selected === null) {
        return;
      }
      const picked = Array.isArray(selected) ? selected : [selected];
      setDirectories((previous) => {
        const next = [...previous];
        for (const path of picked) {
          if (!next.includes(path)) {
            next.push(path);
          }
        }
        return next;
      });
    } catch (err) {
      setError(describeError(err));
    }
  }

  async function runScan() {
    setError(null);
    setChecked(new Set());
    setPhase({ status: "scanning" });
    try {
      const result = await gameApi.scanGames(directories);
      setPhase({ status: "ready", outcome: result });
    } catch (err) {
      setPhase({ status: "choose" });
      setError(describeError(err));
    }
  }

  /** 勾选键：用启动路径（同一候选唯一）。 */
  const importable = useMemo(() => {
    if (!outcome) {
      return [];
    }
    return outcome.games.filter((game) => !game.alreadyInLibrary);
  }, [outcome]);

  function toggle(game: DiscoveredGame) {
    if (game.alreadyInLibrary) {
      return;
    }
    setChecked((previous) => {
      const next = new Set(previous);
      if (next.has(game.launchPath)) {
        next.delete(game.launchPath);
      } else {
        next.add(game.launchPath);
      }
      return next;
    });
  }

  function toggleAll() {
    setChecked((previous) => {
      if (previous.size >= importable.length) {
        return new Set();
      }
      return new Set(importable.map((game) => game.launchPath));
    });
  }

  async function confirmImport() {
    if (!outcome) {
      return;
    }
    const items = outcome.games
      .filter((game) => checked.has(game.launchPath))
      .map((game) => ({
        name: game.name,
        launchPath: game.launchPath,
        launchArgs: game.launchArgs,
        workingDirectory: game.workingDirectory,
      }));

    if (items.length === 0) {
      setError("请先勾选要导入的游戏");
      return;
    }

    setError(null);
    setPhase({ status: "importing", outcome });

    try {
      const result = await gameApi.importDiscoveredGames(items);
      const parts = [`新增 ${result.imported} 个`];
      if (result.skippedExisting > 0) {
        parts.push(`跳过已在库中的 ${result.skippedExisting} 个`);
      }
      if (result.failures.length > 0) {
        parts.push(`失败 ${result.failures.length} 个`);
      }
      const summary = parts.join("，");
      setPhase({ status: "done", outcome, summary });

      if (result.imported > 0) {
        onImported();
      }
      if (result.failures.length > 0) {
        pushToast(
          `部分导入失败：${result.failures
            .map((failure) => `${failure.name}（${failure.reason}）`)
            .join("；")}`,
        );
      }
    } catch (err) {
      setPhase({ status: "ready", outcome });
      pushToast(toUserMessage(err));
    }
  }

  const busy =
    phase.status === "scanning" || phase.status === "importing";
  const importableCount = importable.length;

  return (
    <div className={styles.backdrop} role="presentation">
      <div className={styles.dialog} role="dialog" aria-modal="true" aria-label="扫描游戏">
        <header className={styles.header}>
          <h2 className={styles.title}>扫描游戏</h2>
          <button
            className={styles.close}
            type="button"
            aria-label="关闭"
            disabled={busy}
            onClick={onClose}
          >
            ✕
          </button>
        </header>

        <div className={styles.body}>
          {error !== null && (
            <div className={styles.error} role="alert">
              {error}
            </div>
          )}

          {phase.status === "choose" && (
            <>
              <p className={styles.paragraph}>
                扫描本机的 Steam、NVIDIA App、Epic 清单与你指定的目录，找出可能已安装的游戏。
                扫描是<strong>只读</strong>的，不会自动导入 —— 结果由你逐个勾选确认。
              </p>
              <p className={styles.paragraph}>
                NVIDIA App 与 Steam 的数据来自本机明文清单；若未安装或无法读取，会显示来源状态并跳过。
              </p>

              <div className={styles.dirSection}>
                <div className={styles.dirHeader}>
                  <span>额外扫描目录（可选）</span>
                  <button className={styles.smallButton} type="button" onClick={() => void chooseDirectory()}>
                    选择目录
                  </button>
                </div>
                {directories.length === 0 ? (
                  <p className={styles.muted}>未选择。不选则只扫启动器清单。</p>
                ) : (
                  <ul className={styles.dirList}>
                    {directories.map((directory) => (
                      <li key={directory} className={styles.dirItem}>
                        <span className={styles.mono}>{directory}</span>
                        <button
                          className={styles.smallButton}
                          type="button"
                          onClick={() =>
                            setDirectories((previous) =>
                              previous.filter((path) => path !== directory),
                            )
                          }
                        >
                          移除
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>

              <button
                className={styles.primary}
                type="button"
                onClick={() => void runScan()}
              >
                开始扫描
              </button>
            </>
          )}

          {phase.status === "scanning" && (
            <p className={styles.loading}>正在扫描本机的启动器清单与指定目录…</p>
          )}

          {outcome !== null && (
            <>
              <div className={styles.sourceList}>
                {outcome.sources.map((source) => (
                  <SourceRow key={source.source} result={source} />
                ))}
              </div>

              {phase.status === "importing" ? (
                <p className={styles.loading}>正在导入…（含封面与图标提取，可能需要一点时间）</p>
              ) : (
                <>
                  {outcome.games.length === 0 ? (
                    <p className={styles.muted}>没有发现候选。可以再指定一个目录试试。</p>
                  ) : (
                    <>
                      <div className={styles.listHeader}>
                        <span>
                          发现 {outcome.games.length} 个候选，其中可导入 {importableCount} 个
                        </span>
                        {importableCount > 0 && (
                          <button
                            className={styles.smallButton}
                            type="button"
                            onClick={toggleAll}
                          >
                            {checked.size >= importableCount ? "全不选" : "全选可导入"}
                          </button>
                        )}
                      </div>

                      <ul className={styles.list}>
                        {outcome.games.map((game) => (
                          <li key={game.launchPath} className={styles.item}>
                            <label
                              className={
                                game.alreadyInLibrary
                                  ? `${styles.itemMain} ${styles.disabled}`
                                  : styles.itemMain
                              }
                            >
                              <input
                                type="checkbox"
                                checked={checked.has(game.launchPath)}
                                disabled={game.alreadyInLibrary}
                                onChange={() => toggle(game)}
                              />
                              <span className={styles.itemText}>
                                <span className={styles.itemName} title={game.launchPath}>
                                  {game.name}
                                </span>
                                <span className={styles.itemPath}>{game.launchPath}</span>
                                {game.warnings.length > 0 && (
                                  <span className={styles.warnings}>
                                    {game.warnings.join("；")}
                                  </span>
                                )}
                              </span>
                            </label>
                            <span className={styles.itemMeta}>
                              <span className={styles.badge}>
                                {game.sources.map((source) => SOURCE_LABELS[source]).join(" + ")}
                              </span>
                              <span
                                className={
                                  game.confidence === "low"
                                    ? `${styles.badge} ${styles.badgeLow}`
                                    : styles.badge
                                }
                              >
                                {CONFIDENCE_LABELS[game.confidence]}
                              </span>
                              {game.alreadyInLibrary && (
                                <span className={styles.badge}>已在库中</span>
                              )}
                            </span>
                          </li>
                        ))}
                      </ul>
                    </>
                  )}

                  {phase.status === "done" && (
                    <div className={styles.summary} role="status">
                      {phase.summary}
                    </div>
                  )}
                </>
              )}
            </>
          )}
        </div>

        <footer className={styles.footer}>
          <button
            className={styles.ghost}
            type="button"
            disabled={busy}
            onClick={onClose}
          >
            {phase.status === "done" ? "完成" : "取消"}
          </button>
          {outcome !== null && phase.status !== "done" && (
            <button
              className={styles.primary}
              type="button"
              disabled={busy || checked.size === 0}
              onClick={() => void confirmImport()}
            >
              {phase.status === "importing"
                ? "导入中…"
                : `导入选中的 ${checked.size} 个`}
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}

function SourceRow({ result }: { result: SourceScanResult }) {
  const label = SOURCE_LABELS[result.source as DiscoverySource];
  let state: string;
  if (result.state.kind === "available") {
    state =
      result.candidates.length > 0
        ? `发现 ${result.candidates.length} 个候选`
        : "已扫描，未发现候选";
  } else if (result.state.kind === "notInstalled") {
    state = "未安装，已跳过";
  } else {
    state = `不可用：${result.state.reason}`;
  }

  return (
    <div className={styles.sourceRow}>
      <span className={styles.badge}>{label}</span>
      <span className={styles.sourceState}>{state}</span>
      {result.skipped > 0 && (
        <span className={styles.sourceSkipped}>{result.skipped} 项已跳过</span>
      )}
    </div>
  );
}
