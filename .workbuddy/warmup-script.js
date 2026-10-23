/* ============================================================
   反应力测试挑战 - 游戏逻辑 script.js
   代码按功能分区：
   1. 获取页面元素
   2. 游戏配置与状态管理
   3. 工具函数（计时器、随机时间）
   4. 开始测试流程
   5. 处理测试区域点击 / 空格
   6. 抢跑处理
   7. 有效成绩处理
   8. 成绩评价
   9. 更新页面状态与统计
   10. localStorage 保存与读取
   11. 清空成绩
   12. 切换难度 / 切换主题
   13. 声音控制（Web Audio API）
   14. 键盘操作
   15. 初始化
   整体用 IIFE 包裹，避免污染全局变量。
   ============================================================ */

(function () {
  "use strict";

  /* ========== 1. 获取页面元素 ========== */
  const gameArea        = document.getElementById("gameArea");
  const gameText        = document.getElementById("gameText");
  const gameHint        = document.getElementById("gameHint");
  const startBtn        = document.getElementById("startBtn");
  const restartBtn      = document.getElementById("restartBtn");
  const clearBtn        = document.getElementById("clearBtn");
  const soundBtn        = document.getElementById("soundBtn");
  const themeBtns       = Array.from(document.querySelectorAll(".theme-btn"));
  const diffBtns        = Array.from(document.querySelectorAll(".diff-btn"));
  const currentDiffText = document.getElementById("currentDiffText");
  const currentDiffRange= document.getElementById("currentDiffRange");
  const currentScoreEl  = document.getElementById("currentScore");
  const currentCommentEl= document.getElementById("currentComment");
  const bestScoreEl     = document.getElementById("bestScore");
  const bestSubEl       = document.getElementById("bestSub");
  const avgScoreEl      = document.getElementById("avgScore");
  const validCountEl    = document.getElementById("validCount");
  const recentListEl    = document.getElementById("recentList");
  const celebrateEl     = document.getElementById("celebrate");


  /* ========== 2. 游戏配置与状态管理 ========== */

  // 三个难度的随机等待时间配置（单位：毫秒）
  const DIFFICULTY = {
    easy:   { name: "简单模式", min: 2000, max: 4000, range: "2～4 秒" },
    normal: { name: "普通模式", min: 1000, max: 4000, range: "1～4 秒" },
    hard:   { name: "困难模式", min: 500,  max: 3000, range: "0.5～3 秒" }
  };

  // 游戏状态：idle（初始/可开始）waiting（等待变绿）go（已变绿可点）result（本轮结束）
  let gameState = "idle";
  // 随机等待的计时器 id，用来在抢跑或重置时清除
  let waitTimer = null;
  // 屏幕变绿的准确时间（performance.now()，毫秒）
  let greenTime = 0;
  // 本轮是否已记录过成绩，防止快速连点产生多条成绩
  let hasRecorded = false;

  // 玩家设置
  let currentDifficulty = "normal";
  let theme = "tech";
  let soundOn = true;

  // 成绩数据
  let bestScores   = { easy: null, normal: null, hard: null }; // 三个难度各自的最好成绩
  let recentScores = [];   // 最近五次有效成绩 [{time, diff, comment}]
  let allScores    = [];   // 所有有效成绩（用于计算平均与次数）
  let validCount   = 0;    // 有效测试次数

  // 本次成绩的显示文本与评价（抢跑时为"无效"）
  let lastScoreText = "暂无";
  let lastComment   = "";


  /* ========== 3. 工具函数 ========== */

  // 清除旧的随机等待计时器，保证同一时间只有一个计时器
  function clearTimer() {
    if (waitTimer !== null) {
      clearTimeout(waitTimer);
      waitTimer = null;
    }
  }

  // 设置测试区域的显示状态
  // stateClass: idle / wait / go / fail / done
  function setGameArea(stateClass, mainText, subText) {
    gameArea.className = "game-area state-" + stateClass;
    gameText.textContent = mainText;
    gameHint.textContent = subText || "";
  }

  // 根据当前难度生成随机等待时间（毫秒）
  function getRandomDelay() {
    const cfg = DIFFICULTY[currentDifficulty];
    return cfg.min + Math.random() * (cfg.max - cfg.min);
  }


  /* ========== 4. 开始测试流程 ========== */

  function startTest() {
    // 测试进行中，直接忽略，防止重复创建计时器
    if (gameState === "waiting" || gameState === "go") return;

    // 双保险：先清除任何残留计时器
    clearTimer();
    // 隐藏上一轮的庆祝提示
    hideCelebrate();
    // 重置本轮记录标志
    hasRecorded = false;

    // 进入等待状态
    gameState = "waiting";
    setGameArea("wait", "准备……不要提前点击！", "等待屏幕变绿");
    updateButtons();

    // 生成随机等待时间，到时间后变绿
    const delay = getRandomDelay();
    waitTimer = setTimeout(function () {
      // 双保险：只有仍在等待状态才变绿（抢跑后已清除并改状态，不会进入这里）
      if (gameState !== "waiting") return;
      gameState = "go";
      greenTime = performance.now(); // 记录变绿的精确时间
      setGameArea("go", "现在点击！", "越快越好！");
      playSound("go");
    }, delay);
  }


  /* ========== 5. 处理测试区域点击 / 空格 ========== */

  // 统一的玩家动作入口（鼠标点击和空格键都走这里）
  function handleAction() {
    if (gameState === "waiting") {
      // 还没变绿就点 → 抢跑
      handleFalseStart();
    } else if (gameState === "go") {
      // 变绿后点击 → 有效成绩
      handleValidClick();
    }
    // idle / result 状态点击不产生任何效果
  }


  /* ========== 6. 抢跑处理 ========== */

  function handleFalseStart() {
    // 关键：立即清除随机等待计时器，避免几秒后又变绿
    clearTimer();
    gameState = "result";
    setGameArea("fail", "抢跑了！这次成绩无效", "请重新测试");
    playSound("fail");

    // 本次成绩显示为无效，不记录任何成绩
    lastScoreText = "无效";
    lastComment = "";
    currentScoreEl.textContent = lastScoreText;
    currentCommentEl.textContent = lastComment;

    // 不更新 best / recent / all / count
    updateButtons();
  }


  /* ========== 7. 有效成绩处理 ========== */

  function handleValidClick() {
    // 一轮只能记录一次，防止快速连点产生多条成绩
    if (hasRecorded) return;
    hasRecorded = true;

    // 用 performance.now() 计算反应时间，精度更高
    const reaction = Math.round(performance.now() - greenTime);
    const comment = getComment(reaction);

    gameState = "result";
    setGameArea("done", "本次反应时间：" + reaction + " 毫秒", comment);
    playSound("done");

    // 更新本次成绩显示
    lastScoreText = reaction + " 毫秒";
    lastComment = comment;

    // 在写入前判断是否打破当前难度的最好成绩
    const prevBest = bestScores[currentDifficulty];
    const isRecord = (prevBest === null || reaction < prevBest);

    // 保存本次有效成绩（recent + all + count）
    saveScore(reaction, comment);

    // 如果破纪录，更新最好成绩并庆祝
    if (isRecord) {
      bestScores[currentDifficulty] = reaction;
      saveBest();
      showCelebrate();
      playSound("record");
    }

    updateAllDisplay();
    updateButtons();
  }

  // 把一次有效成绩加入统计
  function saveScore(time, comment) {
    allScores.push(time);
    validCount = allScores.length;
    // 最新成绩放在最前面
    recentScores.unshift({ time: time, diff: currentDifficulty, comment: comment });
    // 超过五次，删除最早的一次
    if (recentScores.length > 5) recentScores.length = 5;
    saveAll();
  }


  /* ========== 8. 成绩评价 ========== */

  // 根据反应时间返回简短评价
  function getComment(ms) {
    if (ms < 180) return "反应之神";
    if (ms < 230) return "闪电反应";
    if (ms < 300) return "非常快";
    if (ms < 400) return "反应不错";
    if (ms < 600) return "还可以更快";
    return "继续练习";
  }


  /* ========== 9. 更新页面状态与统计 ========== */

  // 更新所有成绩统计的显示
  function updateAllDisplay() {
    // 本次成绩
    currentScoreEl.textContent = lastScoreText;
    currentCommentEl.textContent = lastComment;

    // 最好成绩（当前难度）
    const best = bestScores[currentDifficulty];
    bestScoreEl.textContent = (best === null) ? "暂无成绩" : (best + " 毫秒");
    bestSubEl.textContent = DIFFICULTY[currentDifficulty].name;

    // 平均反应时间（所有有效成绩平均，四舍五入）
    if (allScores.length === 0) {
      avgScoreEl.textContent = "暂无数据";
    } else {
      let sum = 0;
      for (let i = 0; i < allScores.length; i++) sum += allScores[i];
      avgScoreEl.textContent = Math.round(sum / allScores.length) + " 毫秒";
    }

    // 有效测试次数
    validCountEl.textContent = validCount;

    // 最近五次成绩列表
    renderRecent();
  }

  // 渲染最近五次成绩
  function renderRecent() {
    if (recentScores.length === 0) {
      recentListEl.innerHTML = '<li class="recent-empty">还没有成绩，开始你的第一次测试吧！</li>';
      return;
    }
    let html = "";
    for (let i = 0; i < recentScores.length; i++) {
      const r = recentScores[i];
      const diffName = DIFFICULTY[r.diff] ? DIFFICULTY[r.diff].name : "未知";
      const cmt = r.comment || "";
      html += '<li>' +
              '<span class="recent-time">' + r.time + ' 毫秒</span>' +
              '<span class="recent-meta">' + diffName + '｜' + cmt + '</span>' +
              '</li>';
    }
    recentListEl.innerHTML = html;
  }

  // 更新按钮的可用状态
  function updateButtons() {
    const inProgress = (gameState === "waiting" || gameState === "go");
    startBtn.disabled = inProgress;     // 测试中开始按钮不可重复点击
    restartBtn.disabled = inProgress;   // 测试中不能重复创建计时任务
    // 测试中难度按钮暂时不可修改
    diffBtns.forEach(function (b) { b.disabled = inProgress; });
  }

  // 显示 / 隐藏新纪录庆祝提示
  function showCelebrate() {
    celebrateEl.classList.remove("show");
    // 触发重绘，让动画可以重新播放
    void celebrateEl.offsetWidth;
    celebrateEl.classList.add("show");
  }
  function hideCelebrate() {
    celebrateEl.classList.remove("show");
  }


  /* ========== 10. localStorage 保存与读取 ========== */

  // 保存所有成绩数据
  function saveAll() {
    try {
      localStorage.setItem("rt_all", JSON.stringify(allScores));
      localStorage.setItem("rt_count", String(validCount));
      localStorage.setItem("rt_recent", JSON.stringify(recentScores));
    } catch (e) {
      // 存储失败（如隐私模式）不影响游戏
    }
  }
  // 保存最好成绩
  function saveBest() {
    try {
      localStorage.setItem("rt_best", JSON.stringify(bestScores));
    } catch (e) {}
  }

  // 读取字符串，失败返回默认值
  function readStr(key, def) {
    try {
      const v = localStorage.getItem(key);
      return (v === null) ? def : v;
    } catch (e) { return def; }
  }
  // 读取对象
  function readObj(key, def) {
    try {
      const v = localStorage.getItem(key);
      if (!v) return def;
      const obj = JSON.parse(v);
      return (obj && typeof obj === "object") ? obj : def;
    } catch (e) { return def; }
  }
  // 读取数组
  function readArr(key) {
    try {
      const v = localStorage.getItem(key);
      if (!v) return [];
      const arr = JSON.parse(v);
      return Array.isArray(arr) ? arr : [];
    } catch (e) { return []; }
  }

  // 从 localStorage 读取所有数据并做基本校验
  function loadData() {
    // 主题
    theme = readStr("rt_theme", "tech");
    if (["tech", "space", "ninja"].indexOf(theme) === -1) theme = "tech";

    // 难度
    currentDifficulty = readStr("rt_diff", "normal");
    if (!DIFFICULTY[currentDifficulty]) currentDifficulty = "normal";

    // 声音
    soundOn = readStr("rt_sound", "on") !== "off";

    // 最好成绩
    bestScores = readObj("rt_best", { easy: null, normal: null, hard: null });
    ["easy", "normal", "hard"].forEach(function (k) {
      const v = bestScores[k];
      if (typeof v !== "number" || !isFinite(v) || v <= 0) bestScores[k] = null;
    });

    // 最近五次（校验每条记录）
    recentScores = readArr("rt_recent").filter(function (r) {
      return r && typeof r.time === "number" && isFinite(r.time) && r.diff && DIFFICULTY[r.diff];
    });
    if (recentScores.length > 5) recentScores.length = 5;

    // 所有有效成绩（只保留正数）
    allScores = readArr("rt_all").filter(function (x) {
      return typeof x === "number" && isFinite(x) && x > 0;
    });
    validCount = allScores.length;
  }


  /* ========== 11. 清空成绩 ========== */

  function clearScores() {
    // 如果测试还在进行，先安全停止
    clearTimer();
    if (gameState === "waiting" || gameState === "go") {
      gameState = "idle";
      setGameArea("idle", "点击下方按钮开始测试", "提示：也可以按空格键操作");
      updateButtons();
    }

    // 弹出确认提示
    if (!window.confirm("确定要清空全部成绩吗？")) return;

    // 清空成绩数据
    bestScores = { easy: null, normal: null, hard: null };
    recentScores = [];
    allScores = [];
    validCount = 0;
    lastScoreText = "暂无";
    lastComment = "";

    // 清除 localStorage 中的成绩数据（保留主题、声音、难度）
    try {
      localStorage.removeItem("rt_best");
      localStorage.removeItem("rt_recent");
      localStorage.removeItem("rt_all");
      localStorage.removeItem("rt_count");
    } catch (e) {}

    hideCelebrate();
    updateAllDisplay();
  }


  /* ========== 12. 切换难度 / 切换主题 ========== */

  function setDifficulty(diff) {
    if (!DIFFICULTY[diff]) return;
    // 测试进行中不允许修改难度
    if (gameState === "waiting" || gameState === "go") return;
    currentDifficulty = diff;
    try { localStorage.setItem("rt_diff", diff); } catch (e) {}

    // 更新难度按钮选中状态
    diffBtns.forEach(function (b) {
      b.classList.toggle("active", b.dataset.diff === diff);
    });
    // 更新当前难度文字
    currentDiffText.textContent = DIFFICULTY[diff].name;
    currentDiffRange.textContent = DIFFICULTY[diff].range;
    // 切换难度后显示对应难度的最好成绩
    updateAllDisplay();
  }

  function setTheme(t) {
    if (["tech", "space", "ninja"].indexOf(t) === -1) return;
    theme = t;
    document.body.setAttribute("data-theme", t);
    try { localStorage.setItem("rt_theme", t); } catch (e) {}
    // 更新主题按钮选中状态
    themeBtns.forEach(function (b) {
      b.classList.toggle("active", b.dataset.theme === t);
    });
    // 主题切换不影响游戏逻辑与成绩数据
  }


  /* ========== 13. 声音控制（Web Audio API） ========== */

  let audioCtx = null;

  // 懒加载 AudioContext（必须在用户交互后使用）
  function getCtx() {
    if (!audioCtx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (AC) audioCtx = new AC();
    }
    return audioCtx;
  }

  // 生成一个单音：freq 频率，start 开始时间，dur 时长，type 波形，peak 音量
  function tone(freq, start, dur, type, peak) {
    const ctx = getCtx();
    if (!ctx) return;
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = type;
    osc.frequency.value = freq;
    // 音量包络：快速上升，缓慢下降，避免咔嗒声
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.linearRampToValueAtTime(peak, start + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + dur);
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start(start);
    osc.stop(start + dur);
  }

  // 播放提示音（type：go 变绿 / done 完成 / fail 抢跑 / record 新纪录）
  function playSound(type) {
    if (!soundOn) return;
    try {
      const ctx = getCtx();
      if (!ctx) return;
      // 浏览器可能挂起 AudioContext，需手动恢复
      if (ctx.state === "suspended") ctx.resume();
      const now = ctx.currentTime;

      if (type === "go") {
        // 变绿：清脆高音
        tone(880, now, 0.14, "sine", 0.25);
      } else if (type === "done") {
        // 完成：两音上行
        tone(660, now, 0.10, "sine", 0.25);
        tone(990, now + 0.10, 0.14, "sine", 0.25);
      } else if (type === "fail") {
        // 抢跑：低沉警告
        tone(180, now, 0.35, "sawtooth", 0.2);
      } else if (type === "record") {
        // 新纪录：上升音阶
        const notes = [523, 659, 784, 1047];
        for (let i = 0; i < notes.length; i++) {
          tone(notes[i], now + i * 0.10, 0.16, "triangle", 0.25);
        }
      }
    } catch (e) {
      // 声音失败不影响游戏主流程
    }
  }

  // 切换声音开关
  function toggleSound() {
    soundOn = !soundOn;
    try { localStorage.setItem("rt_sound", soundOn ? "on" : "off"); } catch (e) {}
    soundBtn.textContent = "声音：" + (soundOn ? "开" : "关");
    // 开启时播放一个小提示音，让玩家确认
    if (soundOn) playSound("done");
  }


  /* ========== 14. 键盘操作 ========== */

  function handleKeyDown(e) {
    // 空格键：代替点击测试区域
    if (e.code === "Space" || e.key === " ") {
      // 按住空格会连续触发，忽略重复事件
      if (e.repeat) { e.preventDefault(); return; }
      // 如果焦点在按钮上，交给按钮自身处理（避免与按钮激活重复）
      const tag = document.activeElement && document.activeElement.tagName;
      if (tag === "BUTTON") return;
      e.preventDefault(); // 阻止页面滚动
      handleAction();
      return;
    }
    // 回车键：触发当前主要按钮
    if (e.code === "Enter" || e.key === "Enter") {
      if (e.repeat) return;
      const tag = document.activeElement && document.activeElement.tagName;
      // 焦点在按钮上时，浏览器会自动触发其 click，不重复处理
      if (tag === "BUTTON") return;
      e.preventDefault();
      // 没在测试中时，回车开始 / 重新测试
      if (gameState === "idle" || gameState === "result") {
        startTest();
      }
    }
  }


  /* ========== 15. 初始化 ========== */

  function init() {
    // 读取本地保存的数据
    loadData();

    // 应用主题（设置 body 属性 + 按钮选中态）
    setTheme(theme);
    // 应用难度（设置按钮选中态 + 文字 + 对应最好成绩）
    setDifficulty(currentDifficulty);
    // 声音按钮文字
    soundBtn.textContent = "声音：" + (soundOn ? "开" : "关");

    // 初始游戏状态
    gameState = "idle";
    setGameArea("idle", "点击下方按钮开始测试", "提示：也可以按空格键操作");
    updateAllDisplay();
    updateButtons();

    // ---- 绑定事件 ----
    // 开始 / 重新测试 都调用 startTest（内部会防重复）
    startBtn.addEventListener("click", startTest);
    restartBtn.addEventListener("click", startTest);
    // 清空成绩
    clearBtn.addEventListener("click", clearScores);
    // 声音开关
    soundBtn.addEventListener("click", toggleSound);
    // 测试区域点击
    gameArea.addEventListener("click", handleAction);

    // 主题按钮
    themeBtns.forEach(function (b) {
      b.addEventListener("click", function () { setTheme(b.dataset.theme); });
    });
    // 难度按钮
    diffBtns.forEach(function (b) {
      b.addEventListener("click", function () { setDifficulty(b.dataset.diff); });
    });

    // 键盘
    document.addEventListener("keydown", handleKeyDown);
  }

  // DOM 加载完成后初始化
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }

})();
