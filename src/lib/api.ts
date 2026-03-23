import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface AsrStatus {
    configured: boolean;
}

export interface AudioDevice {
    id: string;
    name: string;
    is_default: boolean;
}

export interface SceneExample {
    input: string;
    output: string;
}

export interface PromptProfile {
    id: string;
    name: string;
    task_kind: string;
    goal: string;
    tone: string;
    format_style: string;
    preserve_rules: string[];
    glossary: string[];
    examples: SceneExample[];
    advanced_instruction: string;
    expert_mode: boolean;
    legacy_imported: boolean;
}

export interface LlmConfig {
    enabled: boolean;
    base_url: string;
    api_key: string;
    model: string;
    profiles: PromptProfile[];
    active_profile_id: string;
}

export interface ProxyConfig {
    enabled: boolean;
    url: string;
}

export interface OnlineAsrConfig {
    app_key: string;
    access_key: string;
    resource_id: string;
}

export interface SkillConfig {
    id: string;
    name: string;
    keywords: string;
    enabled: boolean;
}

export interface AppConfig {
    trigger_mouse: boolean;
    trigger_hold: boolean;
    trigger_toggle: boolean;
    online_asr_config: OnlineAsrConfig;
    input_device: string;
    llm_config: LlmConfig;
    proxy: ProxyConfig;
    skills: SkillConfig[];
}

export interface HistoryItem {
    id: string;
    timestamp: string;
    text: string;
    duration_ms: number;
}

export const api = {
    getConfig: () => invoke<AppConfig>("get_config"),
    takeRuntimeNotice: () => invoke<string | null>("take_runtime_notice"),
    saveConfig: (config: AppConfig) => invoke("save_config", { config }),
    getHistory: () => invoke<HistoryItem[]>("get_history"),
    clearHistory: () => invoke("clear_history"),
    deleteHistoryItem: (id: string) => invoke("delete_history_item", { id }),
    getAsrStatus: () => invoke<AsrStatus>("get_asr_status"),
    getInputDevices: () => invoke<AudioDevice[]>("get_input_devices"),
    getCurrentInputDevice: () => invoke<string>("get_current_input_device"),
    switchInputDevice: (deviceId: string) => invoke("switch_input_device", { deviceId }),
    startAudioTest: () => invoke("start_audio_test"),
    stopAudioTest: () => invoke("stop_audio_test"),
    testLlmConnection: (config: LlmConfig, proxy: ProxyConfig) => invoke<string>("test_llm_connection", { config, proxy }),
    getDefaultSceneTemplate: () => invoke<PromptProfile>("get_default_scene_template"),
};

export const events = {
    onTranscriptionUpdate: (callback: (payload: HistoryItem) => void) => listen<HistoryItem>("transcription_update", (e) => callback(e.payload)),
    onRecordingStatus: (callback: (isRecording: boolean) => void) => listen<boolean>("recording_status", (e) => callback(e.payload)),
    onRecognitionProcessing: (callback: (isProcessing: boolean) => void) => listen<boolean>("recognition_processing", (e) => callback(e.payload)),
    onAudioLevel: (callback: (level: number) => void) => listen<number>("audio_level", (e) => callback(e.payload)),
    onLlmProcessing: (callback: (isProcessing: boolean) => void) => listen<boolean>("llm_processing", (e) => callback(e.payload)),
    onLlmError: (callback: (message: string) => void) => listen<string>("llm_error", (e) => callback(e.payload)),
    onMousePosition: (callback: (pos: { x: number; y: number }) => void) => listen<{ x: number; y: number }>("mouse_position", (e) => callback(e.payload)),
    onStreamUpdate: (callback: (text: string) => void) => listen<string>("stream_update", (e) => callback(e.payload)),
};
