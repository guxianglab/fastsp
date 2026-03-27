mod asr;
mod audio;
mod http_client;
mod input_listener;
mod llm;
mod skills;
mod storage;
// TODO: 流式模块暂时禁用，等待完整集成
// mod streaming_asr;

use serde::Serialize;
use std::collections::HashSet;
use std::sync::Mutex;
use storage::{AppConfig, HistoryItem, LlmConfig, PromptProfile, ProxyConfig};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tokio_util::sync::CancellationToken;

// Define State Types
type AudioState = Mutex<audio::AudioService>;
type AsrState = asr::AsrService;
type StorageState = storage::StorageService;
type InputListenerState = input_listener::InputListener;
type ProcessingState = Arc<std::sync::atomic::AtomicBool>; // 防止重复处理（跨线程/异步任务共享）
type LlmCancelState = Arc<Mutex<Option<CancellationToken>>>; // LLM 请求取消令牌
type SkillExecutionState = Arc<Mutex<Option<SkillExecutionSession>>>;
// type StreamingAsrState = streaming_asr::StreamingAsrService;

use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use enigo::{Enigo, Key, Keyboard, Settings};

// Monotonic id to correlate a single transcription pipeline across logs.
static TRANSCRIPTION_SEQ: AtomicU64 = AtomicU64::new(1);
static DICTATION_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);
static SKILL_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DictationIntent {
    Raw,
    Polish,
    Skill,
    None,
}

impl DictationIntent {
    fn as_event(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Polish => "polish",
            Self::Skill => "skill",
            Self::None => "none",
        }
    }
}

#[derive(Debug)]
struct PendingFinalizeState {
    session_id: u64,
    intent: DictationIntent,
    window_elapsed: bool,
    asr_result: Option<Result<String, String>>,
}

#[derive(Debug)]
enum DictationState {
    Idle,
    Recording {
        intent: DictationIntent,
        started_at: Instant,
    },
    PendingFinalize(PendingFinalizeState),
    SkillRecording,
}

#[derive(Debug, Default)]
struct SkillExecutionSession {
    id: u64,
    executed: HashSet<String>,
    pending: HashSet<String>,
    consumed_prefix: String,
    last_streaming_browser_open_action: Option<String>,
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

const DOUBLE_CLICK_WINDOW_MS: u64 = 280;

fn emit_dictation_intent<R: Runtime>(app_handle: &AppHandle<R>, intent: DictationIntent) {
    app_handle.emit("dictation_intent", intent.as_event()).ok();
}

/// Show the indicator window.
fn show_indicator_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("indicator") {
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
                    let window_y =
                        physical_bottom_y - ((logical_height + bottom_margin) * scale_factor);

                    let window_pos = tauri::PhysicalPosition::new(window_x as i32, window_y as i32);
                    window.set_position(window_pos).ok();
                    break;
                }
            }
        }

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
    polish_requested: bool,
) {
    if text.trim().is_empty() {
        println!("[TRANSCRIPTION] #{} empty, skipping", seq_id);
        processing.store(false, std::sync::atomic::Ordering::SeqCst);
        emit_dictation_intent(app_handle, DictationIntent::None);
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
    let effective_polish = llm_config.enabled || polish_requested;

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

        let final_text = if effective_polish {
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
                listener
                    .track_mouse_position
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            show_indicator_window(&app_handle_clone);

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
                listener
                    .track_mouse_position
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
            emit_dictation_intent(&app_handle_clone, DictationIntent::None);
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
                    emit_dictation_intent(&app_handle_clone, DictationIntent::None);
                    return;
                }
            }
        } else {
            emit_dictation_intent(&app_handle_clone, DictationIntent::None);
            emit_session_complete(&app_handle_clone);
            text
        };

        if final_text.trim().is_empty() {
            println!("[TRANSCRIPTION] #{} final empty, skipping", seq_id);
            emit_dictation_intent(&app_handle_clone, DictationIntent::None);
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
        })
        .join()
        .ok();
    });
}

#[allow(unreachable_code)]
async fn execute_skill_transcript<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: &str,
    llm_cancel: &LlmCancelState,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    seq_id: u64,
    is_final: bool,
) {
    if text.trim().is_empty() {
        return;
    }

    execute_skill_transcript_streaming(
        app_handle,
        text,
        llm_cancel,
        skill_state,
        skill_session_id,
        seq_id,
        is_final,
    )
    .await;
    return;

    let Some((effective_text, transcript_offset)) =
        prepare_skill_transcript(skill_state, skill_session_id, text)
    else {
        return;
    };

    let storage = app_handle.state::<StorageState>();
    let mut config = storage.load_config();
    let skills_config = config.skills.clone();
    let browser_skill =
        skills::find_skill_config(&skills_config, skills::OPEN_BROWSER_SKILL_ID).cloned();
    let windows_skill =
        skills::find_skill_config(&skills_config, skills::OPEN_WINDOWS_SKILL_ID).cloned();

    let matched_skills = skills::match_skills(&effective_text, &skills_config);
    let mut max_consumed_local_end = 0usize;
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
            let local_consumed_end = next_match
                .map(|next_skill_match| next_skill_match.start)
                .unwrap_or(effective_text.len());

            if skills::is_config_skill(&skill_match.skill_id) {
                let action_key = format!("config:{}", skill_match.skill_id);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    continue;
                }

                match execute_config_skill(
                    app_handle,
                    &effective_text,
                    skill_match,
                    next_match,
                    &mut config,
                ) {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        max_consumed_local_end = max_consumed_local_end.max(local_consumed_end);
                        println!(
                            "[SKILL] #{} Executed config skill successfully: {}",
                            seq_id, skill_match.skill_id
                        );
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
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

            if skill_match.skill_id == skills::OPEN_WINDOWS_SKILL_ID {
                match windows_skill.as_ref() {
                    Some(windows_skill) => match plan_windows_command(
                        &effective_text,
                        windows_skill,
                        Some(skill_match),
                    ) {
                        Ok(Some(plan)) => {
                            if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                                if !is_windows_open_query_ready(windows_skill, query) {
                                    continue;
                                }
                                let action_key = windows_action_key(&plan.action);
                                if !is_final
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    continue;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            let action_key = windows_action_key(&plan.action);
                            if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                                continue;
                            }

                            match execute_windows_plan(
                                app_handle,
                                windows_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                Ok(_) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    max_consumed_local_end =
                                        max_consumed_local_end.max(plan.consumed_end);
                                    clear_streaming_browser_open_action_candidate(
                                        skill_state,
                                        skill_session_id,
                                    );
                                    println!(
                                        "[SKILL] #{} Executed Windows command successfully",
                                        seq_id
                                    );
                                }
                                Err(e) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    emit_voice_command_feedback(app_handle, "error", e.clone());
                                    eprintln!(
                                        "[SKILL] #{} Windows execution failed: {}",
                                        seq_id, e
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                        }
                        Err(e) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                            emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Windows plan failed: {}", seq_id, e);
                        }
                    },
                    None => {
                        emit_voice_command_feedback(app_handle, "error", "Windows skill missing");
                        eprintln!("[SKILL] #{} Windows skill missing from config", seq_id);
                    }
                }
                continue;
            }

            if skill_match.skill_id == skills::OPEN_BROWSER_SKILL_ID {
                match browser_skill.as_ref() {
                    Some(browser_skill) => match plan_browser_command(
                        &effective_text,
                        browser_skill,
                        Some(skill_match),
                    ) {
                        Ok(Some(plan)) => {
                            if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                                if !is_browser_open_query_ready(browser_skill, query) {
                                    continue;
                                }
                                let action_key = browser_action_key(&plan.action);
                                if !is_final
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    continue;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            let action_key = browser_action_key(&plan.action);
                            if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                                continue;
                            }

                            match execute_browser_plan(
                                app_handle,
                                browser_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                Ok(_) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    max_consumed_local_end =
                                        max_consumed_local_end.max(plan.consumed_end);
                                    clear_streaming_browser_open_action_candidate(
                                        skill_state,
                                        skill_session_id,
                                    );
                                    println!(
                                        "[SKILL] #{} Executed browser command successfully",
                                        seq_id
                                    );
                                }
                                Err(e) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    emit_voice_command_feedback(app_handle, "error", e.clone());
                                    eprintln!(
                                        "[SKILL] #{} Browser execution failed: {}",
                                        seq_id, e
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                        }
                        Err(e) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                            emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Browser plan failed: {}", seq_id, e);
                        }
                    },
                    None => {
                        emit_voice_command_feedback(app_handle, "error", "浏览器技能未配置");
                        eprintln!("[SKILL] #{} Browser skill missing from config", seq_id);
                    }
                }
                continue;
            }

            let action_key = format!("skill:{}", skill_match.skill_id);
            if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                continue;
            }

            match skills::execute_skill(&skill_match.skill_id) {
                Ok(_) => {
                    complete_skill_action(skill_state, skill_session_id, &action_key, true);
                    max_consumed_local_end = max_consumed_local_end.max(local_consumed_end);
                    println!(
                        "[SKILL] #{} Executed successfully: {}",
                        seq_id, skill_match.skill_id
                    );
                }
                Err(e) => {
                    complete_skill_action(skill_state, skill_session_id, &action_key, true);
                    emit_voice_command_feedback(app_handle, "error", e.clone());
                    eprintln!(
                        "[SKILL] #{} Execution failed for {}: {}",
                        seq_id, skill_match.skill_id, e
                    );
                }
            }
        }

        if max_consumed_local_end > 0 {
            advance_skill_transcript_consumed(
                skill_state,
                skill_session_id,
                text,
                transcript_offset + max_consumed_local_end,
            );
        }
        return;
    }

    if let Some(windows_skill) = windows_skill.as_ref() {
        match plan_windows_command(&effective_text, windows_skill, None) {
            Ok(Some(plan)) => {
                if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                    if !is_windows_open_query_ready(windows_skill, query) {
                        return;
                    }
                    let action_key = windows_action_key(&plan.action);
                    if !is_final
                        && !confirm_streaming_browser_open_action(
                            skill_state,
                            skill_session_id,
                            &action_key,
                        )
                    {
                        println!(
                            "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                            seq_id,
                            query.trim()
                        );
                        return;
                    }
                } else {
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                }

                let action_key = windows_action_key(&plan.action);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    return;
                }

                let is_open_target =
                    matches!(&plan.action, skills::WindowsAction::OpenTarget { .. });
                match execute_windows_plan(
                    app_handle,
                    windows_skill,
                    &plan,
                    &config,
                    llm_cancel,
                    seq_id,
                )
                .await
                {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end,
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed Windows fallback successfully", seq_id);
                        return;
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        if is_open_target {
                            eprintln!(
                                "[SKILL] #{} Windows fallback unresolved, trying browser fallback: {}",
                                seq_id, e
                            );
                        } else {
                            emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Windows fallback failed: {}", seq_id, e);
                            return;
                        }
                    }
                }
            }
            Ok(None) => {
                clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
            }
            Err(_) => {
                clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
            }
        }
    }

    if let Some(browser_skill) = browser_skill.as_ref() {
        match plan_browser_command(&effective_text, browser_skill, None) {
            Ok(Some(plan)) => {
                if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                    if !is_browser_open_query_ready(browser_skill, query) {
                        return;
                    }
                    let action_key = browser_action_key(&plan.action);
                    if !is_final
                        && !confirm_streaming_browser_open_action(
                            skill_state,
                            skill_session_id,
                            &action_key,
                        )
                    {
                        println!(
                            "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                            seq_id,
                            query.trim()
                        );
                        return;
                    }
                } else {
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                }

                let action_key = browser_action_key(&plan.action);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    return;
                }

                match execute_browser_plan(
                    app_handle,
                    browser_skill,
                    &plan,
                    &config,
                    llm_cancel,
                    seq_id,
                )
                .await
                {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end,
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed browser fallback successfully", seq_id);
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        emit_voice_command_feedback(app_handle, "error", e.clone());
                        eprintln!("[SKILL] #{} Browser fallback failed: {}", seq_id, e);
                    }
                }
            }
            Ok(None) => {
                clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                println!(
                    "[SKILL] #{} No skill matched for text: '{}'",
                    seq_id,
                    preview_text(&effective_text, 40)
                );
            }
            Err(_) => {
                clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
            }
        }
    }
}

async fn execute_skill_transcript_streaming<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: &str,
    llm_cancel: &LlmCancelState,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    seq_id: u64,
    is_final: bool,
) {
    let storage = app_handle.state::<StorageState>();
    let mut config = storage.load_config();

    loop {
        let Some((effective_text, transcript_offset)) =
            prepare_skill_transcript(skill_state, skill_session_id, text)
        else {
            return;
        };

        let (clause_text, clause_consumed_end) = split_skill_clause(&effective_text);
        let clause_text = clause_text.trim();
        if clause_text.is_empty() {
            advance_skill_transcript_consumed(
                skill_state,
                skill_session_id,
                text,
                transcript_offset + clause_consumed_end,
            );
            continue;
        }

        let skills_config = config.skills.clone();
        let browser_skill =
            skills::find_skill_config(&skills_config, skills::OPEN_BROWSER_SKILL_ID).cloned();
        let windows_skill =
            skills::find_skill_config(&skills_config, skills::OPEN_WINDOWS_SKILL_ID).cloned();
        let matched_skills = skills::match_skills(clause_text, &skills_config);
        let mut consumed_local_end = 0usize;

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
                let local_consumed_end = next_match
                    .map(|next_skill_match| next_skill_match.start)
                    .unwrap_or(clause_text.len());

                if skills::is_config_skill(&skill_match.skill_id) {
                    if !is_final {
                        let wait_reason = if config_skill_requires_more_input(
                            clause_text,
                            skill_match,
                            next_match,
                        ) {
                            "more input"
                        } else {
                            "final transcript"
                        };
                        println!(
                            "[SKILL] #{} Waiting for {} before executing config skill: {}",
                            seq_id, wait_reason, skill_match.skill_id
                        );
                        return;
                    }

                    let action_key = format!("config:{}", skill_match.skill_id);
                    if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                        continue;
                    }

                    match execute_config_skill(
                        app_handle,
                        clause_text,
                        skill_match,
                        next_match,
                        &mut config,
                    ) {
                        Ok(_) => {
                            complete_skill_action(skill_state, skill_session_id, &action_key, true);
                            consumed_local_end = consumed_local_end.max(local_consumed_end);
                        }
                        Err(e) => {
                            complete_skill_action(skill_state, skill_session_id, &action_key, true);
                            emit_voice_command_feedback(
                                app_handle,
                                "error",
                                format!("配置更新失败: {}", e),
                            );
                            eprintln!(
                                "[SKILL] #{} Config skill execution failed for {}: {}",
                                seq_id, skill_match.skill_id, e
                            );
                        }
                    }
                    continue;
                }

                if skill_match.skill_id == skills::OPEN_WINDOWS_SKILL_ID {
                    let Some(windows_skill) = windows_skill.as_ref() else {
                        emit_voice_command_feedback(app_handle, "error", "Windows skill missing");
                        eprintln!("[SKILL] #{} Windows skill missing from config", seq_id);
                        continue;
                    };

                    match plan_windows_command(clause_text, windows_skill, Some(skill_match)) {
                        Ok(Some(plan)) => {
                            let should_wait_for_stability = matches!(
                                &plan.action,
                                skills::WindowsAction::OpenTarget { query }
                                    if resolve_windows_target_candidate(windows_skill, query).is_none()
                            );
                            if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                                if !is_windows_open_query_ready(windows_skill, query) {
                                    continue;
                                }
                                if !is_final
                                    && should_wait_for_stability
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &windows_action_key(&plan.action),
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    return;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            if execute_reserved_windows_plan(
                                app_handle,
                                skill_state,
                                skill_session_id,
                                windows_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                consumed_local_end = consumed_local_end.max(plan.consumed_end);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Windows plan failed: {}", seq_id, e);
                        }
                    }
                    continue;
                }

                if skill_match.skill_id == skills::OPEN_BROWSER_SKILL_ID {
                    let Some(browser_skill) = browser_skill.as_ref() else {
                        emit_voice_command_feedback(app_handle, "error", "娴忚鍣ㄦ妧鑳芥湭閰嶇疆");
                        eprintln!("[SKILL] #{} Browser skill missing from config", seq_id);
                        continue;
                    };

                    match plan_browser_command(clause_text, browser_skill, Some(skill_match)) {
                        Ok(Some(plan)) => {
                            if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                                if !is_browser_open_query_ready(browser_skill, query) {
                                    continue;
                                }
                                if !is_final
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &browser_action_key(&plan.action),
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    return;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            if execute_reserved_browser_plan(
                                app_handle,
                                skill_state,
                                skill_session_id,
                                browser_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                consumed_local_end = consumed_local_end.max(plan.consumed_end);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Browser plan failed: {}", seq_id, e);
                        }
                    }
                    continue;
                }

                let action_key = format!("skill:{}", skill_match.skill_id);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    continue;
                }

                match skills::execute_skill(&skill_match.skill_id) {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        consumed_local_end = consumed_local_end.max(local_consumed_end);
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        emit_voice_command_feedback(app_handle, "error", e.clone());
                        eprintln!(
                            "[SKILL] #{} Execution failed for {}: {}",
                            seq_id, skill_match.skill_id, e
                        );
                    }
                }
            }

            if consumed_local_end > 0 {
                advance_skill_transcript_consumed(
                    skill_state,
                    skill_session_id,
                    text,
                    transcript_offset + consumed_local_end.min(clause_consumed_end),
                );
                continue;
            }

            return;
        }

        if let Some(windows_skill) = windows_skill.as_ref() {
            if let Some(plan) = plan_direct_windows_target(clause_text, windows_skill) {
                if execute_reserved_windows_plan(
                    app_handle,
                    skill_state,
                    skill_session_id,
                    windows_skill,
                    &plan,
                    &config,
                    llm_cancel,
                    seq_id,
                )
                .await
                {
                    advance_skill_transcript_consumed(
                        skill_state,
                        skill_session_id,
                        text,
                        transcript_offset + clause_consumed_end,
                    );
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                    println!(
                        "[SKILL] #{} Executed direct Windows target successfully",
                        seq_id
                    );
                    continue;
                }
            }

            match plan_windows_command(clause_text, windows_skill, None) {
                Ok(Some(plan)) => {
                    if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                        if !is_windows_open_query_ready(windows_skill, query) {
                            return;
                        }
                        let requires_stable_confirmation =
                            resolve_windows_target_candidate(windows_skill, query).is_none();
                        if !is_final
                            && requires_stable_confirmation
                            && !confirm_streaming_browser_open_action(
                                skill_state,
                                skill_session_id,
                                &windows_action_key(&plan.action),
                            )
                        {
                            println!(
                                "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                                seq_id,
                                query.trim()
                            );
                            return;
                        }
                    } else {
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                    }

                    if execute_reserved_windows_plan(
                        app_handle,
                        skill_state,
                        skill_session_id,
                        windows_skill,
                        &plan,
                        &config,
                        llm_cancel,
                        seq_id,
                    )
                    .await
                    {
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end.min(clause_consumed_end),
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed Windows fallback successfully", seq_id);
                        continue;
                    }
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }

        if let Some(browser_skill) = browser_skill.as_ref() {
            match plan_browser_command(clause_text, browser_skill, None) {
                Ok(Some(plan)) => {
                    if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                        if !is_browser_open_query_ready(browser_skill, query) {
                            return;
                        }
                        if !is_final
                            && !confirm_streaming_browser_open_action(
                                skill_state,
                                skill_session_id,
                                &browser_action_key(&plan.action),
                            )
                        {
                            println!(
                                "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                                seq_id,
                                query.trim()
                            );
                            return;
                        }
                    } else {
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                    }

                    if execute_reserved_browser_plan(
                        app_handle,
                        skill_state,
                        skill_session_id,
                        browser_skill,
                        &plan,
                        &config,
                        llm_cancel,
                        seq_id,
                    )
                    .await
                    {
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end.min(clause_consumed_end),
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed browser fallback successfully", seq_id);
                        continue;
                    }
                }
                Ok(None) => {
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                    println!(
                        "[SKILL] #{} No skill matched for text: '{}'",
                        seq_id,
                        preview_text(clause_text, 40)
                    );
                }
                Err(_) => {}
            }
        }

        if clause_consumed_end < effective_text.len() {
            advance_skill_transcript_consumed(
                skill_state,
                skill_session_id,
                text,
                transcript_offset + clause_consumed_end,
            );
            continue;
        }

        return;
    }
}

async fn execute_reserved_windows_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    windows_skill: &skills::SkillConfig,
    plan: &skills::WindowsActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> bool {
    let action_key = windows_action_key(&plan.action);
    if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
        return false;
    }

    match execute_windows_plan(app_handle, windows_skill, plan, config, llm_cancel, seq_id).await {
        Ok(_) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            true
        }
        Err(e) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            emit_voice_command_feedback(app_handle, "error", e.clone());
            eprintln!("[SKILL] #{} Windows execution failed: {}", seq_id, e);
            false
        }
    }
}

async fn execute_reserved_browser_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    browser_skill: &skills::SkillConfig,
    plan: &skills::BrowserActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> bool {
    let action_key = browser_action_key(&plan.action);
    if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
        return false;
    }

    match execute_browser_plan(app_handle, browser_skill, plan, config, llm_cancel, seq_id).await {
        Ok(_) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            true
        }
        Err(e) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            emit_voice_command_feedback(app_handle, "error", e.clone());
            eprintln!("[SKILL] #{} Browser execution failed: {}", seq_id, e);
            false
        }
    }
}

fn spawn_skill_transcript_processing<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    llm_cancel: LlmCancelState,
    skill_state: SkillExecutionState,
    skill_session_id: u64,
    seq_id: u64,
) {
    if text.trim().is_empty() {
        return;
    }

    println!(
        "[SKILL] #{} Streaming update: {} chars, preview='{}'",
        seq_id,
        text.len(),
        preview_text(&text, 80)
    );

    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        execute_skill_transcript(
            &app_handle_clone,
            &text,
            &llm_cancel,
            &skill_state,
            skill_session_id,
            seq_id,
            false,
        )
        .await;
    });
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

fn start_skill_execution_session(state: &SkillExecutionState) -> u64 {
    let session_id = SKILL_SESSION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    if let Ok(mut guard) = state.lock() {
        *guard = Some(SkillExecutionSession {
            id: session_id,
            executed: HashSet::new(),
            pending: HashSet::new(),
            consumed_prefix: String::new(),
            last_streaming_browser_open_action: None,
        });
    }
    session_id
}

fn finish_skill_execution_session(state: &SkillExecutionState, session_id: u64) {
    if let Ok(mut guard) = state.lock() {
        if guard.as_ref().map(|session| session.id) == Some(session_id) {
            *guard = None;
        }
    }
}

fn current_skill_execution_session_id(state: &SkillExecutionState) -> Option<u64> {
    state
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|session| session.id))
}

fn reserve_skill_action(state: &SkillExecutionState, session_id: u64, action_key: &str) -> bool {
    let Ok(mut guard) = state.lock() else {
        return false;
    };
    let Some(session) = guard.as_mut() else {
        return false;
    };
    if session.id != session_id {
        return false;
    }
    if session.executed.contains(action_key) || session.pending.contains(action_key) {
        return false;
    }
    session.pending.insert(action_key.to_string());
    true
}

fn complete_skill_action(
    state: &SkillExecutionState,
    session_id: u64,
    action_key: &str,
    mark_executed: bool,
) {
    if let Ok(mut guard) = state.lock() {
        if let Some(session) = guard.as_mut() {
            if session.id == session_id {
                session.pending.remove(action_key);
                if mark_executed {
                    session.executed.insert(action_key.to_string());
                }
            }
        }
    }
}

fn browser_action_key(action: &skills::BrowserAction) -> String {
    match action {
        skills::BrowserAction::OpenTarget { query } => {
            format!("browser:open_target:{}", query.trim().to_lowercase())
        }
        skills::BrowserAction::SwitchTabIndex { index } => format!("browser:switch_tab:{}", index),
        skills::BrowserAction::Find { query } => format!(
            "browser:find:{}",
            query.as_deref().unwrap_or_default().trim().to_lowercase()
        ),
        other => format!("browser:{:?}", other),
    }
}

fn windows_action_key(action: &skills::WindowsAction) -> String {
    match action {
        skills::WindowsAction::OpenTarget { query } => {
            format!("windows:open_target:{}", query.trim().to_lowercase())
        }
        other => format!("windows:{:?}", other),
    }
}

fn normalize_direct_windows_query(query: &str) -> String {
    let mut normalized = query
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | ';' | '.' | ':' | '\n' | '\r' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
                )
        })
        .trim()
        .to_string();

    for prefix in [
        "\u{6253}\u{5f00}",
        "\u{542f}\u{52a8}",
        "\u{8fd0}\u{884c}",
        "open",
        "launch",
        "start",
        "run",
    ] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.trim().to_string();
            break;
        }
    }

    for suffix in [
        "\u{9875}\u{9762}",
        "\u{754c}\u{9762}",
        "\u{9762}\u{677f}",
        " page",
        " panel",
    ] {
        if let Some(stripped) = normalized.strip_suffix(suffix) {
            normalized = stripped.trim().to_string();
            break;
        }
    }

    normalized
}

fn resolve_windows_target_candidate(
    windows_skill: &skills::SkillConfig,
    query: &str,
) -> Option<skills::WindowsTargetConfig> {
    skills::resolve_windows_target(windows_skill, query).or_else(|| {
        let normalized = normalize_direct_windows_query(query);
        if normalized.is_empty() || normalized == query.trim() {
            None
        } else {
            skills::resolve_windows_target(windows_skill, &normalized)
        }
    })
}

fn plan_direct_windows_target(
    transcript: &str,
    windows_skill: &skills::SkillConfig,
) -> Option<skills::WindowsActionPlan> {
    let query = normalize_direct_windows_query(transcript);
    if query.is_empty() || resolve_windows_target_candidate(windows_skill, &query).is_none() {
        return None;
    }

    Some(skills::WindowsActionPlan {
        action: skills::WindowsAction::OpenTarget { query },
        action_name: "Open Windows Target".to_string(),
        note: None,
        consumed_end: transcript.len(),
    })
}

fn split_skill_clause(text: &str) -> (&str, usize) {
    let mut boundary: Option<(usize, usize)> = None;

    for (idx, ch) in text.char_indices() {
        if matches!(
            ch,
            ',' | ';' | '\n' | '\r' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
        ) {
            boundary = Some((idx, ch.len_utf8()));
            break;
        }
    }

    for marker in ["\u{7136}\u{540e}", "\u{5e76}\u{4e14}", "\u{63a5}\u{7740}"] {
        if let Some(idx) = text.find(marker) {
            let should_replace = boundary
                .map(|(current_idx, _)| idx < current_idx)
                .unwrap_or(true);
            if should_replace {
                boundary = Some((idx, marker.len()));
            }
        }
    }

    let Some((boundary_start, boundary_len)) = boundary else {
        return (text.trim_end(), text.len());
    };

    let clause = text[..boundary_start].trim_end_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | ';' | '.' | ':' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
            )
    });
    let mut consumed_end = boundary_start + boundary_len;

    while consumed_end < text.len() {
        let Some(next_char) = text[consumed_end..].chars().next() else {
            break;
        };
        if next_char.is_whitespace()
            || matches!(
                next_char,
                ',' | ';' | '.' | ':' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
            )
        {
            consumed_end += next_char.len_utf8();
            continue;
        }
        break;
    }

    (clause, consumed_end)
}

fn trim_skill_transcript_prefix(text: &str) -> (&str, usize) {
    let trimmed = text.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '，' | '。' | ',' | '.' | '、' | ';' | '；' | ':' | '：')
    });
    (trimmed, text.len() - trimmed.len())
}

fn shared_prefix_len(left: &str, right: &str) -> usize {
    let mut matched = 0usize;
    let mut left_chars = left.chars();
    let mut right_chars = right.chars();

    loop {
        match (left_chars.next(), right_chars.next()) {
            (Some(left_char), Some(right_char)) if left_char == right_char => {
                matched += left_char.len_utf8();
            }
            _ => break,
        }
    }

    matched
}

fn prepare_skill_transcript(
    state: &SkillExecutionState,
    session_id: u64,
    transcript: &str,
) -> Option<(String, usize)> {
    let Ok(mut guard) = state.lock() else {
        return None;
    };
    let Some(session) = guard.as_mut() else {
        return None;
    };
    if session.id != session_id {
        return None;
    }

    let consumed_len = shared_prefix_len(&session.consumed_prefix, transcript);
    if consumed_len < session.consumed_prefix.len() {
        session.consumed_prefix.truncate(consumed_len);
    }

    let remaining = &transcript[consumed_len..];
    let (trimmed, leading_offset) = trim_skill_transcript_prefix(remaining);
    if trimmed.trim().is_empty() {
        return None;
    }

    Some((trimmed.to_string(), consumed_len + leading_offset))
}

fn advance_skill_transcript_consumed(
    state: &SkillExecutionState,
    session_id: u64,
    transcript: &str,
    consumed_end: usize,
) {
    let Ok(mut guard) = state.lock() else {
        return;
    };
    let Some(session) = guard.as_mut() else {
        return;
    };
    if session.id != session_id {
        return;
    }

    let clamped_end = consumed_end.min(transcript.len());
    if !transcript.is_char_boundary(clamped_end) {
        return;
    }

    session.consumed_prefix = transcript[..clamped_end].to_string();
    session.last_streaming_browser_open_action = None;
}

fn clear_streaming_browser_open_action_candidate(state: &SkillExecutionState, session_id: u64) {
    if let Ok(mut guard) = state.lock() {
        if let Some(session) = guard.as_mut() {
            if session.id == session_id {
                session.last_streaming_browser_open_action = None;
            }
        }
    }
}

fn confirm_streaming_browser_open_action(
    state: &SkillExecutionState,
    session_id: u64,
    action_key: &str,
) -> bool {
    let Ok(mut guard) = state.lock() else {
        return false;
    };
    let Some(session) = guard.as_mut() else {
        return false;
    };
    if session.id != session_id {
        return false;
    }

    if session.last_streaming_browser_open_action.as_deref() == Some(action_key) {
        session.last_streaming_browser_open_action = None;
        true
    } else {
        session.last_streaming_browser_open_action = Some(action_key.to_string());
        false
    }
}

fn is_browser_open_query_ready(browser_skill: &skills::SkillConfig, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if skills::normalize_browser_url(trimmed).is_ok() {
        return true;
    }
    if skills::resolve_browser_site_url(browser_skill, trimmed).is_some() {
        return true;
    }

    let visible_len = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    let ascii_only = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ".-_/".contains(ch));
    if ascii_only {
        return visible_len >= 4;
    }

    visible_len >= 2
}

fn is_windows_open_query_ready(windows_skill: &skills::SkillConfig, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if resolve_windows_target_candidate(windows_skill, trimmed).is_some() {
        return true;
    }

    let visible_len = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    let ascii_only = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ".-_/".contains(ch));
    if ascii_only {
        return visible_len >= 3;
    }

    visible_len >= 2
}

fn set_browser_llm_state<R: Runtime>(app_handle: &AppHandle<R>, active: bool) {
    app_handle.emit("llm_processing", active).ok();
    let listener = app_handle.state::<InputListenerState>();
    listener
        .track_mouse_position
        .store(active, std::sync::atomic::Ordering::Relaxed);
    if active {
        show_indicator_window(app_handle);
    }
}

fn store_llm_cancel_token(llm_cancel: &LlmCancelState, token: Option<CancellationToken>) {
    if let Ok(mut guard) = llm_cancel.lock() {
        *guard = token;
    }
}

async fn resolve_browser_navigation_target<R: Runtime>(
    app_handle: &AppHandle<R>,
    browser_skill: &skills::SkillConfig,
    query: &str,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(String, String), String> {
    if let Ok(url) = skills::normalize_browser_url(query) {
        return Ok((url, format!("已打开网址：{}", query.trim())));
    }

    if let Some(url) = skills::resolve_browser_site_url(browser_skill, query) {
        return Ok((url, format!("已打开站点：{}", query.trim())));
    }

    let options = browser_skill
        .browser_options
        .as_ref()
        .ok_or_else(|| "浏览器技能配置缺失".to_string())?;

    let mut llm_reason: Option<String> = None;

    if options.llm_site_resolution_enabled {
        let cancel_token = CancellationToken::new();
        store_llm_cancel_token(llm_cancel, Some(cancel_token.clone()));
        set_browser_llm_state(app_handle, true);

        let resolution = tokio::select! {
            result = llm::resolve_browser_url(query, &config.llm_config, &config.proxy) => Some(result),
            _ = cancel_token.cancelled() => None,
        };

        set_browser_llm_state(app_handle, false);
        store_llm_cancel_token(llm_cancel, None);

        match resolution {
            Some(Ok(outcome)) => {
                if let Some(url) = outcome.resolved_url {
                    println!(
                        "[SKILL] #{} Browser target resolved via LLM: {}",
                        seq_id, url
                    );
                    return Ok((url, format!("已解析并打开：{}", query.trim())));
                }
                llm_reason = outcome.fallback_reason;
            }
            Some(Err(error)) => {
                llm_reason = Some(error.to_string());
            }
            None => return Err("浏览器网址解析已取消".to_string()),
        }
    }

    if options.search_fallback_enabled {
        let search_url = skills::build_browser_search_url(&options.search_url_template, query)?;
        let message = if let Some(reason) = llm_reason {
            format!(
                "未识别到精确网址，已改为搜索：{}（{}）",
                query.trim(),
                reason
            )
        } else {
            format!("未识别到精确网址，已改为搜索：{}", query.trim())
        };
        return Ok((search_url, message));
    }

    Err(llm_reason.unwrap_or_else(|| format!("未识别到可打开的网址：{}", query.trim())))
}

async fn resolve_windows_target<R: Runtime>(
    app_handle: &AppHandle<R>,
    windows_skill: &skills::SkillConfig,
    query: &str,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(skills::WindowsTargetConfig, String), String> {
    if let Some(target) = resolve_windows_target_candidate(windows_skill, query) {
        return Ok((target, format!("Opened Windows target: {}", query.trim())));
    }

    let options = windows_skill
        .windows_options
        .as_ref()
        .ok_or_else(|| "Windows skill configuration is missing".to_string())?;

    if !options.llm_target_resolution_enabled {
        return Err(format!("No Windows target matched: {}", query.trim()));
    }

    let cancel_token = CancellationToken::new();
    store_llm_cancel_token(llm_cancel, Some(cancel_token.clone()));
    set_browser_llm_state(app_handle, true);

    let resolution = tokio::select! {
        result = llm::resolve_windows_target(query, windows_skill, &config.llm_config, &config.proxy) => Some(result),
        _ = cancel_token.cancelled() => None,
    };

    set_browser_llm_state(app_handle, false);
    store_llm_cancel_token(llm_cancel, None);

    match resolution {
        Some(Ok(outcome)) => {
            if let Some(target_id) = outcome.resolved_target_id {
                if let Some(target) =
                    skills::resolve_windows_target_by_id(windows_skill, &target_id)
                {
                    println!(
                        "[SKILL] #{} Windows target resolved via LLM: {}",
                        seq_id, target_id
                    );
                    return Ok((
                        target,
                        format!("Resolved and opened Windows target: {}", query.trim()),
                    ));
                }
            }
            Err(outcome
                .fallback_reason
                .unwrap_or_else(|| format!("No Windows target matched: {}", query.trim())))
        }
        Some(Err(error)) => Err(error.to_string()),
        None => Err("Windows target resolution was cancelled".to_string()),
    }
}

fn plan_windows_command(
    transcript: &str,
    windows_skill: &skills::SkillConfig,
    windows_match: Option<&skills::SkillMatch>,
) -> Result<Option<skills::WindowsActionPlan>, String> {
    match skills::plan_windows_action(transcript, windows_skill, windows_match) {
        skills::WindowsPlanResult::None => Ok(None),
        skills::WindowsPlanResult::Feedback(message) => Err(message),
        skills::WindowsPlanResult::Action(plan) => Ok(Some(plan)),
    }
}

fn plan_browser_command(
    transcript: &str,
    browser_skill: &skills::SkillConfig,
    browser_match: Option<&skills::SkillMatch>,
) -> Result<Option<skills::BrowserActionPlan>, String> {
    match skills::plan_browser_action(transcript, browser_skill, browser_match) {
        skills::BrowserPlanResult::None => Ok(None),
        skills::BrowserPlanResult::Feedback(message) => Err(message),
        skills::BrowserPlanResult::Action(plan) => Ok(Some(plan)),
    }
}

async fn execute_browser_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    browser_skill: &skills::SkillConfig,
    plan: &skills::BrowserActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(), String> {
    let note = plan.note.clone();
    let success_message = match &plan.action {
        skills::BrowserAction::OpenTarget { query } => {
            let (url, message) = resolve_browser_navigation_target(
                app_handle,
                browser_skill,
                query,
                config,
                llm_cancel,
                seq_id,
            )
            .await?;
            skills::open_browser_url(&url)?;
            message
        }
        skills::BrowserAction::Find { query } => {
            skills::execute_browser_shortcut_action(&plan.action)?;
            match query.as_deref() {
                Some(value) if !value.is_empty() => format!("已打开查找并输入：{}", value),
                _ => "已打开页面查找".to_string(),
            }
        }
        skills::BrowserAction::SwitchTabIndex { index } => {
            skills::execute_browser_shortcut_action(&plan.action)?;
            format!("已切换到第 {} 个页面", index)
        }
        other_action => {
            skills::execute_browser_shortcut_action(other_action)?;
            match other_action {
                skills::BrowserAction::NewTab => "已新建浏览器页面".to_string(),
                skills::BrowserAction::CloseTab => "已关闭当前浏览器页面".to_string(),
                skills::BrowserAction::NextTab => "已切换到下一个页面".to_string(),
                skills::BrowserAction::PreviousTab => "已切换到上一个页面".to_string(),
                skills::BrowserAction::ReopenTab => "已重新打开最近关闭的页面".to_string(),
                skills::BrowserAction::GoBack => "已后退".to_string(),
                skills::BrowserAction::GoForward => "已前进".to_string(),
                skills::BrowserAction::Refresh => "已刷新页面".to_string(),
                skills::BrowserAction::HardRefresh => "已强制刷新页面".to_string(),
                skills::BrowserAction::StopLoading => "已停止页面加载".to_string(),
                skills::BrowserAction::GoHome => "已返回主页".to_string(),
                skills::BrowserAction::ScrollUp => "已向上滚动".to_string(),
                skills::BrowserAction::ScrollDown => "已向下滚动".to_string(),
                skills::BrowserAction::ScrollTop => "已滚动到顶部".to_string(),
                skills::BrowserAction::ScrollBottom => "已滚动到底部".to_string(),
                skills::BrowserAction::PageUp => "已向上翻页".to_string(),
                skills::BrowserAction::PageDown => "已向下翻页".to_string(),
                skills::BrowserAction::Fullscreen => "已切换全屏".to_string(),
                skills::BrowserAction::CopyUrl => "已复制当前网址".to_string(),
                skills::BrowserAction::OpenHistory => "已打开历史记录".to_string(),
                skills::BrowserAction::OpenDownloads => "已打开下载列表".to_string(),
                skills::BrowserAction::OpenDevtools => "已打开开发者工具".to_string(),
                skills::BrowserAction::MinimizeWindow => "已最小化浏览器窗口".to_string(),
                skills::BrowserAction::MaximizeWindow => "已最大化浏览器窗口".to_string(),
                skills::BrowserAction::NewPrivateWindow => "已新建隐私窗口".to_string(),
                skills::BrowserAction::CloseOtherTabs => "已执行关闭其他页面".to_string(),
                skills::BrowserAction::CloseTabsToRight => "已执行关闭右侧页面".to_string(),
                skills::BrowserAction::OpenTarget { .. }
                | skills::BrowserAction::Find { .. }
                | skills::BrowserAction::SwitchTabIndex { .. } => unreachable!(),
            }
        }
    };

    emit_voice_command_feedback(app_handle, "success", success_message);
    if let Some(note) = note {
        emit_voice_command_feedback(app_handle, "info", note);
    }
    Ok(())
}

async fn execute_windows_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    windows_skill: &skills::SkillConfig,
    plan: &skills::WindowsActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(), String> {
    let note = plan.note.clone();
    let success_message = match &plan.action {
        skills::WindowsAction::OpenTarget { query } => {
            let (target, message) = resolve_windows_target(
                app_handle,
                windows_skill,
                query,
                config,
                llm_cancel,
                seq_id,
            )
            .await?;
            skills::open_windows_target(&target)?;
            message
        }
        other_action => {
            skills::execute_windows_shortcut_action(other_action)?;
            match other_action {
                skills::WindowsAction::ShowDesktop => "Showed the desktop".to_string(),
                skills::WindowsAction::LockScreen => "Locked the screen".to_string(),
                skills::WindowsAction::OpenRunDialog => "Opened the Run dialog".to_string(),
                skills::WindowsAction::OpenClipboardHistory => {
                    "Opened clipboard history".to_string()
                }
                skills::WindowsAction::OpenQuickSettings => "Opened quick settings".to_string(),
                skills::WindowsAction::OpenNotifications => {
                    "Opened notification center".to_string()
                }
                skills::WindowsAction::OpenTarget { .. } => unreachable!(),
            }
        }
    };

    emit_voice_command_feedback(app_handle, "success", success_message);
    if let Some(note) = note {
        emit_voice_command_feedback(app_handle, "info", note);
    }
    Ok(())
}

fn config_skill_requires_more_input(
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
) -> bool {
    match skill_match.skill_id.as_str() {
        skills::SWITCH_POLISH_SCENE_SKILL_ID => {
            skills::extract_scene_query(transcript, skill_match, next_match).is_empty()
        }
        _ => false,
    }
}

fn plan_config_skill_update(
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
    config: &AppConfig,
) -> Result<ConfigSkillPlan, String> {
    match skill_match.skill_id.as_str() {
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
                skills::SceneResolveResult::None => {
                    Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                        level: "error".to_string(),
                        message: format!("未找到匹配场景：{}", scene_query),
                    }))
                }
                skills::SceneResolveResult::Ambiguous(names) => {
                    Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                        level: "error".to_string(),
                        message: format!("匹配到多个场景：{}", names.join("、")),
                    }))
                }
            }
        }
        _ => Err(format!(
            "Unsupported config skill: {}",
            skill_match.skill_id
        )),
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
    intent: DictationIntent,
    skill_mode: bool,
    llm_cancel: LlmCancelState,
    skill_state: SkillExecutionState,
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
    emit_dictation_intent(app_handle, intent);
    show_indicator_window(app_handle);

    let storage = app_handle.state::<StorageState>();
    let config = storage.load_config();
    let asr = app_handle.state::<AsrState>();
    let handle = app_handle.clone();
    let skill_session_id = if skill_mode {
        start_skill_execution_session(&skill_state)
    } else {
        0
    };
    match asr.start_streaming_session(
        stream_rx,
        sample_rate,
        config.online_asr_config,
        config.proxy,
        move |text| {
            handle.emit("stream_update", &text).ok();
            if skill_mode {
                let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
                spawn_skill_transcript_processing(
                    &handle,
                    text,
                    llm_cancel.clone(),
                    skill_state.clone(),
                    skill_session_id,
                    seq_id,
                );
            }
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
            emit_dictation_intent(app_handle, DictationIntent::None);
            emit_session_complete(app_handle);
            if let Ok(audio) = app_handle.state::<AudioState>().lock() {
                let _ = audio.stop_recording();
            }
            false
        }
    }
}

fn stop_recording_now<R: Runtime>(app_handle: &AppHandle<R>) -> (Vec<f32>, u32) {
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

fn finish_streaming_asr_async(
    tx: std::sync::mpsc::Sender<input_listener::InputEvent>,
    session_id: u64,
    session: asr::StreamingSession,
    log_tag: &str,
) {
    let log_tag = log_tag.to_string();
    std::thread::spawn(move || {
        let transcribe_started = Instant::now();
        let result = session.finish_and_wait().map_err(|err| err.to_string());
        println!(
            "[{}] Dictation ASR finished in {} ms for session {}",
            log_tag,
            transcribe_started.elapsed().as_millis(),
            session_id
        );
        tx.send(input_listener::InputEvent::DictationAsrFinished { session_id, result })
            .ok();
    });
}

fn dispatch_final_transcription<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    intent: DictationIntent,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    log_tag: &str,
) {
    let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    println!(
        "[{}] #{} Dispatching final dictation, intent={:?}, {} chars, preview='{}'",
        log_tag,
        seq_id,
        intent,
        text.len(),
        preview_text(&text, 80)
    );
    process_transcription(
        app_handle,
        text,
        processing,
        llm_cancel,
        seq_id,
        matches!(intent, DictationIntent::Polish),
    );
}

fn maybe_finalize_pending_dictation<R: Runtime>(
    app_handle: &AppHandle<R>,
    pending: &mut PendingFinalizeState,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    log_tag: &str,
) -> bool {
    if !pending.window_elapsed {
        return false;
    }

    let Some(result) = pending.asr_result.take() else {
        return false;
    };

    match result {
        Ok(text) => {
            dispatch_final_transcription(
                app_handle,
                text,
                pending.intent,
                processing,
                llm_cancel,
                log_tag,
            );
        }
        Err(err) => {
            eprintln!("[{}] Transcription error: {}", log_tag, err);
            app_handle.emit("recognition_processing", false).ok();
            emit_dictation_intent(app_handle, DictationIntent::None);
            emit_session_complete(app_handle);
            processing.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    true
}

fn begin_pending_finalize_window(
    tx: std::sync::mpsc::Sender<input_listener::InputEvent>,
    session_id: u64,
) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(DOUBLE_CLICK_WINDOW_MS));
        tx.send(input_listener::InputEvent::DictationFinalizeWindowElapsed { session_id })
            .ok();
    });
}

fn stop_dictation_recording<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    session_id: u64,
    tx: std::sync::mpsc::Sender<input_listener::InputEvent>,
    log_tag: &str,
) -> Result<(), String> {
    let stop_started = Instant::now();
    let (_buffer, _sample_rate) = stop_recording_now(app_handle);
    println!(
        "[{}] Capture stopped in {} ms",
        log_tag,
        stop_started.elapsed().as_millis()
    );

    app_handle.emit("recognition_processing", true).ok();
    begin_pending_finalize_window(tx.clone(), session_id);

    let Some(session) = streaming_session.take() else {
        return Err("No active streaming session to finish".to_string());
    };

    finish_streaming_asr_async(tx, session_id, session, log_tag);
    Ok(())
}

fn stop_skill_recording_async<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    llm_cancel: LlmCancelState,
    skill_state: SkillExecutionState,
    log_tag: &str,
) {
    let stop_started = Instant::now();
    let (_buffer, _sample_rate) = stop_recording_now(app_handle);
    println!(
        "[{}] Skill capture stopped in {} ms",
        log_tag,
        stop_started.elapsed().as_millis()
    );
    emit_dictation_intent(app_handle, DictationIntent::None);
    emit_session_complete(app_handle);

    let Some(session) = streaming_session.take() else {
        if let Some(session_id) = current_skill_execution_session_id(&skill_state) {
            finish_skill_execution_session(&skill_state, session_id);
        }
        return;
    };

    let Some(skill_session_id) = current_skill_execution_session_id(&skill_state) else {
        return;
    };

    let app_handle_clone = app_handle.clone();
    let log_tag = log_tag.to_string();
    std::thread::spawn(move || match session.finish_and_wait() {
        Ok(text) => {
            app_handle_clone.emit("stream_update", text.clone()).ok();
            let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
            let app_handle_for_async = app_handle_clone.clone();
            let skill_state_for_async = skill_state.clone();
            tauri::async_runtime::spawn(async move {
                execute_skill_transcript(
                    &app_handle_for_async,
                    &text,
                    &llm_cancel,
                    &skill_state_for_async,
                    skill_session_id,
                    seq_id,
                    true,
                )
                .await;
                finish_skill_execution_session(&skill_state_for_async, skill_session_id);
            });
        }
        Err(e) => {
            eprintln!("[{}] Skill final transcription error: {}", log_tag, e);
            finish_skill_execution_session(&skill_state, skill_session_id);
        }
    });
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
    config: AppConfig,
) -> Result<(), String> {
    // Update listener flags immediately (hot-reload)
    listener
        .enable_mouse
        .store(config.trigger_mouse, std::sync::atomic::Ordering::Relaxed);
    listener
        .enable_alt
        .store(config.trigger_toggle, std::sync::atomic::Ordering::Relaxed);

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
    let configured = !config.online_asr_config.app_key.is_empty()
        && !config.online_asr_config.access_key.is_empty();
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
    device_id: String,
) -> Result<(), String> {
    // Update audio service
    if let Ok(mut audio) = audio.lock() {
        audio
            .init_with_device(&device_id, app.clone())
            .map_err(|e| e.to_string())?;
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
    llm::test_connection(&config, &proxy)
        .await
        .map_err(|e| e.to_string())
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
            input_listener.start(tx.clone());

            // Shared processing flag:
            // We must NOT allow a new transcription/paste to start while the previous async
            // pipeline (LLM + enigo typing) is still running; otherwise keystrokes interleave
            // and output becomes garbled/duplicated.
            let processing_state: ProcessingState = Arc::new(std::sync::atomic::AtomicBool::new(false));

            // LLM cancellation state - allows cancelling ongoing LLM requests
            let llm_cancel_state: LlmCancelState = Arc::new(Mutex::new(None));
            let skill_execution_state: SkillExecutionState = Arc::new(Mutex::new(None));

            // Background Thread to handle events
            let processing_for_thread = processing_state.clone();
            let llm_cancel_for_thread = llm_cancel_state.clone();
            let skill_execution_for_thread = skill_execution_state.clone();
            let event_tx = tx.clone();
            #[allow(unreachable_code)]
            std::thread::spawn(move || {
                let mut dictation_state = DictationState::Idle;
                let mut streaming_session: Option<asr::StreamingSession> = None;

                for event in rx {
                    match event {
                        input_listener::InputEvent::Click => {
                            match &mut dictation_state {
                                DictationState::Idle => {
                                    if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                        cancel_pending_llm(&llm_cancel_for_thread, "CLICK");
                                        std::thread::sleep(Duration::from_millis(50));
                                    }

                                    if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                        continue;
                                    }

                                    let forced_polish = app_handle
                                        .state::<StorageState>()
                                        .load_config()
                                        .llm_config
                                        .enabled;
                                    let intent = if forced_polish {
                                        DictationIntent::Polish
                                    } else {
                                        DictationIntent::Raw
                                    };

                                    if begin_recording_session(
                                        &app_handle,
                                        &mut streaming_session,
                                        intent,
                                        false,
                                        llm_cancel_for_thread.clone(),
                                        skill_execution_for_thread.clone(),
                                    ) {
                                        dictation_state = DictationState::Recording {
                                            intent,
                                            started_at: Instant::now(),
                                        };
                                    }
                                }
                                DictationState::Recording { intent, started_at } => {
                                    let forced_polish = app_handle
                                        .state::<StorageState>()
                                        .load_config()
                                        .llm_config
                                        .enabled;

                                    if started_at.elapsed()
                                        <= Duration::from_millis(DOUBLE_CLICK_WINDOW_MS)
                                    {
                                        if !forced_polish && *intent == DictationIntent::Raw {
                                            *intent = DictationIntent::Polish;
                                            emit_dictation_intent(
                                                &app_handle,
                                                DictationIntent::Polish,
                                            );
                                        }
                                        continue;
                                    }

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

                                    let session_id =
                                        DICTATION_SESSION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
                                    let final_intent = if forced_polish {
                                        DictationIntent::Polish
                                    } else {
                                        *intent
                                    };

                                    match stop_dictation_recording(
                                        &app_handle,
                                        &mut streaming_session,
                                        session_id,
                                        event_tx.clone(),
                                        "CLICK",
                                    ) {
                                        Ok(()) => {
                                            dictation_state = DictationState::PendingFinalize(
                                                PendingFinalizeState {
                                                    session_id,
                                                    intent: final_intent,
                                                    window_elapsed: false,
                                                    asr_result: None,
                                                },
                                            );
                                        }
                                        Err(err) => {
                                            eprintln!(
                                                "[CLICK] Failed to finalize dictation session: {}",
                                                err
                                            );
                                            processing_for_thread.store(
                                                false,
                                                std::sync::atomic::Ordering::SeqCst,
                                            );
                                            emit_dictation_intent(
                                                &app_handle,
                                                DictationIntent::None,
                                            );
                                            emit_session_complete(&app_handle);
                                            dictation_state = DictationState::Idle;
                                        }
                                    }
                                }
                                DictationState::PendingFinalize(pending) => {
                                    if pending.window_elapsed {
                                        continue;
                                    }

                                    let forced_polish = app_handle
                                        .state::<StorageState>()
                                        .load_config()
                                        .llm_config
                                        .enabled;
                                    if !forced_polish
                                        && pending.intent == DictationIntent::Raw
                                    {
                                        pending.intent = DictationIntent::Polish;
                                        emit_dictation_intent(
                                            &app_handle,
                                            DictationIntent::Polish,
                                        );
                                    }
                                }
                                DictationState::SkillRecording => {}
                            }
                        }
                        input_listener::InputEvent::MouseMove => {
                            // Mouse movement detected - indicator stays at bottom-center
                        }
                        input_listener::InputEvent::StartSkill => {
                            if !matches!(dictation_state, DictationState::Idle) {
                                continue;
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                cancel_pending_llm(&llm_cancel_for_thread, "SKILL");
                                std::thread::sleep(Duration::from_millis(50));
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                continue;
                            }

                            if begin_recording_session(
                                &app_handle,
                                &mut streaming_session,
                                DictationIntent::Skill,
                                true,
                                llm_cancel_for_thread.clone(),
                                skill_execution_for_thread.clone(),
                            ) {
                                dictation_state = DictationState::SkillRecording;
                            }
                        }
                        input_listener::InputEvent::StopSkill => {
                            if !matches!(dictation_state, DictationState::SkillRecording) {
                                continue;
                            }

                            dictation_state = DictationState::Idle;

                            stop_skill_recording_async(
                                &app_handle,
                                &mut streaming_session,
                                llm_cancel_for_thread.clone(),
                                skill_execution_for_thread.clone(),
                                "SKILL",
                            );
                        }
                        input_listener::InputEvent::DictationFinalizeWindowElapsed {
                            session_id,
                        } => {
                            let should_reset = if let DictationState::PendingFinalize(pending) =
                                &mut dictation_state
                            {
                                if pending.session_id != session_id {
                                    false
                                } else {
                                    pending.window_elapsed = true;
                                    maybe_finalize_pending_dictation(
                                        &app_handle,
                                        pending,
                                        processing_for_thread.clone(),
                                        llm_cancel_for_thread.clone(),
                                        "CLICK",
                                    )
                                }
                            } else {
                                false
                            };

                            if should_reset {
                                dictation_state = DictationState::Idle;
                            }
                        }
                        input_listener::InputEvent::DictationAsrFinished {
                            session_id,
                            result,
                        } => {
                            let should_reset = if let DictationState::PendingFinalize(pending) =
                                &mut dictation_state
                            {
                                if pending.session_id != session_id {
                                    false
                                } else {
                                    if let Ok(text) = result.as_ref() {
                                        app_handle.emit("stream_update", text.clone()).ok();
                                    }
                                    app_handle.emit("recognition_processing", false).ok();
                                    pending.asr_result = Some(result);
                                    maybe_finalize_pending_dictation(
                                        &app_handle,
                                        pending,
                                        processing_for_thread.clone(),
                                        llm_cancel_for_thread.clone(),
                                        "CLICK",
                                    )
                                }
                            } else {
                                false
                            };

                            if should_reset {
                                dictation_state = DictationState::Idle;
                            }
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
    use super::{
        advance_skill_transcript_consumed, config_skill_requires_more_input,
        confirm_streaming_browser_open_action, plan_config_skill_update, prepare_skill_transcript,
        AppConfig, ConfigSkillPlan, SkillExecutionSession, SkillExecutionState,
        VoiceCommandFeedback,
    };
    use crate::skills::{SkillMatch, SWITCH_POLISH_SCENE_SKILL_ID};
    use crate::storage::PromptProfile;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

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
            voice_aliases: voice_aliases
                .iter()
                .map(|alias| alias.to_string())
                .collect(),
            ..PromptProfile::new_default()
        }
    }

    fn test_skill_state() -> SkillExecutionState {
        Arc::new(Mutex::new(Some(SkillExecutionSession {
            id: 7,
            executed: HashSet::new(),
            pending: HashSet::new(),
            consumed_prefix: String::new(),
            last_streaming_browser_open_action: None,
        })))
    }

    #[test]
    fn split_skill_clause_stops_at_first_separator() {
        let input = "控制面板，打开命令提示符";
        let (clause, consumed_end) = super::split_skill_clause(input);

        assert_eq!(clause, "控制面板");
        assert_eq!(&input[consumed_end..], "打开命令提示符");
    }

    #[test]
    fn prepare_skill_transcript_can_continue_after_unmatched_intro_clause() {
        let state = test_skill_state();
        let transcript = "那现在的话，我可以直接跟他说，帮我切换到中译英";
        let (first_clause, consumed_end) = super::split_skill_clause(transcript);

        assert_eq!(first_clause, "那现在的话");

        advance_skill_transcript_consumed(&state, 7, transcript, consumed_end);
        let prepared =
            prepare_skill_transcript(&state, 7, transcript).expect("expected remaining clause");

        assert_eq!(prepared.0, "我可以直接跟他说，帮我切换到中译英");
    }

    #[test]
    fn normalize_direct_windows_query_strips_open_prefix_and_page_suffix() {
        assert_eq!(
            super::normalize_direct_windows_query("打开设置页面"),
            "设置"
        );
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
    fn switch_scene_waits_for_scene_name_during_streaming() {
        let skill_match = SkillMatch {
            skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
            keyword: "切换到".to_string(),
            start: 0,
            end: "切换到".len(),
        };

        assert!(config_skill_requires_more_input(
            "切换到",
            &skill_match,
            None
        ));
        assert!(!config_skill_requires_more_input(
            "切换到中译英",
            &skill_match,
            None
        ));
    }

    #[test]
    fn prepare_skill_transcript_removes_consumed_prefix_and_leading_punctuation() {
        let state = test_skill_state();
        advance_skill_transcript_consumed(&state, 7, "打开新浪", "打开新浪".len());

        let prepared = prepare_skill_transcript(&state, 7, "打开新浪。打开谷歌")
            .expect("expected remaining transcript");

        assert_eq!(prepared, ("打开谷歌".to_string(), "打开新浪。".len()));
    }

    #[test]
    fn prepare_skill_transcript_rewinds_to_common_prefix_when_asr_rewrites_text() {
        let state = test_skill_state();
        advance_skill_transcript_consumed(&state, 7, "打开新浪", "打开新浪".len());

        let prepared =
            prepare_skill_transcript(&state, 7, "打开新郎").expect("expected rewritten transcript");

        assert_eq!(prepared, ("郎".to_string(), "打开新".len()));
    }

    #[test]
    fn streaming_browser_open_requires_two_matching_updates() {
        let state = test_skill_state();

        assert!(!confirm_streaming_browser_open_action(
            &state,
            7,
            "browser:open_target:新浪"
        ));
        assert!(confirm_streaming_browser_open_action(
            &state,
            7,
            "browser:open_target:新浪"
        ));
        assert!(!confirm_streaming_browser_open_action(
            &state,
            7,
            "browser:open_target:谷歌"
        ));
    }
}
