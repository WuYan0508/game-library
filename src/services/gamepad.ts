/**
 * 手柄控制服务（层 1：游戏库导航）。
 *
 * 基于标准 Gamepad API（WebView2/Chromium 原生支持，无需驱动与 Rust 改动）。
 * - 方向：十字键（12-15）或左摇杆（死区 0.5），支持长按重复（首延迟 420ms，连发 130ms）
 * - 按键：A=确认启动，B=返回，Y=详情，菜单=设置（边缘触发，长按不重复）
 * - 安全阀：仅当「手柄控制」开启、有已连接手柄、且窗口持有焦点时才轮询；
 *   启动游戏抢走焦点后自动挂起，不会在后台移动游戏库焦点。
 */

export type GamepadAction =
  | "up"
  | "down"
  | "left"
  | "right"
  | "confirm"
  | "back"
  | "detail"
  | "menu";

const STORAGE_KEY = "gamepadControl";
/** 左摇杆死区。 */
const DEADZONE = 0.5;
/** 方向长按首次重复前的延迟。 */
const REPEAT_DELAY = 450;
/** 方向长按的连发间隔。 */
const REPEAT_INTERVAL = 150;
/** 方向切换的边缘冷却：吸收十字键抖动，避免一次按键连跳多格。 */
const EDGE_COOLDOWN = 90;

type Handler = (action: GamepadAction) => void;

/** 手柄处理器分层栈：后注册的在最上层，只有栈顶接收操作。
 *  用稳定 id 注册（如 "library-grid"、"dialog-detail"）——
 *  更新同 id 只替换处理器、不改变层级，避免重渲染时层级乱跳。 */
const stack: { id: string; handler: Handler }[] = [];
// 模块加载即从存储初始化：否则重启后开关显示开着、轮询却永远不会启动
let enabled = localStorage.getItem(STORAGE_KEY) === "true";
let rafId: number | null = null;
let heldDir: GamepadAction | null = null;
let nextRepeat = 0;
let lastMoveAt = 0;
let prevButtons: boolean[] = [];

export function getStoredGamepadEnabled(): boolean {
  return localStorage.getItem(STORAGE_KEY) === "true"; // 默认关闭
}

/** 当前连接的手柄名（取第一个）；没有则为 null。 */
export function getConnectedGamepadName(): string | null {
  const pads = Array.from(navigator.getGamepads?.() ?? []).filter(
    (pad): pad is Gamepad => pad !== null,
  );
  if (pads.length === 0) {
    return null;
  }
  const id = pads[0].id.trim();
  return id.length > 0 ? id : null;
}

/**
 * 注册/更新/移除一层手柄处理器。
 * 传 handler = 注册（新 id 压栈顶，已有 id 原位替换）；传 null = 移除该层。
 */
export function setGamepadLayer(id: string, handler: Handler | null): void {
  const index = stack.findIndex((entry) => entry.id === id);
  if (handler === null) {
    if (index >= 0) {
      stack.splice(index, 1);
    }
  } else if (index >= 0) {
    stack[index].handler = handler;
  } else {
    stack.push({ id, handler });
  }
  if (stack.length > 0) {
    start();
  } else {
    stop();
  }
}

export function setGamepadEnabled(value: boolean): void {
  localStorage.setItem(STORAGE_KEY, value ? "true" : "false");
  enabled = value;
  if (!enabled) {
    stop();
  } else {
    start();
  }
}

function emit(action: GamepadAction): void {
  if (stack.length === 0) {
    return;
  }
  stack[stack.length - 1].handler(action);
}

function start(): void {
  if (rafId !== null || !enabled || stack.length === 0) {
    return;
  }
  rafId = requestAnimationFrame(poll);
}

function stop(): void {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  heldDir = null;
  prevButtons = [];
}

function poll(): void {
  rafId = requestAnimationFrame(poll);

  // 窗口失焦即挂起：启动的游戏里按 A 不会移动游戏库焦点
  if (document.hidden || !document.hasFocus()) {
    heldDir = null;
    prevButtons = [];
    return;
  }

  const pads = Array.from(navigator.getGamepads?.() ?? []).filter(
    (pad): pad is Gamepad => pad !== null,
  );
  if (pads.length === 0) {
    heldDir = null;
    prevButtons = [];
    return;
  }
  const pad = pads[0];

  // 方向：十字键优先，左摇杆叠加（死区 0.5）
  const ax = pad.axes[0] ?? 0;
  const ay = pad.axes[1] ?? 0;
  const pressed = (index: number) => pad.buttons[index]?.pressed ?? false;
  let dir: GamepadAction | null = null;
  if (pressed(14) || ax <= -DEADZONE) dir = "left";
  else if (pressed(15) || ax >= DEADZONE) dir = "right";
  else if (pressed(12) || ay <= -DEADZONE) dir = "up";
  else if (pressed(13) || ay >= DEADZONE) dir = "down";

  const now = performance.now();
  if (dir !== null) {
    const isEdge = dir !== heldDir;
    if (isEdge && now - lastMoveAt < EDGE_COOLDOWN) {
      // 冷却期内的方向切换视为抖动：更新方向但不发射
      heldDir = dir;
      nextRepeat = now + REPEAT_DELAY;
    } else if (isEdge) {
      heldDir = dir;
      nextRepeat = now + REPEAT_DELAY;
      lastMoveAt = now;
      emit(dir);
    } else if (!isEdge && now >= nextRepeat) {
      nextRepeat = now + REPEAT_INTERVAL;
      lastMoveAt = now;
      emit(dir);
    }
  } else {
    heldDir = null;
  }

  // 按键边缘触发（standard mapping：0=A，1=B，3=Y，9=菜单）
  if (pressed(0) && !prevButtons[0]) emit("confirm");
  if (pressed(1) && !prevButtons[1]) emit("back");
  if (pressed(3) && !prevButtons[3]) emit("detail");
  if (pressed(9) && !prevButtons[9]) emit("menu");
  prevButtons = pad.buttons.map((button) => button.pressed);
}
