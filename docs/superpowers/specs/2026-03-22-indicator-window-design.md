# Indicator Window 设计规格

## 概述

重新实现语音识别悬浮指示器窗口，解决事件接收失败问题，优化动画流畅度，并实现优雅的退场动画。

## 问题分析

### 当前问题

1. **事件接收失败**：`public/indicator.html` 中的 `window.__TAURI__` 在脚本执行时可能未初始化完成，导致所有事件监听器未注册
2. **窗口隐藏时序冲突**：Rust 端 `hide_indicator_window_delayed()` 使用固定延迟，与前端动画时长不匹配，导致"硬切"
3. **代码冗余**：存在 React 版本 (`src/indicator.tsx`) 未被使用

### 根本原因

```javascript
// 当前代码 - 竞态条件
if (window.__TAURI__ && window.__TAURI__.event) {
  // 如果 Tauri IPC 未就绪，整个块被跳过
  window.__TAURI__.event.listen(...);
}
```

## 技术方案

### 技术选型

- **前端**：纯 HTML/CSS/JS（单文件 `public/indicator.html`）
- **后端**：Rust + Tauri 2.0 事件系统
- **窗口隐藏**：前端主导，动画完成后调用 `window.hide()`

### 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                         Rust Backend                             │
├─────────────────────────────────────────────────────────────────┤
│  录音开始 → emit("recording_status", true)                       │
│  音频数据 → emit("audio_level", rms)  [节流，每50ms]              │
│  流式识别 → emit("stream_update", text)                          │
│  识别处理 → emit("recognition_processing", bool)                 │
│  LLM处理  → emit("llm_processing", bool)                         │
│  会话结束 → emit("session_complete", ())     ← 新增              │
└────────────────────────┬────────────────────────────────────────┘
                         │ Tauri IPC (全局广播)
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    indicator.html (独立窗口)                     │
├─────────────────────────────────────────────────────────────────┤
│  1. 等待 Tauri 就绪 (waitForTauri)                               │
│  2. 注册所有事件监听                                              │
│  3. requestAnimationFrame 驱动动画循环                           │
│  4. 收到 session_complete → 播放退场动画 → 调用 hide()           │
└─────────────────────────────────────────────────────────────────┘
```

## 详细设计

### 1. 前端事件监听初始化

**问题**：脚本执行时 `window.__TAURI__` 可能未就绪

**解决方案**：异步轮询等待 Tauri 就绪

```javascript
function waitForTauri(timeout = 5000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();

    const check = () => {
      if (window.__TAURI__?.event) {
        resolve(window.__TAURI__);
      } else if (Date.now() - start > timeout) {
        reject(new Error('Tauri timeout'));
      } else {
        requestAnimationFrame(check);
      }
    };

    check();
  });
}

// 使用
waitForTauri().then(({ event, window }) => {
  event.listen('audio_level', (e) => { ... });
  event.listen('recording_status', (e) => { ... });
  event.listen('stream_update', (e) => { ... });
  event.listen('recognition_processing', (e) => { ... });
  event.listen('llm_processing', (e) => { ... });
  event.listen('session_complete', () => startFadeOut());

  // 保存 window API 用于隐藏
  window.__TAURI_WINDOW__ = window;
});
```

### 2. 窗口隐藏时序（前端主导）

**Rust 端变更**：

```rust
// 删除函数
// fn hide_indicator_window_delayed<R: Runtime>(...) { ... }

// 新增函数
fn emit_session_complete<R: Runtime>(app_handle: &AppHandle<R>) {
    app_handle.emit("session_complete", ()).ok();
}
```

**前端处理**：

```javascript
function startFadeOut() {
  capsule.classList.add('fade-out');

  // 等待 CSS 动画完成
  setTimeout(() => {
    window.__TAURI_WINDOW__.getCurrentWindow().hide();
    // 重置状态，为下次显示做准备
    resetState();
  }, 500); // 匹配 CSS animation-duration
}
```

### 3. UI 结构

```html
<div class="capsule" id="capsule">
  <div class="dots-wrapper" id="dots">
    <div class="dot dot-1"></div>
    <div class="dot dot-2"></div>
    <div class="dot dot-3"></div>
    <div class="dot dot-4"></div>
  </div>
  <div class="text-scroller" id="scroller">
    <span class="text-content" id="textContent"></span>
  </div>
</div>
```

### 4. 动画状态机

| 状态 | 触发条件 | Dots 表现 | 胶囊表现 |
|------|----------|----------|---------|
| **IDLE** | 初始/退场完成 | 静止 scale(0.8) | 宽度 72px，opacity 0 |
| **RECORDING** | `recording_status=true` | 跟随 `audio_level` 弹跳缩放 | 显示，宽度随文字扩展 |
| **STREAMING** | `stream_update` 有内容 | 同上 | 保持宽度 |
| **PROCESSING** | `recognition_processing=true` | 紫色呼吸辉光，缓慢起伏 | 边框微光 |
| **LLM** | `llm_processing=true` | 紫色辉光加强 | 边框发光加强 |
| **FADE_OUT** | `session_complete` | 淡出 | 宽度收缩 + 透明度归零 |

### 5. 关键 CSS

```css
/* 胶囊容器 */
.capsule {
  background: rgba(15, 23, 42, 0.7);
  backdrop-filter: blur(16px);
  border-radius: 40px;
  height: 48px;
  min-width: 72px;
  max-width: 72px;
  transition: max-width 0.6s cubic-bezier(0.16, 1, 0.3, 1),
              opacity 0.4s ease,
              transform 0.4s ease;
}

.capsule.has-text {
  max-width: 600px;
  padding: 12px 24px;
}

.capsule.processing {
  border-color: rgba(167, 139, 250, 0.4);
  box-shadow: 0 0 25px -5px rgba(167, 139, 250, 0.5);
}

/* 退场动画 */
.capsule.fade-out {
  animation: fadeOut 0.5s ease-out forwards;
}

@keyframes fadeOut {
  0%   { opacity: 1; transform: scale(1); max-width: 600px; }
  100% { opacity: 0; transform: scale(0.9); max-width: 72px; }
}

/* 文字滚动遮罩 */
.text-scroller {
  overflow-x: hidden;
  white-space: nowrap;
  mask-image: linear-gradient(
    to right,
    transparent 0%,
    black 10%,
    black 90%,
    transparent 100%
  );
  opacity: 0;
  width: 0;
  transition: opacity 0.4s ease, width 0.6s cubic-bezier(0.16, 1, 0.3, 1);
}

.capsule.has-text .text-scroller {
  opacity: 1;
  width: 100%;
}

/* Dots 颜色 */
.dot-1 { background: #38bdf8; } /* 天蓝 */
.dot-2 { background: #34d399; } /* 翠绿 */
.dot-3 { background: #a78bfa; } /* 紫色 */
.dot-4 { background: #f472b6; } /* 粉红 */
```

### 6. 动画循环

```javascript
function animate() {
  const time = Date.now();

  if (isThinking) {
    // 处理中：紫色呼吸
    dots.forEach((dot, i) => {
      const scale = 0.6 + Math.sin(time / 300 + i * 0.8) * 0.6;
      const y = Math.sin(time / 150 + i) * 3;
      dot.style.transform = `scale(${Math.max(0.4, scale)}) translateY(${y}px)`;
      dot.style.filter = 'brightness(1.5) drop-shadow(0 0 4px rgba(167,139,250,0.8))';
    });
  } else if (isRecording || smoothedLevel > 0.01) {
    // 录音中：跟随音量
    smoothedLevel += (targetLevel - smoothedLevel) * 0.25;

    dots.forEach((dot, i) => {
      const baseScale = 0.8 + Math.sin(time / 150 + i * 1.5) * 0.2;
      const voiceScale = 1 + smoothedLevel * (1.5 + (i % 2) * 0.5);
      const scale = Math.min(3.5, baseScale * voiceScale);
      const y = -smoothedLevel * 4;
      dot.style.transform = `scale(${scale}) translateY(${y}px)`;
      dot.style.filter = 'drop-shadow(0 2px 4px rgba(0,0,0,0.3))';
    });
  } else {
    // 静止
    dots.forEach(dot => {
      dot.style.transform = 'scale(0.8) translateY(0)';
      dot.style.filter = 'none';
    });
  }

  requestAnimationFrame(animate);
}
```

## 文件变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `public/indicator.html` | 重写 | 修复事件监听 + 优化动画 |
| `src/indicator.tsx` | 删除 | 不再使用 React 版本 |
| `src/main.tsx` | 修改 | 删除 Indicator 导入和条件渲染 |
| `src/components/VolumeIndicator.tsx` | 删除 | 冗余组件 |
| `src-tauri/src/lib.rs` | 修改 | 删除 `hide_indicator_window_delayed`，新增 `emit_session_complete` |

## 验收标准

1. 录音开始时，indicator 窗口显示，4 个 dots 跟随音量跳动
2. 流式识别文字实时显示，胶囊平滑展开
3. 文字过长时，左侧渐变消隐，新字始终在右侧
4. LLM 处理时，dots 显示紫色呼吸辉光
5. 会话结束时，播放 0.5s 退场动画后窗口隐藏
6. 无"硬切"或闪烁现象
