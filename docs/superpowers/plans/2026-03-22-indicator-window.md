# Indicator Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the indicator window to receive Tauri events and implement smooth fade-out animations with frontend-controlled window hiding.

**Architecture:** Pure HTML/CSS/JS indicator window that waits for Tauri IPC to be ready before registering event listeners. Rust backend emits a new `session_complete` event instead of directly hiding the window, allowing frontend to play exit animation first.

**Tech Stack:** Tauri 2.0, vanilla JavaScript, CSS animations

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `public/indicator.html` | Rewrite | Complete indicator UI with fixed event handling |
| `src/indicator.tsx` | Delete | Unused React version |
| `src/main.tsx` | Modify | Remove Indicator import |
| `src/components/VolumeIndicator.tsx` | Delete | Unused component |
| `src-tauri/src/lib.rs` | Modify | Replace `hide_indicator_window_delayed` with `emit_session_complete` |

---

## Task 1: Rewrite indicator.html with Fixed Event Handling

**Files:**
- Rewrite: `public/indicator.html`

**Context:** The current indicator.html has a race condition where `window.__TAURI__` may not be ready when the script executes. We need to wait for Tauri to be ready before registering event listeners.

- [ ] **Step 1: Create the complete indicator.html with waitForTauri pattern**

Write the complete file to `public/indicator.html`:

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Indicator</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }

    html, body {
      width: 100%;
      height: 100%;
      background: transparent;
      overflow: hidden;
      user-select: none;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    }

    #app {
      width: 100%;
      height: 100%;
      display: flex;
      align-items: center;
      justify-content: center;
      background: transparent;
    }

    /* ─────────────────────────────────────────────
       胶囊容器 - Glassmorphic Pill
       ───────────────────────────────────────────── */
    .capsule {
      background: rgba(15, 23, 42, 0.7);
      backdrop-filter: blur(16px);
      -webkit-backdrop-filter: blur(16px);
      border: 1px solid rgba(255, 255, 255, 0.08);
      border-radius: 40px;
      padding: 12px 18px;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 16px;
      height: 48px;
      min-width: 72px;
      max-width: 72px;
      transition:
        max-width 0.6s cubic-bezier(0.16, 1, 0.3, 1),
        opacity 0.4s ease,
        transform 0.4s ease,
        box-shadow 0.4s ease,
        border-color 0.4s ease;
      box-shadow:
        0 10px 30px -5px rgba(0, 0, 0, 0.5),
        inset 0 0 0 1px rgba(255, 255, 255, 0.05);
      /* 初始隐藏，等待显示 */
      opacity: 0;
    }

    .capsule.visible {
      opacity: 1;
    }

    .capsule.has-text {
      max-width: 600px;
      padding: 12px 24px;
      background: rgba(15, 23, 42, 0.85);
      box-shadow:
        0 15px 35px -5px rgba(0, 0, 0, 0.6),
        0 0 30px -10px rgba(79, 157, 154, 0.3);
    }

    .capsule.processing {
      background: rgba(15, 23, 42, 0.95);
      border-color: rgba(167, 139, 250, 0.4);
      box-shadow:
        0 0 25px -5px rgba(167, 139, 250, 0.5),
        inset 0 0 0 1px rgba(255, 255, 255, 0.1);
    }

    /* ─────────────────────────────────────────────
       退场动画
       ───────────────────────────────────────────── */
    .capsule.fade-out {
      animation: fadeOut 0.5s ease-out forwards;
    }

    @keyframes fadeOut {
      0% {
        opacity: 1;
        transform: scale(1);
      }
      100% {
        opacity: 0;
        transform: scale(0.9);
      }
    }

    /* ─────────────────────────────────────────────
       Dots 容器
       ───────────────────────────────────────────── */
    .dots-wrapper {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 6px;
      height: 24px;
      flex-shrink: 0;
    }

    .dot {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      transition: transform 0.1s ease-out, filter 0.3s ease;
      transform-origin: center;
    }

    .dot-1 { background: #38bdf8; } /* 天蓝 */
    .dot-2 { background: #34d399; } /* 翠绿 */
    .dot-3 { background: #a78bfa; } /* 紫色 */
    .dot-4 { background: #f472b6; } /* 粉红 */

    /* ─────────────────────────────────────────────
       文字滚动容器
       ───────────────────────────────────────────── */
    .text-scroller {
      overflow-x: hidden;
      white-space: nowrap;
      scroll-behavior: smooth;
      /* 左侧渐变消隐遮罩 */
      -webkit-mask-image: linear-gradient(
        to right,
        transparent 0%,
        black 24px,
        black calc(100% - 24px),
        transparent 100%
      );
      mask-image: linear-gradient(
        to right,
        transparent 0%,
        black 24px,
        black calc(100% - 24px),
        transparent 100%
      );
      opacity: 0;
      width: 0;
      max-width: 500px;
      transition:
        opacity 0.4s ease,
        width 0.6s cubic-bezier(0.16, 1, 0.3, 1);
    }

    .capsule.has-text .text-scroller {
      opacity: 1;
      width: 100%;
    }

    .text-content {
      color: #f8fafc;
      font-size: 15px;
      font-weight: 400;
      letter-spacing: 0.5px;
      display: inline-block;
      padding-right: 8px;
      text-shadow: 0 2px 4px rgba(0, 0, 0, 0.5);
    }

    /* ─────────────────────────────────────────────
       错误状态显示
       ───────────────────────────────────────────── */
    .error-message {
      position: fixed;
      bottom: 10px;
      left: 50%;
      transform: translateX(-50%);
      background: rgba(239, 68, 68, 0.9);
      color: white;
      padding: 8px 16px;
      border-radius: 8px;
      font-size: 12px;
      display: none;
    }

    .error-message.visible {
      display: block;
    }
  </style>
</head>
<body>
  <div id="app">
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
  </div>
  <div class="error-message" id="errorMessage"></div>

  <script>
    // ─────────────────────────────────────────────
    // DOM 元素引用
    // ─────────────────────────────────────────────
    const capsule = document.getElementById('capsule');
    const scroller = document.getElementById('scroller');
    const textContent = document.getElementById('textContent');
    const errorMessage = document.getElementById('errorMessage');
    const dots = Array.from(document.querySelectorAll('.dot'));

    // ─────────────────────────────────────────────
    // 状态变量
    // ─────────────────────────────────────────────
    let isRecording = false;
    let isProcessing = false;
    let isLlmProcessing = false;
    let streamText = '';
    let isFadingOut = false;

    // 音量平滑
    let targetLevel = 0;
    let smoothedLevel = 0;

    // Tauri API 引用
    let tauriEvent = null;
    let tauriWindow = null;

    // ─────────────────────────────────────────────
    // 工具函数
    // ─────────────────────────────────────────────

    /**
     * 等待 Tauri IPC 就绪
     * 解决 window.__TAURI__ 未初始化的竞态条件
     */
    function waitForTauri(timeout = 5000) {
      return new Promise((resolve, reject) => {
        const startTime = Date.now();

        function check() {
          if (window.__TAURI__ && window.__TAURI__.event) {
            resolve(window.__TAURI__);
          } else if (Date.now() - startTime > timeout) {
            reject(new Error('Tauri IPC timeout after ' + timeout + 'ms'));
          } else {
            requestAnimationFrame(check);
          }
        }

        check();
      });
    }

    /**
     * 显示错误信息
     */
    function showError(msg) {
      console.error('[Indicator]', msg);
      errorMessage.textContent = msg;
      errorMessage.classList.add('visible');
      setTimeout(() => {
        errorMessage.classList.remove('visible');
      }, 5000);
    }

    /**
     * 重置所有状态
     */
    function resetState() {
      capsule.classList.remove('has-text', 'processing', 'fade-out', 'visible');
      streamText = '';
      textContent.textContent = '';
      targetLevel = 0;
      smoothedLevel = 0;
      isRecording = false;
      isProcessing = false;
      isLlmProcessing = false;
      isFadingOut = false;
    }

    /**
     * 滚动到最右侧
     */
    function scrollToEnd() {
      scroller.scrollTo({
        left: scroller.scrollWidth,
        behavior: 'smooth'
      });
    }

    /**
     * 判断是否处于思考状态
     */
    function isThinking() {
      return !isRecording && (isProcessing || isLlmProcessing);
    }

    /**
     * 开始退场动画
     */
    function startFadeOut() {
      if (isFadingOut) return;
      isFadingOut = true;

      capsule.classList.add('fade-out');

      // 等待动画完成后隐藏窗口
      setTimeout(() => {
        if (tauriWindow) {
          tauriWindow.getCurrentWindow().hide().catch(err => {
            console.error('[Indicator] Failed to hide window:', err);
          });
        }
        // 重置状态为下次显示做准备
        resetState();
      }, 500); // 匹配 CSS animation-duration
    }

    // ─────────────────────────────────────────────
    // 动画循环
    // ─────────────────────────────────────────────
    function animate() {
      const time = Date.now();

      if (isThinking()) {
        // 思考状态：紫色呼吸辉光
        dots.forEach((dot, i) => {
          const scale = Math.max(0.4, 0.6 + Math.sin(time / 300 + i * 0.8) * 0.6);
          const y = Math.sin(time / 150 + i) * 3;
          dot.style.transform = `scale(${scale}) translateY(${y}px)`;
          dot.style.filter = 'brightness(1.5) drop-shadow(0 0 4px rgba(167, 139, 250, 0.8))';
        });
      } else if (isRecording || smoothedLevel > 0.01) {
        // 录音状态：跟随音量跳动
        smoothedLevel += (targetLevel - smoothedLevel) * 0.25;

        dots.forEach((dot, i) => {
          const baseScale = 0.8 + Math.sin(time / 150 + i * 1.5) * 0.2;
          const voiceScale = 1 + smoothedLevel * (1.5 + (i % 2) * 0.5);
          const scale = Math.min(3.5, baseScale * voiceScale);
          const y = -smoothedLevel * 4;
          dot.style.transform = `scale(${scale}) translateY(${y}px)`;
          dot.style.filter = 'drop-shadow(0 2px 4px rgba(0, 0, 0, 0.3))';
        });
      } else {
        // 静止状态
        dots.forEach(dot => {
          dot.style.transform = 'scale(0.8) translateY(0)';
          dot.style.filter = 'none';
        });
      }

      requestAnimationFrame(animate);
    }

    // ─────────────────────────────────────────────
    // 事件处理
    // ─────────────────────────────────────────────
    function setupEventListeners(event) {
      // 音量级别
      event.listen('audio_level', (e) => {
        // 放大数据让反应更灵敏
        const raw = Math.min(e.payload * 8, 1.0);
        targetLevel = Math.sqrt(raw);
      });

      // 录音状态
      event.listen('recording_status', (e) => {
        isRecording = e.payload;
        console.log('[Indicator] Recording:', isRecording);

        if (isRecording) {
          capsule.classList.add('visible');
        } else {
          targetLevel = 0;
          // 录音结束，如果没有后置处理就准备退场
          if (!isProcessing && !isLlmProcessing) {
            setTimeout(() => {
              if (!isProcessing && !isLlmProcessing && !isFadingOut) {
                startFadeOut();
              }
            }, 1000);
          }
        }
      });

      // 流式文字更新
      event.listen('stream_update', (e) => {
        streamText = e.payload || '';
        if (streamText.length > 0) {
          capsule.classList.add('has-text');
          textContent.textContent = streamText;
          setTimeout(scrollToEnd, 50);
        }
      });

      // 识别处理状态
      event.listen('recognition_processing', (e) => {
        isProcessing = e.payload;
        console.log('[Indicator] Recognition processing:', isProcessing);

        if (isProcessing) {
          capsule.classList.add('processing');
        } else if (!isLlmProcessing && !isRecording) {
          capsule.classList.remove('processing');
          capsule.classList.remove('has-text');
          setTimeout(() => {
            textContent.textContent = '';
            streamText = '';
          }, 400);
        }
      });

      // LLM 处理状态
      event.listen('llm_processing', (e) => {
        isLlmProcessing = e.payload;
        console.log('[Indicator] LLM processing:', isLlmProcessing);

        if (isLlmProcessing) {
          capsule.classList.add('processing');
        } else if (!isProcessing && !isRecording) {
          capsule.classList.remove('processing');
          capsule.classList.remove('has-text');
          setTimeout(() => {
            textContent.textContent = '';
            streamText = '';
          }, 400);
        }
      });

      // 会话结束 - 开始退场
      event.listen('session_complete', () => {
        console.log('[Indicator] Session complete, starting fade-out');
        startFadeOut();
      });

      console.log('[Indicator] All event listeners registered');
    }

    // ─────────────────────────────────────────────
    // 初始化
    // ─────────────────────────────────────────────
    async function init() {
      console.log('[Indicator] Initializing...');

      try {
        // 等待 Tauri 就绪
        const tauri = await waitForTauri();
        console.log('[Indicator] Tauri IPC ready');

        // 保存 API 引用
        tauriEvent = tauri.event;
        tauriWindow = tauri.window;

        // 注册事件监听
        setupEventListeners(tauriEvent);

        // 启动动画循环
        animate();

        console.log('[Indicator] Initialization complete');
      } catch (err) {
        showError('初始化失败: ' + err.message);
        console.error('[Indicator] Init failed:', err);
      }
    }

    // 页面加载完成后初始化
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', init);
    } else {
      init();
    }
  </script>
</body>
</html>
```

- [ ] **Step 2: Verify the file was written correctly**

Run: `head -50 public/indicator.html`
Expected: The file starts with `<!DOCTYPE html>`

- [ ] **Step 3: Commit**

```bash
git add public/indicator.html
git commit -m "fix(indicator): rewrite with waitForTauri pattern and proper event handling

- Add waitForTauri() to wait for Tauri IPC to be ready
- Fix race condition where event listeners were never registered
- Add session_complete event handler for graceful fade-out
- Add resetState() function to clean up between sessions
- Add visible class for initial show animation
- Add error message display for debugging"
```

---

## Task 2: Modify Rust Backend to Emit session_complete Event

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Context:** Replace `hide_indicator_window_delayed()` calls with `emit_session_complete()` to let the frontend control the hide timing after animation.

- [ ] **Step 1: Add emit_session_complete function**

In `src-tauri/src/lib.rs`, add this function after `hide_indicator_window_delayed` (around line 152):

```rust
/// Emit session_complete event to let frontend handle fade-out animation
fn emit_session_complete<R: Runtime>(app_handle: &AppHandle<R>) {
    app_handle.emit("session_complete", ()).ok();
}
```

- [ ] **Step 2: Replace hide_indicator_window_delayed calls with emit_session_complete**

Find and replace all occurrences of `hide_indicator_window_delayed`:

1. **In `process_transcription()`** (around line 238 and 255):
   - Replace `hide_indicator_window_delayed(&app_handle_clone, 1500);` with `emit_session_complete(&app_handle_clone);`

2. **In `stop_and_transcribe()`** (around line 538 and 553):
   - Replace `hide_indicator_window_delayed(app_handle, 1500);` with `emit_session_complete(app_handle);`

3. **In `begin_recording_session()` error path** (around line 458):
   - Replace `hide_indicator_window_delayed(app_handle, 500);` with `emit_session_complete(app_handle);`

- [ ] **Step 3: Delete the hide_indicator_window_delayed function**

Delete the entire `hide_indicator_window_delayed` function (lines 143-151):

```rust
// DELETE THIS ENTIRE FUNCTION:
/// Hide the indicator window with a delay (allows CSS fade-out animations to finish)
fn hide_indicator_window_delayed<R: Runtime>(app_handle: &AppHandle<R>, delay_ms: u64) {
    let handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        if let Some(window) = handle.get_webview_window("indicator") {
            window.hide().ok();
        }
    });
}
```

- [ ] **Step 4: Verify the changes compile**

Run: `cd src-tauri && cargo check 2>&1 | head -30`
Expected: No errors, compilation succeeds

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(backend): replace hide_indicator_window_delayed with emit_session_complete

- Add emit_session_complete() to signal frontend to start fade-out
- Remove hide_indicator_window_delayed() - frontend now controls hide timing
- This allows frontend to complete fade-out animation before hiding"
```

---

## Task 3: Delete Unused React Indicator Files

**Files:**
- Delete: `src/indicator.tsx`
- Delete: `src/components/VolumeIndicator.tsx`
- Modify: `src/main.tsx`

**Context:** Clean up unused React-based indicator implementations.

- [ ] **Step 1: Delete src/indicator.tsx**

Run: `rm src/indicator.tsx`

- [ ] **Step 2: Delete src/components/VolumeIndicator.tsx**

Run: `rm src/components/VolumeIndicator.tsx`

- [ ] **Step 3: Update src/main.tsx to remove Indicator import**

In `src/main.tsx`, remove the Indicator import and conditional rendering.

Current content:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { Indicator } from "./indicator";

const isIndicator = window.location.search.includes("mode=indicator");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isIndicator ? <Indicator /> : <App />}
  </React.StrictMode>,
);
```

Replace with:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 4: Verify frontend builds**

Run: `npm run build`
Expected: Build succeeds without errors

- [ ] **Step 5: Commit**

```bash
git add src/indicator.tsx src/components/VolumeIndicator.tsx src/main.tsx
git commit -m "chore: remove unused React indicator components

- Delete src/indicator.tsx - replaced by pure HTML version
- Delete src/components/VolumeIndicator.tsx - unused
- Simplify src/main.tsx - remove indicator routing logic"
```

---

## Task 4: Integration Testing

**Files:**
- None (manual testing)

**Context:** Verify the complete flow works end-to-end.

- [ ] **Step 1: Start the development server**

Run: `npm run tauri dev`

Expected: App launches, indicator window is created (hidden initially)

- [ ] **Step 2: Test recording trigger**

Action: Press the recording hotkey (mouse middle button or configured trigger)

Expected:
1. Indicator window appears at bottom-center
2. 4 dots start animating
3. Dots respond to audio level (bounce when speaking)

- [ ] **Step 3: Test streaming text**

Action: Speak a sentence

Expected:
1. Capsule expands horizontally as text appears
2. Text scrolls left, new text appears on right
3. Left edge has fade-out mask for overflow

- [ ] **Step 4: Test LLM processing**

Action: Release recording trigger (if LLM is enabled)

Expected:
1. Dots change to purple breathing glow
2. Capsule border has purple glow
3. After LLM completes, fade-out animation plays
4. Window hides after 0.5s

- [ ] **Step 5: Test without LLM**

Action: Disable LLM in settings, test recording

Expected:
1. After recording ends, 1s delay then fade-out
2. Window hides smoothly

- [ ] **Step 6: Verify console logs**

Open DevTools on indicator window (if possible) or check terminal output.

Expected: See `[Indicator]` log messages showing:
- "Initializing..."
- "Tauri IPC ready"
- "All event listeners registered"
- "Recording: true/false"
- "Session complete, starting fade-out"

---

## Verification Checklist

After completing all tasks, verify:

- [ ] `public/indicator.html` contains `waitForTauri()` function
- [ ] `src/indicator.tsx` has been deleted
- [ ] `src/components/VolumeIndicator.tsx` has been deleted
- [ ] `src/main.tsx` no longer imports Indicator
- [ ] `src-tauri/src/lib.rs` has `emit_session_complete()` function
- [ ] `src-tauri/src/lib.rs` no longer has `hide_indicator_window_delayed()` function
- [ ] `cargo check` passes in src-tauri
- [ ] `npm run build` passes
- [ ] Manual testing shows dots responding to audio
- [ ] Manual testing shows fade-out animation before window hides
