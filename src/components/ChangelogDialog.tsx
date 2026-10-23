import styles from "./ChangelogDialog.module.css";

interface ChangelogEntry {
  version: string;
  current?: boolean;
  items: string[];
}

/** 每个版本只记录「新增的功能」，修改与修复不在此列出。 */
const CHANGELOG: ChangelogEntry[] = [
  {
    version: "1.24",
    current: true,
    items: [
      "新增「赛前热身」：内置反应力测试小游戏，在独立小窗打开，不占用游戏库",
    ],
  },
  {
    version: "1.23",
    items: [
      "游戏详情新增「更改图标」：选择 EXE 提取其图标，或使用 ICO / PNG / JPG / WebP 图片",
      "游戏详情新增「更改启动目标」：游戏搬家后无需删掉重加，直接更新启动位置",
    ],
  },
  {
    version: "1.22",
    items: [
      "启动时检测到新版本改为弹窗提示，可在弹窗内直接下载安装并重启",
    ],
  },
  {
    version: "1.21",
    items: [
      "「检查更新」按钮移至行右侧",
      "版本号旁新增 GitHub 仓库链接",
    ],
  },
  {
    version: "1.2",
    items: [
      "全新白色手柄应用图标",
      "设置内新增「更新日志」",
      "主界面磨砂玻璃背景，隐约透出桌面",
      "卡面交换动画：松手后平滑滑入新位置",
      "拖拽到列表边缘时自动滚动，滚动中卡片始终跟随鼠标",
      "卡面与顶栏文字不可选中",
      "游戏详情弹窗改为实底样式，遮罩带磨砂质感",
      "应用内检查更新与自动升级（官方 Updater，签名校验）",
    ],
  },
  {
    version: "1.1",
    items: [
      "自动发现扩展：新增 Battle.net、Ubisoft Connect 来源与 AMD 探测",
      "扫描来源状态展示：未安装 / 不可用一目了然",
    ],
  },
  {
    version: "1.0",
    items: [
      "首个版本",
      "手动导入 EXE 与快捷方式，按启动目标自动去重",
      "封面三级降级：自定义封面 → 提取图标 → 占位图",
      "应用内一键启动",
      "游玩时长仅统计由本应用启动的会话",
      "进程树追踪：启动器拉起的游戏也能正确计时",
      "电脑睡眠的时间不计入游玩时长",
      "异常退出后的会话恢复，时长只可能少记不会多记",
      "无边框窗口与自定义标题栏",
      "深色 / 白色外观切换",
      "开机自启动",
      "系统托盘与关闭按钮行为设置",
      "卡面长按拖拽排序",
      "游戏详情页：改名 / 查看文件位置",
      "单实例：重复启动自动唤醒已有窗口",
      "维护功能：一键重新提取缺失的图标",
      "数据完全本地存储，卸载不删除游戏记录",
    ],
  },
];

export default function ChangelogDialog({ onClose }: { onClose: () => void }) {
  return (
    <div
      className={styles.backdrop}
      onClick={onClose}
      role="presentation"
    >
      <div
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-label="更新日志"
        onClick={(event) => event.stopPropagation()}
      >
        <div className={styles.header}>
          <h2 className={styles.title}>更新日志</h2>
          <button
            className={styles.closeButton}
            type="button"
            onClick={onClose}
            aria-label="关闭"
          >
            ✕
          </button>
        </div>
        <div className={styles.body}>
          {CHANGELOG.map((entry) => (
            <section key={entry.version} className={styles.entry}>
              <h3 className={styles.versionTitle}>
                <span className={styles.versionTag}>v{entry.version}</span>
                {entry.current && (
                  <span className={styles.currentBadge}>当前版本</span>
                )}
              </h3>
              <ul className={styles.items}>
                {entry.items.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
