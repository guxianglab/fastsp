# FastSP

**FastSP 是一款面向 Windows 的 AI 语音输入法**：按住说话，松开即上屏；既能做**高速听写**，也能做**语音指令（Skills）**。语音识别基于**在线流式 ASR**，可选开启 **LLM 二次纠错**，让口述更像"写出来的文字"。

- **流式识别**：在线 ASR 实时返回中间结果与最终结果
- **全局输入**：在任何可输入的地方直接上屏（聊天、浏览器、IDE、系统输入框…）
- **两种模式**：听写（Dictation）/ 指令（Skills）
- **可选 AI 纠错**：OpenAI 兼容接口，支持提示词"场景"管理

## 下载与安装（推荐）

- **Windows 安装包（NSIS）**：到 [Releases](https://github.com/guxiangjun/fastsp/releases) 下载最新版本并安装

> 开发者构建请看下方「开发与构建」。

## 30 秒上手

1. **首次启动**：打开 Settings，填入在线 ASR 凭证
2. **选择麦克风**：Settings → `Audio Input` → 选择 Input Device（可 Test）
3. **选一个触发方式**（建议先用鼠标中键）：
   - **听写模式（上屏文字）**
     - 鼠标 **中键按住**：按住说话，松开上屏
     - **右 Alt**：按一次开始录音，再按一次结束并上屏
   - **指令模式（Skills）**
     - **Ctrl + Win 按住**：按住说话，松开触发技能（不会上屏文本）

录音时鼠标附近会出现**青色指示器**；开启 LLM 纠错且正在处理时会显示**红色指示器**。

## 为什么 FastSP 更像"输入法"

- **上屏策略更稳**：使用"剪贴板写入 + Ctrl+V"粘贴，兼容很多 Windows 原生控件（如资源管理器地址栏、系统搜索框等）
- **"说"与"写"的差距更小**：可选 LLM 纠错，专注修正同音字、漏字、标点与语序，不改语义
- **指令即技能**：把语音从"输入"升级成"动作"（打开计算器/截图/打开资源管理器…）

## 功能一览

- **触发方式**
  - 鼠标中键按住（听写）
  - 右 Alt 切换（听写）
  - Ctrl + Win 按住（Skills）
- **多语言**：自动/中文/英文/日语/韩语/粤语
- **在线 ASR**
  - 在 Settings 中配置 App Key、Access Token、Resource ID
  - 使用流式 WebSocket 识别，支持实时预览文本
  - 支持代理（HTTP / SOCKS5）
- **LLM 纠错（可选）**
  - OpenAI 兼容 API（也适配常见第三方/自部署网关）
  - 提示词"场景（Profiles）"管理：新建/删除/切换/重置
  - 一键测试连通性
- **Skills（语音指令）**
  - 默认内置：写邮件、计算器、浏览器、记事本、文件管理器、截图
  - 每个技能可开关 + 自定义关键词（逗号分隔）
- **历史记录**：自动保存、复制、清空

## 隐私与安全

- **语音识别会发送到你配置的 ASR 服务**：应用会将音频发送到 Settings 中配置的在线 ASR 服务。
- **LLM 纠错是可选项**：仅在你开启后，才会把**识别出的文本**发送到你配置的 LLM 接口进行纠错。
- **可控可审计**：所有配置均在本地文件中，随时可关闭、清空历史。

## 配置与数据位置

- **配置文件**：`%APPDATA%\com.fastsp\config.json`
- **历史记录**：`%APPDATA%\com.fastsp\history.json`
配置示例（以实际版本为准）：

```json
{
  "trigger_mouse": true,
  "trigger_hold": true,
  "trigger_toggle": true,
  "online_asr_config": {
    "app_key": "",
    "access_key": "",
    "resource_id": "volc.bigasr.sauc.duration"
  },
  "input_device": "",
  "llm_config": {
    "enabled": false,
    "base_url": "https://api.openai.com/v1",
    "api_key": "",
    "model": "gpt-4o-mini",
    "profiles": [
      { "id": "default", "name": "默认", "content": "" }
    ],
    "active_profile_id": "default"
  },
  "proxy": {
    "enabled": false,
    "url": "http://127.0.0.1:7890"
  },
  "skills": [
    {
      "id": "open_calculator",
      "name": "计算器",
      "keywords": "计算器,算一下,打开计算器",
      "enabled": true
    }
  ]
}
```

### LLM 提示词（Profiles）怎么写

- **占位符**：使用 `{text}` 表示待纠错文本
- **输出格式**：必须返回 JSON，例如：

```json
{"corrected":"纠正后的文本"}
```

> 小建议：把不同场景拆开（会议纪要/写代码/写邮件/客服话术），效果会比一个"万能提示词"稳定很多。

### 代理（Proxy）

代理会同时作用于：
- 在线 ASR 请求
- LLM API 请求

示例：
- `http://127.0.0.1:7890`
- `socks5://127.0.0.1:1080`

## 开发与构建

### 环境要求

- Windows 10/11
- Node.js 18+
- Rust（建议使用最新稳定版）

### 安装依赖

```bash
npm install
```

### 开发模式

```bash
npm run tauri dev
```

### 构建安装包

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`（包含 Windows NSIS 安装包）。

## 技术栈（实现概要）

- **前端**：React + TypeScript + TailwindCSS + Vite
- **桌面端**：Tauri v2 + Rust
- **ASR**：在线流式 WebSocket ASR
- **音频采集**：cpal
- **全局输入监听**：rdev
- **上屏**：剪贴板 + Ctrl+V（enigo）
- **网络**：reqwest（支持 HTTP / SOCKS5 代理）

## 常见问题（FAQ）

### 1) 为什么 Ctrl+Win 没有上屏文字？

这是 **Skills 模式**：Ctrl+Win 触发会把识别文本用于匹配技能并执行动作，默认**不做上屏**。要听写请用鼠标中键或右 Alt。

### 2) 在线 ASR 连接失败？

先检查 Settings 里的 App Key、Access Token、Resource ID 是否填写正确；如果你在代理环境下，再到 Settings → Network 里配置 HTTP/SOCKS5 代理。

### 3) 我担心隐私问题

语音识别会使用你配置的在线 ASR 服务；如果开启 LLM 纠错，识别后的**文本**还会发送到你配置的 LLM 服务端。你随时可以关闭 LLM 纠错。

## 贡献

欢迎提 Issue / PR。建议：
- Issue 描述里包含：触发方式、目标应用、复现步骤、日志/截图
- PR 尽量保持改动聚焦，避免引入多余状态与复杂度

## 许可证

MIT License
