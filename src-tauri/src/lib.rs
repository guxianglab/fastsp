mod asr;
mod audio;
mod http_client;
mod input_listener;
mod llm;
mod skills;
mod storage;
// TODO: 流式模块暂时禁用，等待完整集成
// mod streaming_asr;

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use storage::{AppConfig, HistoryItem, LlmConfig, PromptProfile, ProxyConfig};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

// Define State Types
type AudioState = Mutex<audio::AudioService>;
type AsrState = asr::AsrService;
type StorageState = storage::StorageService;
type InputListenerState = input_listener::InputListener;
type ProcessingState = Arc<std::sync::atomic::AtomicBool>; // 防止重复处理（跨线程/异步任务共享）
type LlmCancelState = Arc<Mutex<Option<CancellationToken>>>; // LLM 请求取消令牌
// type StreamingAsrState = streaming_asr::StreamingAsrService;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Instant;

use enigo::{Enigo, Key, Keyboard, Settings};
use arboard::Clipboard;

// Monotonic id to correlate a single transcription pipeline across logs.
static TRANSCRIPTION_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordingMode {
    Dictation,
    Skill,
}

fn preview_text(s: &str, max_chars: usize) -> String {
    // Keep logs readable: single-line preview with a hard cap.
    let mut out = String::with_capacity(max_chars.min(s.len()));
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' || ch == '\t' {
            out.push(' ');
        } else {
            out.push(ch);
        }
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out
}

// Indicator window colors
const INDICATOR_COLOR_RECORDING: &str = "#4f9d9a"; // Indigo-cyan for normal recording
const INDICATOR_COLOR_LLM: &str = "#dc2626"; // Red for LLM processing

/// Show the indicator window and set its color
fn show_indicator_window<R: Runtime>(app_handle: &AppHandle<R>, is_llm: bool) {
    if let Some(window) = app_handle.get_webview_window("indicator") {
        let color = if is_llm { INDICATOR_COLOR_LLM } else { INDICATOR_COLOR_RECORDING };
        
        let listener = app_handle.state::<InputListenerState>();
        let (x, y) = listener.get_last_mouse_position();
        
        // Find the monitor where the text is being entered (or mouse is present)
        if let Ok(monitors) = app_handle.available_monitors() {
            for monitor in monitors {
                let pos = monitor.position();
                let size = monitor.size();
                
                let in_x = x >= pos.x as f64 && x < (pos.x + size.width as i32) as f64;
                let in_y = y >= pos.y as f64 && y < (pos.y + size.height as i32) as f64;
                
                if in_x && in_y {
                    let scale_factor = monitor.scale_factor();
                    
                    // Allow more width so the capsule can expand dynamically for text
                    let logical_width = 800.0;
                    let logical_height = 100.0;
                    let bottom_margin = 40.0; // Distance from bottom
                    
                    // Calculate physical positions for bottom center of this monitor
                    let physical_center_x = pos.x as f64 + (size.width as f64 / 2.0);
                    let physical_bottom_y = pos.y as f64 + size.height as f64;
                    
                    let window_x = physical_center_x - (logical_width * scale_factor / 2.0);
                    let window_y = physical_bottom_y - ((logical_height + bottom_margin) * scale_factor);
                    
                    let window_pos = tauri::PhysicalPosition::new(window_x as i32, window_y as i32);
                    window.set_position(window_pos).ok();
                    break;
                }
            }
        }
        
        window.emit("indicator_color", color).ok();
        window.show().ok();
    }
}

/// Emit session_complete event to let frontend handle fade-out animation
fn emit_session_complete<R: Runtime>(app_handle: &AppHandle<R>) {
    app_handle.emit("session_complete", ()).ok();
}

fn show_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        window.show().ok();
        window.unminimize().ok();
        window.set_focus().ok();
    }
}

fn hide_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        window.hide().ok();
    }
    if let Some(indicator) = app_handle.get_webview_window("indicator") {
        indicator.hide().ok();
    }
}

/// Process transcribed text: apply LLM correction if enabled, save to history, emit event, paste
fn process_transcription<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    seq_id: u64,
) {
    if text.trim().is_empty() {
        println!("[TRANSCRIPTION] #{} empty, skipping", seq_id);
        processing.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }
    
    println!(
        "[TRANSCRIPTION] #{} Processing: {} chars, preview='{}'",
        seq_id,
        text.len(),
        preview_text(&text, 80)
    );

    let storage = app_handle.state::<StorageState>();
    let config = storage.load_config();
    let llm_config = config.llm_config.clone();
    let proxy_config = config.proxy.clone();

    let app_handle_clone = app_handle.clone();
    let processing_clone = processing.clone();
    let llm_cancel_clone = llm_cancel.clone();

    // Use tokio runtime to handle async LLM correction
    tauri::async_runtime::spawn(async move {
        // Always clear the processing flag when this async pipeline is done
        struct ProcessingGuard(ProcessingState);
        impl Drop for ProcessingGuard {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = ProcessingGuard(processing_clone);

        let final_text = if llm_config.enabled {
            // Create cancellation token for this LLM request
            let cancel_token = CancellationToken::new();
            {
                if let Ok(mut guard) = llm_cancel_clone.lock() {
                    *guard = Some(cancel_token.clone());
                }
            }

            app_handle_clone.emit("llm_processing", true).ok();
            {
                let listener = app_handle_clone.state::<InputListenerState>();
                listener.track_mouse_position.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            show_indicator_window(&app_handle_clone, true);

            // Use tokio::select! to race between LLM request and cancellation
            let llm_result = tokio::select! {
                result = llm::correct_text(&text, &llm_config, &proxy_config) => {
                    Some(result)
                }
                _ = cancel_token.cancelled() => {
                    println!("[TRANSCRIPTION] #{} LLM request cancelled", seq_id);
                    None
                }
            };

            // Clear the cancel token
            {
                if let Ok(mut guard) = llm_cancel_clone.lock() {
                    *guard = None;
                }
            }

            app_handle_clone.emit("llm_processing", false).ok();
            {
                let listener = app_handle_clone.state::<InputListenerState>();
                listener.track_mouse_position.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            emit_session_complete(&app_handle_clone);

            match llm_result {
                Some(Ok(outcome)) => {
                    println!(
                        "[TRANSCRIPTION] #{} scene='{}' fallback={}",
                        seq_id,
                        outcome.applied_scene,
                        outcome.fallback_reason.is_some()
                    );
                    if let Some(reason) = outcome.fallback_reason.clone() {
                        app_handle_clone.emit("llm_error", reason).ok();
                    }
                    outcome.final_text
                }
                Some(Err(e)) => {
                    eprintln!("LLM correction failed, using original text: {}", e);
                    // Emit error event for frontend
                    app_handle_clone.emit("llm_error", e.to_string()).ok();
                    text
                }
                None => {
                    // Cancelled - don't output anything
                    println!("[TRANSCRIPTION] #{} aborted due to cancellation", seq_id);
                    return;
                }
            }
        } else {
            emit_session_complete(&app_handle_clone);
            text
        };

        if final_text.trim().is_empty() {
            println!("[TRANSCRIPTION] #{} final empty, skipping", seq_id);
            return;
        }

        // Save to history
        let item = HistoryItem {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            text: final_text.clone(),
            duration_ms: 0,
        };
        let storage = app_handle_clone.state::<StorageState>();
        storage.add_history_item(item.clone()).ok();
        app_handle_clone.emit("transcription_update", item).ok();

        // Output text (blocking, on a dedicated thread to not block tokio)
        let text_to_paste = final_text;
        let id = seq_id;
        std::thread::spawn(move || {
            output_text(&text_to_paste, id);
        }).join().ok();
    });
}

/// Process transcription for a skill trigger
/// Matches recognized text against skill keywords and executes matching skill
fn process_transcription_for_skill<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    processing: ProcessingState,
    seq_id: u64,
) {
    if text.trim().is_empty() {
        println!("[SKILL] #{} empty, skipping", seq_id);
        processing.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    println!(
        "[SKILL] #{} Processing: {} chars, preview='{}'",
        seq_id,
        text.len(),
        preview_text(&text, 80)
    );

    let storage = app_handle.state::<StorageState>();
    let mut config = storage.load_config();
    let skills_config = config.skills.clone();

    // Try to match one or more skills in the transcript.
    let matched_skills = skills::match_skills(&text, &skills_config);
    if !matched_skills.is_empty() {
        let matched_ids: Vec<&str> = matched_skills
            .iter()
            .map(|skill_match| skill_match.skill_id.as_str())
            .collect();
        println!(
            "[SKILL] #{} Matched skills: {}",
            seq_id,
            matched_ids.join(", ")
        );

        for (index, skill_match) in matched_skills.iter().enumerate() {
            let next_match = matched_skills.get(index + 1);

            if skills::is_config_skill(&skill_match.skill_id) {
                match execute_config_skill(app_handle, &text, skill_match, next_match, &mut config) {
                    Ok(_) => {
                        println!(
                            "[SKILL] #{} Executed config skill successfully: {}",
                            seq_id, skill_match.skill_id
                        );
                    }
                    Err(e) => {
                        emit_voice_command_feedback(
                            app_handle,
                            "error",
                            format!("配置更新失败：{}", e),
                        );
                        eprintln!(
                            "[SKILL] #{} Config skill execution failed for {}: {}",
                            seq_id, skill_match.skill_id, e
                        );
                    }
                }
                continue;
            }

            match skills::execute_skill(&skill_match.skill_id) {
                Ok(_) => {
                    println!(
                        "[SKILL] #{} Executed successfully: {}",
                        seq_id, skill_match.skill_id
                    );
                }
                Err(e) => {
                    eprintln!(
                        "[SKILL] #{} Execution failed for {}: {}",
                        seq_id, skill_match.skill_id, e
                    );
                }
            }
        }
    } else {
        println!("[SKILL] #{} No skill matched for text: '{}'", seq_id, preview_text(&text, 40));
    }

    // Clear processing flag
    processing.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// 将识别结果输出到当前焦点窗口
/// 使用剪贴板 + Ctrl+V 粘贴，确保兼容 Windows 原生控件（资源管理器地址栏、搜索框等）
fn output_text(text: &str, seq_id: u64) {
    println!("[OUTPUT] #{} start: {} chars", seq_id, text.len());

    // 等待目标窗口完成鼠标/键盘事件处理
    // 重要：这个延迟是必要的，原因如下：
    // 1. 鼠标中键触发时，目标窗口需要时间处理中键释放事件
    // 2. Windows 原生控件（如资源管理器地址栏）对输入事件的处理有延迟
    // 3. 某些应用（如 Word、浏览器）需要时间完成焦点切换
    // 
    // 80ms 是经验值，在大多数情况下足够，同时不会让用户感到明显延迟
    const INPUT_SETTLE_DELAY_MS: u64 = 80;
    std::thread::sleep(std::time::Duration::from_millis(INPUT_SETTLE_DELAY_MS));

    // 使用剪贴板粘贴方式，兼容性更好
    // 1. 保存原剪贴板内容（可能失败，忽略）
    // 2. 写入新文本
    // 3. 模拟 Ctrl+V
    // 4. 延迟后恢复原内容

    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[OUTPUT] #{} clipboard init failed: {:?}", seq_id, e);
            return;
        }
    };

    // 保存原剪贴板文本（可能为空或非文本，忽略错误）
    let original_text = clipboard.get_text().ok();

    // 写入要粘贴的文本
    if let Err(e) = clipboard.set_text(text) {
        eprintln!("[OUTPUT] #{} clipboard set_text failed: {:?}", seq_id, e);
        return;
    }

    // 短暂延迟确保剪贴板更新
    std::thread::sleep(std::time::Duration::from_millis(10));

    // 模拟 Ctrl+V 粘贴
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[OUTPUT] #{} enigo init failed: {:?}", seq_id, e);
            return;
        }
    };

    // Ctrl 按下 -> V 按下 -> V 释放 -> Ctrl 释放
    if let Err(e) = enigo.key(Key::Control, enigo::Direction::Press) {
        eprintln!("[OUTPUT] #{} Ctrl press failed: {:?}", seq_id, e);
    }
    std::thread::sleep(std::time::Duration::from_millis(5));
    if let Err(e) = enigo.key(Key::Unicode('v'), enigo::Direction::Click) {
        eprintln!("[OUTPUT] #{} V click failed: {:?}", seq_id, e);
    }
    std::thread::sleep(std::time::Duration::from_millis(5));
    if let Err(e) = enigo.key(Key::Control, enigo::Direction::Release) {
        eprintln!("[OUTPUT] #{} Ctrl release failed: {:?}", seq_id, e);
    }

    println!("[OUTPUT] #{} paste done", seq_id);

    // 延迟后恢复原剪贴板内容（避免覆盖用户剪贴板）
    // 使用较长延迟确保粘贴完成
    std::thread::sleep(std::time::Duration::from_millis(100));
    if let Some(original) = original_text {
        // 恢复原内容，忽略错误
        let _ = clipboard.set_text(&original);
        println!("[OUTPUT] #{} clipboard restored", seq_id);
    }
}

#[derive(Serialize)]
pub struct AsrStatus {
    configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct VoiceCommandFeedback {
    level: String,
    message: String,
}

#[derive(Clone, Debug)]
enum ConfigSkillPlan {
    Save {
        config: AppConfig,
        feedback: VoiceCommandFeedback,
    },
    Feedback(VoiceCommandFeedback),
}

fn emit_voice_command_feedback<R: Runtime>(
    app_handle: &AppHandle<R>,
    level: &str,
    message: impl Into<String>,
) {
    app_handle
        .emit(
            "voice_command_feedback",
            VoiceCommandFeedback {
                level: level.to_string(),
                message: message.into(),
            },
        )
        .ok();
}

fn save_and_emit_config_update<R: Runtime>(
    app_handle: &AppHandle<R>,
    config: &AppConfig,
) -> Result<(), String> {
    let storage = app_handle.state::<StorageState>();
    storage.save_config(config).map_err(|e| e.to_string())?;
    app_handle.emit("config_updated", config.clone()).ok();
    Ok(())
}

fn plan_config_skill_update(
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
    config: &AppConfig,
) -> Result<ConfigSkillPlan, String> {
    match skill_match.skill_id.as_str() {
        skills::ENABLE_POLISH_SKILL_ID => {
            if config.llm_config.enabled {
                return Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                    level: "info".to_string(),
                    message: "润色已经处于启用状态".to_string(),
                }));
            }

            let mut next_config = config.clone();
            next_config.llm_config.enabled = true;
            Ok(ConfigSkillPlan::Save {
                config: next_config,
                feedback: VoiceCommandFeedback {
                    level: "success".to_string(),
                    message: "已启用润色".to_string(),
                },
            })
        }
        skills::DISABLE_POLISH_SKILL_ID => {
            if !config.llm_config.enabled {
                return Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                    level: "info".to_string(),
                    message: "润色已经处于关闭状态".to_string(),
                }));
            }

            let mut next_config = config.clone();
            next_config.llm_config.enabled = false;
            Ok(ConfigSkillPlan::Save {
                config: next_config,
                feedback: VoiceCommandFeedback {
                    level: "success".to_string(),
                    message: "已关闭润色".to_string(),
                },
            })
        }
        skills::SWITCH_POLISH_SCENE_SKILL_ID => {
            let scene_query = skills::extract_scene_query(transcript, skill_match, next_match);
            if scene_query.is_empty() {
                return Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                    level: "error".to_string(),
                    message: "未识别到要切换的润色场景".to_string(),
                }));
            }

            match skills::resolve_scene(&config.llm_config.profiles, &scene_query) {
                skills::SceneResolveResult::Unique {
                    profile_id,
                    profile_name,
                } => {
                    if config.llm_config.active_profile_id == profile_id {
                        return Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                            level: "info".to_string(),
                            message: format!("当前已经是场景“{}”", profile_name),
                        }));
                    }

                    let mut next_config = config.clone();
                    next_config.llm_config.active_profile_id = profile_id;
                    Ok(ConfigSkillPlan::Save {
                        config: next_config,
                        feedback: VoiceCommandFeedback {
                            level: "success".to_string(),
                            message: format!("已切换到场景“{}”", profile_name),
                        },
                    })
                }
                skills::SceneResolveResult::None => Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                    level: "error".to_string(),
                    message: format!("未找到匹配场景：{}", scene_query),
                })),
                skills::SceneResolveResult::Ambiguous(names) => {
                    Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                        level: "error".to_string(),
                        message: format!("匹配到多个场景：{}", names.join("、")),
                    }))
                }
            }
        }
        _ => Err(format!("Unsupported config skill: {}", skill_match.skill_id)),
    }
}

fn execute_config_skill<R: Runtime>(
    app_handle: &AppHandle<R>,
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
    config: &mut AppConfig,
) -> Result<(), String> {
    match plan_config_skill_update(transcript, skill_match, next_match, config)? {
        ConfigSkillPlan::Save {
            config: next_config,
            feedback,
        } => {
            save_and_emit_config_update(app_handle, &next_config)?;
            *config = next_config;
            let VoiceCommandFeedback { level, message } = feedback;
            emit_voice_command_feedback(app_handle, &level, message);
            Ok(())
        }
        ConfigSkillPlan::Feedback(feedback) => {
            let VoiceCommandFeedback { level, message } = feedback;
            emit_voice_command_feedback(app_handle, &level, message);
            Ok(())
        }
    }
}



fn begin_recording_session<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
) -> bool {
    let started_at = Instant::now();
    let audio = app_handle.state::<AudioState>();
    let (sample_rate, stream_rx) = match audio.lock() {
        Ok(audio) => match audio.start_recording_with_streaming() {
            Ok(rx) => (audio.get_sample_rate(), rx),
            Err(err) => {
                eprintln!("[START] Failed to start audio capture: {}", err);
                return false;
            }
        },
        Err(_) => return false,
    };

    app_handle.emit("recording_status", true).ok();
    let listener = app_handle.state::<InputListenerState>();
    listener
        .track_mouse_position
        .store(true, std::sync::atomic::Ordering::Relaxed);
    show_indicator_window(app_handle, false);

    let storage = app_handle.state::<StorageState>();
    let config = storage.load_config();
    let asr = app_handle.state::<AsrState>();
    let handle = app_handle.clone();
    match asr.start_streaming_session(
        stream_rx,
        sample_rate,
        config.online_asr_config,
        config.proxy,
        move |text| {
            handle.emit("stream_update", text).ok();
        },
    ) {
        Ok(session) => {
            *streaming_session = Some(session);
            println!(
                "[START] Recording session ready in {} ms",
                started_at.elapsed().as_millis()
            );
            true
        }
        Err(err) => {
            eprintln!("[START] Failed to start streaming preview: {}", err);
            app_handle.emit("recording_status", false).ok();
            listener
                .track_mouse_position
                .store(false, std::sync::atomic::Ordering::Relaxed);
            emit_session_complete(app_handle);
            if let Ok(audio) = app_handle.state::<AudioState>().lock() {
                let _ = audio.stop_recording();
            }
            false
        }
    }
}

fn finish_recording_session<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> (Vec<f32>, u32) {
    app_handle.emit("recording_status", false).ok();
    let listener = app_handle.state::<InputListenerState>();
    listener
        .track_mouse_position
        .store(false, std::sync::atomic::Ordering::Relaxed);
    // DO NOT hide the window here! We want it to stay visible for processing states.

    let audio = app_handle.state::<AudioState>();
    let mut buffer = Vec::new();
    let mut sample_rate = 48_000u32;
    if let Ok(audio) = audio.lock() {
        sample_rate = audio.get_sample_rate();
        if let Ok(b) = audio.stop_recording() {
            buffer = b;
        }
    }

    // Session finished later

    (buffer, sample_rate)
}

fn stop_and_transcribe<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    log_tag: &str,
    skill_mode: bool,
) {
    let stop_started = Instant::now();
    let (_buffer, _sample_rate) = finish_recording_session(app_handle);
    println!(
        "[{}] Capture stopped in {} ms",
        log_tag,
        stop_started.elapsed().as_millis()
    );

    app_handle.emit("recognition_processing", true).ok();
    let transcribe_started = Instant::now();
    let text_result = if let Some(session) = streaming_session.take() {
        session.finish_and_wait()
    } else {
        Err(anyhow::anyhow!("No active streaming session to finish"))
    };
    
    match text_result {
        Ok(text) => {
            app_handle.emit("stream_update", text.clone()).ok();
            app_handle.emit("recognition_processing", false).ok();
            let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
            println!(
                "[{}] #{} Online transcription completed in {} ms, {} chars, preview='{}'",
                log_tag,
                seq_id,
                transcribe_started.elapsed().as_millis(),
                text.len(),
                preview_text(&text, 80)
            );

            if skill_mode {
                process_transcription_for_skill(
                    app_handle,
                    text,
                    processing,
                    seq_id,
                );
                emit_session_complete(app_handle);
            } else {
                process_transcription(
                    app_handle,
                    text,
                    processing,
                    llm_cancel,
                    seq_id,
                );
            }
        }
        Err(e) => {
            app_handle.emit("recognition_processing", false).ok();
            eprintln!("[{}] Transcription error: {}", log_tag, e);
            processing.store(false, std::sync::atomic::Ordering::SeqCst);
            emit_session_complete(app_handle);
        }
    }
}

fn cancel_pending_llm(llm_cancel: &LlmCancelState, log_tag: &str) {
    if let Ok(guard) = llm_cancel.lock() {
        if let Some(token) = guard.as_ref() {
            println!("[{}] Cancelling ongoing LLM request", log_tag);
            token.cancel();
        }
    }
}

#[tauri::command]
fn get_config(state: tauri::State<StorageState>) -> AppConfig {
    state.load_config()
}

#[tauri::command]
fn take_runtime_notice(state: tauri::State<StorageState>) -> Option<String> {
    state.take_runtime_notice()
}

#[tauri::command]
fn save_config(
    state: tauri::State<StorageState>, 
    listener: tauri::State<InputListenerState>,
    config: AppConfig
) -> Result<(), String> {
    // Update listener flags immediately (hot-reload)
    listener.enable_mouse.store(config.trigger_mouse, std::sync::atomic::Ordering::Relaxed);
    listener.enable_alt.store(config.trigger_toggle, std::sync::atomic::Ordering::Relaxed);
    
    state.save_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_history(state: tauri::State<StorageState>) -> Vec<HistoryItem> {
    state.load_history()
}

#[tauri::command]
fn clear_history(state: tauri::State<StorageState>) -> Result<(), String> {
    state.clear_history().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_history_item(id: String, state: tauri::State<StorageState>) -> Result<(), String> {
    state.delete_history_item(id).map_err(|e| e.to_string())
}



#[tauri::command]
async fn get_asr_status(state: tauri::State<'_, StorageState>) -> Result<AsrStatus, String> {
    let config = state.load_config();
    let configured =
        !config.online_asr_config.app_key.is_empty() && !config.online_asr_config.access_key.is_empty();
    Ok(AsrStatus { configured })
}

#[tauri::command]
fn get_input_devices() -> Vec<audio::AudioDevice> {
    audio::AudioService::get_input_devices()
}

#[tauri::command]
fn get_current_input_device(audio: tauri::State<AudioState>) -> String {
    if let Ok(audio) = audio.lock() {
        audio.get_current_device_name()
    } else {
        String::new()
    }
}

#[tauri::command]
fn switch_input_device<R: Runtime>(
    app: AppHandle<R>,
    audio: tauri::State<AudioState>,
    storage: tauri::State<StorageState>,
    device_id: String
) -> Result<(), String> {
    // Update audio service
    if let Ok(mut audio) = audio.lock() {
        audio.init_with_device(&device_id, app.clone()).map_err(|e| e.to_string())?;
    } else {
        return Err("Failed to lock audio service".to_string());
    }
    
    // Save to config
    let mut config = storage.load_config();
    config.input_device = device_id;
    storage.save_config(&config).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
fn start_audio_test(audio: tauri::State<AudioState>) -> Result<(), String> {
    if let Ok(audio) = audio.lock() {
        audio.start_test().map_err(|e| e.to_string())
    } else {
        Err("Failed to lock audio service".to_string())
    }
}

#[tauri::command]
fn stop_audio_test(audio: tauri::State<AudioState>) -> Result<(), String> {
    if let Ok(audio) = audio.lock() {
        audio.stop_test().map_err(|e| e.to_string())
    } else {
        Err("Failed to lock audio service".to_string())
    }
}

#[tauri::command]
async fn test_llm_connection(config: LlmConfig, proxy: ProxyConfig) -> Result<String, String> {
    llm::test_connection(&config, &proxy).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn get_default_scene_template() -> PromptProfile {
    storage::blank_scene_template()
}

#[tauri::command]
fn get_default_scene_profiles() -> Vec<PromptProfile> {
    storage::default_scene_profiles()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Create indicator window
            println!("Creating indicator window...");
            let indicator_url = WebviewUrl::App("indicator.html".into());
            println!("Indicator URL: {:?}", indicator_url);

            match WebviewWindowBuilder::new(app, "indicator", indicator_url)
                .title("")
                .inner_size(800.0, 100.0) // 容纳动态伸缩的胶囊形状和文字
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .visible(false)
                .shadow(false)
                .focused(false)
                .build()
            {
                Ok(window) => {
                    println!("Indicator window created successfully: {:?}", window.label());
                },
                Err(e) => eprintln!("Failed to create indicator window: {:?}", e),
            }

            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .expect("default window icon is required for tray");

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(&tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => std::process::exit(0),
                    _ => {}
                })
                .build(app)?;

            // Initialize Storage (config in AppData\Roaming)
            let app_dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
            let storage_service = storage::StorageService::new(app_dir.clone());
            let config = storage_service.load_config();

            // Initialize Services
            let asr_service = asr::AsrService::new();
            let mut audio_service = audio::AudioService::new();

            // Try to initialize with configured device, fallback to default if it fails
            let device_init_result = audio_service.init_with_device(&config.input_device, app_handle.clone());

            if let Err(e) = device_init_result {
                eprintln!("Failed to init audio with configured device '{}': {}", config.input_device, e);
                eprintln!("Attempting to fallback to default audio device...");

                // Try to initialize with empty device name (default device)
                match audio_service.init_with_device("", app_handle.clone()) {
                    Ok(_) => {
                        println!("Successfully initialized with default audio device");
                        println!("Please select your preferred device in Settings");
                        // Do NOT update config - keep the original device name so user can see what was selected before
                    },
                    Err(fallback_err) => {
                        eprintln!("Failed to init audio with default device: {}", fallback_err);
                        eprintln!("Application will continue but audio recording will not work until a device is selected in settings.");
                    }
                }
            }

            let audio_state = Mutex::new(audio_service);

            let input_listener = input_listener::InputListener::new();
            // Update listener flags based on config
            input_listener.enable_mouse.store(config.trigger_mouse, std::sync::atomic::Ordering::Relaxed);
            input_listener.enable_alt.store(config.trigger_toggle, std::sync::atomic::Ordering::Relaxed);

            // Channel for Input Events
            let (tx, rx) = std::sync::mpsc::channel();
            input_listener.start(tx);

            // Shared processing flag:
            // We must NOT allow a new transcription/paste to start while the previous async
            // pipeline (LLM + enigo typing) is still running; otherwise keystrokes interleave
            // and output becomes garbled/duplicated.
            let processing_state: ProcessingState = Arc::new(std::sync::atomic::AtomicBool::new(false));

            // LLM cancellation state - allows cancelling ongoing LLM requests
            let llm_cancel_state: LlmCancelState = Arc::new(Mutex::new(None));

            // Background Thread to handle events
            let processing_for_thread = processing_state.clone();
            let llm_cancel_for_thread = llm_cancel_state.clone();
            #[allow(unreachable_code)]
            std::thread::spawn(move || {
                let mut recording_mode: Option<RecordingMode> = None;
                let mut streaming_session: Option<asr::StreamingSession> = None;

                for event in rx {
                    match event {
                        input_listener::InputEvent::Toggle => {
                            if recording_mode == Some(RecordingMode::Dictation)
                                && !processing_for_thread.load(std::sync::atomic::Ordering::SeqCst)
                            {
                                recording_mode = None;

                                if processing_for_thread
                                    .compare_exchange(
                                        false,
                                        true,
                                        std::sync::atomic::Ordering::SeqCst,
                                        std::sync::atomic::Ordering::SeqCst,
                                    )
                                    .is_err()
                                {
                                    continue;
                                }
                                stop_and_transcribe(
                                    &app_handle,
                                    &mut streaming_session,
                                    processing_for_thread.clone(),
                                    llm_cancel_for_thread.clone(),
                                    "TOGGLE",
                                    false,
                                );
                            } else if recording_mode.is_none() {
                                if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                    cancel_pending_llm(&llm_cancel_for_thread, "TOGGLE");
                                    std::thread::sleep(std::time::Duration::from_millis(50));
                                }

                                if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                    continue;
                                }

                                if begin_recording_session(&app_handle, &mut streaming_session) {
                                    recording_mode = Some(RecordingMode::Dictation);
                                }
                            }
                        },
                        input_listener::InputEvent::MouseMove => {
                            // Mouse movement detected - indicator stays at bottom-center
                        },
                        input_listener::InputEvent::StartSkill => {
                            if recording_mode.is_some() {
                                continue;
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                cancel_pending_llm(&llm_cancel_for_thread, "SKILL");
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                continue;
                            }

                            if begin_recording_session(&app_handle, &mut streaming_session) {
                                recording_mode = Some(RecordingMode::Skill);
                            }
                        },
                        input_listener::InputEvent::StopSkill => {
                            if recording_mode != Some(RecordingMode::Skill)
                                || processing_for_thread.load(std::sync::atomic::Ordering::SeqCst)
                            {
                                continue;
                            }

                            recording_mode = None;

                            if processing_for_thread
                                .compare_exchange(
                                    false,
                                    true,
                                    std::sync::atomic::Ordering::SeqCst,
                                    std::sync::atomic::Ordering::SeqCst,
                                )
                                .is_err()
                            {
                                continue;
                            }

                            stop_and_transcribe(
                                &app_handle,
                                &mut streaming_session,
                                processing_for_thread.clone(),
                                llm_cancel_for_thread.clone(),
                                "SKILL",
                                true,
                            );
                        }
                    }
                }
            });

            // manage states
            app.manage(audio_state);
            app.manage(asr_service);
            app.manage(storage_service);
            app.manage(input_listener); // expose to commands if needed (to update config)
            app.manage(processing_state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config, take_runtime_notice, save_config, get_history, clear_history, delete_history_item,
            get_asr_status,
            get_input_devices, get_current_input_device, switch_input_device,
            start_audio_test, stop_audio_test,
            test_llm_connection, get_default_scene_template, get_default_scene_profiles
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    hide_main_window(&window.app_handle());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{plan_config_skill_update, AppConfig, ConfigSkillPlan, VoiceCommandFeedback};
    use crate::skills::{
        SkillMatch, DISABLE_POLISH_SKILL_ID, ENABLE_POLISH_SKILL_ID, SWITCH_POLISH_SCENE_SKILL_ID,
    };
    use crate::storage::PromptProfile;

    fn expect_saved_config(plan: ConfigSkillPlan) -> (AppConfig, VoiceCommandFeedback) {
        match plan {
            ConfigSkillPlan::Save { config, feedback } => (config, feedback),
            ConfigSkillPlan::Feedback(feedback) => {
                panic!("expected saved config, got feedback: {:?}", feedback)
            }
        }
    }

    fn profile(id: &str, name: &str, voice_aliases: &[&str]) -> PromptProfile {
        PromptProfile {
            id: id.to_string(),
            name: name.to_string(),
            voice_aliases: voice_aliases.iter().map(|alias| alias.to_string()).collect(),
            ..PromptProfile::new_default()
        }
    }

    #[test]
    fn enable_polish_plans_enabled_config() {
        let config = AppConfig::default();
        let plan = plan_config_skill_update(
            "启用润色",
            &SkillMatch {
                skill_id: ENABLE_POLISH_SKILL_ID.to_string(),
                keyword: "启用润色".to_string(),
                start: 0,
                end: "启用润色".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        let (next_config, feedback) = expect_saved_config(plan);
        assert!(next_config.llm_config.enabled);
        assert_eq!(feedback.message, "已启用润色");
    }

    #[test]
    fn disable_polish_plans_disabled_config() {
        let mut config = AppConfig::default();
        config.llm_config.enabled = true;

        let plan = plan_config_skill_update(
            "关闭润色",
            &SkillMatch {
                skill_id: DISABLE_POLISH_SKILL_ID.to_string(),
                keyword: "关闭润色".to_string(),
                start: 0,
                end: "关闭润色".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        let (next_config, feedback) = expect_saved_config(plan);
        assert!(!next_config.llm_config.enabled);
        assert_eq!(feedback.message, "已关闭润色");
    }

    #[test]
    fn switch_scene_matches_voice_alias() {
        let mut config = AppConfig::default();
        config.llm_config.profiles = vec![
            profile("default", "默认", &[]),
            profile("email", "邮件写作", &["邮件"]),
        ];
        config.llm_config.active_profile_id = "default".to_string();

        let transcript = "切换到邮件";
        let plan = plan_config_skill_update(
            transcript,
            &SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: 0,
                end: "切换到".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        let (next_config, feedback) = expect_saved_config(plan);
        assert_eq!(next_config.llm_config.active_profile_id, "email");
        assert_eq!(feedback.message, "已切换到场景“邮件写作”");
    }

    #[test]
    fn switch_scene_falls_back_to_profile_name() {
        let mut config = AppConfig::default();
        config.llm_config.profiles = vec![profile("meeting", "会议纪要", &[])];
        config.llm_config.active_profile_id = "meeting".to_string();

        let plan = plan_config_skill_update(
            "切换到会议纪要模式",
            &SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: 0,
                end: "切换到".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        match plan {
            ConfigSkillPlan::Feedback(feedback) => {
                assert_eq!(feedback.message, "当前已经是场景“会议纪要”");
            }
            ConfigSkillPlan::Save { .. } => panic!("expected no-op feedback when already active"),
        }
    }

    #[test]
    fn switch_scene_reports_alias_conflicts() {
        let mut config = AppConfig::default();
        config.llm_config.profiles = vec![
            profile("email", "邮件", &["客服"]),
            profile("support", "客服回复", &["客服"]),
        ];

        let plan = plan_config_skill_update(
            "切换到客服",
            &SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: 0,
                end: "切换到".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        match plan {
            ConfigSkillPlan::Feedback(feedback) => {
                assert_eq!(feedback.level, "error");
                assert!(feedback.message.contains("匹配到多个场景"));
            }
            ConfigSkillPlan::Save { .. } => panic!("expected ambiguity feedback"),
        }
    }

    #[test]
    fn combined_enable_and_switch_commands_apply_in_order() {
        let mut config = AppConfig::default();
        config.llm_config.enabled = false;
        config.llm_config.profiles = vec![
            profile("default", "默认", &[]),
            profile("email", "邮件", &["邮件"]),
        ];
        config.llm_config.active_profile_id = "default".to_string();

        let transcript = "启用润色切换到邮件";
        let enable_match = SkillMatch {
            skill_id: ENABLE_POLISH_SKILL_ID.to_string(),
            keyword: "启用润色".to_string(),
            start: 0,
            end: "启用润色".len(),
        };
        let switch_match = SkillMatch {
            skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
            keyword: "切换到".to_string(),
            start: "启用润色".len(),
            end: "启用润色切换到".len(),
        };

        let (after_enable, _) = expect_saved_config(
            plan_config_skill_update(transcript, &enable_match, Some(&switch_match), &config)
                .expect("enable plan should succeed"),
        );
        let (after_switch, _) = expect_saved_config(
            plan_config_skill_update(transcript, &switch_match, None, &after_enable)
                .expect("switch plan should succeed"),
        );

        assert!(after_switch.llm_config.enabled);
        assert_eq!(after_switch.llm_config.active_profile_id, "email");
    }
}

