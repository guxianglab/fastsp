import { useCallback, useEffect, useRef, useState } from "react";
import {
    AlertCircle,
    Check,
    ChevronDown,
    ChevronUp,
    Globe,
    Info,
    Keyboard,
    Loader2,
    Mic,
    Plus,
    RotateCcw,
    Sparkles,
    Trash2,
    X,
    Zap,
} from "lucide-react";
import {
    api,
    AppConfig,
    AudioDevice,
    LlmConfig,
    PromptProfile,
    ProxyConfig,
    SceneExample,
    SkillConfig,
    events,
} from "../lib/api";

interface SettingsModalProps {
    isOpen: boolean;
    onClose: () => void;
    isFirstSetup?: boolean;
}

type SceneTaskKind =
    | "plain_correction"
    | "email"
    | "meeting_notes"
    | "customer_service"
    | "custom_transform";

type SceneTemplateDescriptor = {
    key: SceneTaskKind;
    label: string;
    description: string;
    profile: Pick<PromptProfile, "task_kind" | "goal" | "tone" | "format_style" | "preserve_rules">;
};

const SCENE_TEMPLATES: SceneTemplateDescriptor[] = [
    {
        key: "plain_correction",
        label: "纠错",
        description: "只修正识别错误，不改原意。",
        profile: {
            task_kind: "plain_correction",
            goal: "修正明显的 ASR 错误，让文本更像自然书写，但不改变原意。",
            tone: "自然、克制、贴近原说话人。",
            format_style: "输出单段或单条可直接粘贴的文本。",
            preserve_rules: ["保留数字、专有名词、品牌名和事实。", "不要补充未说出的信息。"],
        },
    },
    {
        key: "email",
        label: "邮件",
        description: "整理成可直接发送的邮件正文。",
        profile: {
            task_kind: "email",
            goal: "把口述整理成清晰、专业、可直接发送的邮件正文。",
            tone: "专业、友好、简洁。",
            format_style: "输出邮件正文，包含称呼、主体和结尾。",
            preserve_rules: ["保留时间、承诺、结论和关键信息。", "不要虚构收件人和事实。"],
        },
    },
    {
        key: "meeting_notes",
        label: "纪要",
        description: "整理成会议纪要或结构化要点。",
        profile: {
            task_kind: "meeting_notes",
            goal: "把口述整理成结构清晰的会议纪要。",
            tone: "中性、清晰、结果导向。",
            format_style: "优先按要点或小节组织，突出结论、风险、待办。",
            preserve_rules: ["不要添加未明确提到的 owner、截止日期或决策。", "术语和产品名保持准确。"],
        },
    },
    {
        key: "customer_service",
        label: "客服",
        description: "整理成可直接发给客户的回复。",
        profile: {
            task_kind: "customer_service",
            goal: "把口述整理成清晰、礼貌、可直接发送给客户的回复。",
            tone: "同理心、专业、明确。",
            format_style: "输出单条完整回复，不要解释内部过程。",
            preserve_rules: ["保留政策、金额、时间和承诺。", "不要暴露内部规则或推测。"],
        },
    },
    {
        key: "custom_transform",
        label: "自定义",
        description: "自由定义一个受控的自定义转换场景。",
        profile: {
            task_kind: "custom_transform",
            goal: "根据场景配置把口述内容转换成单一的最终文本。",
            tone: "按场景要求执行。",
            format_style: "输出单一 final_text，不添加解释。",
            preserve_rules: ["不要暴露内部规则。", "除非场景明确允许，否则保留事实和关键实体。"],
        },
    },
];

const FALLBACK_SCENE_TEMPLATE: PromptProfile = {
    id: "default",
    name: "Default",
    task_kind: "plain_correction",
    goal: "",
    tone: "",
    format_style: "",
    preserve_rules: [],
    glossary: [],
    examples: [],
    advanced_instruction: "",
    expert_mode: false,
    legacy_imported: false,
};

function cloneExamples(examples: SceneExample[]) {
    return examples.map((example) => ({ ...example }));
}

function toMultiline(items: string[]) {
    return items.join("\n");
}

function fromMultiline(value: string) {
    return value
        .split("\n")
        .map((item) => item.trim())
        .filter(Boolean);
}

function templateForTaskKind(taskKind: string) {
    return SCENE_TEMPLATES.find((template) => template.key === taskKind) ?? SCENE_TEMPLATES[0];
}

export function SettingsModal({ isOpen, onClose, isFirstSetup = false }: SettingsModalProps) {
    const [config, setConfig] = useState<AppConfig | null>(null);
    const [inputDevices, setInputDevices] = useState<AudioDevice[]>([]);
    const [currentDevice, setCurrentDevice] = useState("");
    const [switchingDevice, setSwitchingDevice] = useState(false);
    const [isTesting, setIsTesting] = useState(false);
    const [audioLevel, setAudioLevel] = useState(0);
    const [llmTesting, setLlmTesting] = useState(false);
    const [llmTestResult, setLlmTestResult] = useState<{ success: boolean; message: string } | null>(null);
    const [showSceneEditor, setShowSceneEditor] = useState(false);
    const [defaultSceneTemplate, setDefaultSceneTemplate] = useState<PromptProfile | null>(null);
    const [showCloseWarning, setShowCloseWarning] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [saveSuccess, setSaveSuccess] = useState(false);
    const saveTimeoutRef = useRef<number | null>(null);
    const pendingConfigRef = useRef<AppConfig | null>(null);

    const debouncedSave = useCallback(async (configToSave: AppConfig) => {
        pendingConfigRef.current = configToSave;
        if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
        setIsSaving(true);
        setSaveSuccess(false);
        saveTimeoutRef.current = window.setTimeout(async () => {
            const finalConfig = pendingConfigRef.current;
            if (finalConfig) {
                try {
                    await api.saveConfig(finalConfig);
                    setSaveSuccess(true);
                    setTimeout(() => setSaveSuccess(false), 1500);
                } catch (error) {
                    console.error("Failed to save config:", error);
                }
            }
            setIsSaving(false);
            pendingConfigRef.current = null;
        }, 300);
    }, []);

    useEffect(() => {
        return () => {
            if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
        };
    }, []);

    useEffect(() => {
        if (!isOpen) return;
        api.getConfig().then(setConfig);
        api.getInputDevices().then(setInputDevices);
        api.getCurrentInputDevice().then(setCurrentDevice);
        api.getDefaultSceneTemplate().then(setDefaultSceneTemplate);
        setLlmTestResult(null);
        const interval = setInterval(() => {
            api.getInputDevices().then(setInputDevices);
        }, 3000);
        return () => clearInterval(interval);
    }, [isOpen]);

    useEffect(() => {
        if (!isTesting) return;
        const unsub = events.onAudioLevel((level) => setAudioLevel(Math.min(1, level * 5)));
        return () => {
            unsub.then((fn) => fn());
        };
    }, [isTesting]);

    useEffect(() => {
        if (!isOpen && isTesting) {
            api.stopAudioTest();
            setIsTesting(false);
            setAudioLevel(0);
        }
    }, [isOpen, isTesting]);

    const updateConfig = (key: keyof AppConfig, value: any) => {
        if (!config) return;
        const newConfig = { ...config, [key]: value };
        setConfig(newConfig);
        debouncedSave(newConfig);
    };

    const updateLlmConfig = (key: keyof LlmConfig, value: any) => {
        if (!config) return;
        const newLlmConfig = { ...config.llm_config, [key]: value };
        const newConfig = { ...config, llm_config: newLlmConfig };
        setConfig(newConfig);
        debouncedSave(newConfig);
        setLlmTestResult(null);
    };

    const updateProxyConfig = (key: keyof ProxyConfig, value: any) => {
        if (!config) return;
        const newConfig = { ...config, proxy: { ...config.proxy, [key]: value } };
        setConfig(newConfig);
        debouncedSave(newConfig);
    };

    const updateSkillConfig = (skillId: string, key: keyof SkillConfig, value: any) => {
        if (!config) return;
        const newSkills = config.skills.map((skill) => (skill.id === skillId ? { ...skill, [key]: value } : skill));
        const newConfig = { ...config, skills: newSkills };
        setConfig(newConfig);
        debouncedSave(newConfig);
    };

    const activeProfile =
        config?.llm_config.profiles.find((profile) => profile.id === config.llm_config.active_profile_id) ??
        config?.llm_config.profiles[0] ??
        null;

    const updateActiveProfile = (patch: Partial<PromptProfile>) => {
        if (!config || !activeProfile) return;
        const newProfiles = config.llm_config.profiles.map((profile) =>
            profile.id === activeProfile.id ? { ...profile, ...patch } : profile
        );
        updateLlmConfig("profiles", newProfiles);
    };

    const switchProfile = (profileId: string) => updateLlmConfig("active_profile_id", profileId);

    const createProfile = () => {
        if (!config) return;
        const template = defaultSceneTemplate ?? FALLBACK_SCENE_TEMPLATE;
        const newId = `profile_${Date.now()}`;
        const newProfile: PromptProfile = {
            ...template,
            id: newId,
            name: `场景 ${config.llm_config.profiles.length + 1}`,
            preserve_rules: [...template.preserve_rules],
            glossary: [...template.glossary],
            examples: cloneExamples(template.examples),
            advanced_instruction: "",
            expert_mode: false,
            legacy_imported: false,
        };
        const newConfig = {
            ...config,
            llm_config: { ...config.llm_config, profiles: [...config.llm_config.profiles, newProfile], active_profile_id: newId },
        };
        setConfig(newConfig);
        debouncedSave(newConfig);
    };

    const deleteActiveProfile = () => {
        if (!config || !activeProfile || config.llm_config.profiles.length <= 1) return;
        const newProfiles = config.llm_config.profiles.filter((profile) => profile.id !== activeProfile.id);
        const newConfig = {
            ...config,
            llm_config: { ...config.llm_config, profiles: newProfiles, active_profile_id: newProfiles[0].id },
        };
        setConfig(newConfig);
        debouncedSave(newConfig);
    };

    const resetActiveProfile = () => {
        if (!activeProfile) return;
        const template = templateForTaskKind(activeProfile.task_kind);
        updateActiveProfile({
            task_kind: template.profile.task_kind,
            goal: template.profile.goal,
            tone: template.profile.tone,
            format_style: template.profile.format_style,
            preserve_rules: [...template.profile.preserve_rules],
            glossary: [],
            examples: [],
            advanced_instruction: "",
            expert_mode: false,
            legacy_imported: false,
        });
    };

    const applySceneTemplate = (taskKind: SceneTaskKind) => {
        const template = templateForTaskKind(taskKind);
        updateActiveProfile({
            task_kind: template.profile.task_kind,
            goal: template.profile.goal,
            tone: template.profile.tone,
            format_style: template.profile.format_style,
            preserve_rules: [...template.profile.preserve_rules],
            legacy_imported: false,
        });
    };

    const updateExample = (index: number, patch: Partial<SceneExample>) => {
        if (!activeProfile) return;
        updateActiveProfile({
            examples: activeProfile.examples.map((example, currentIndex) =>
                currentIndex === index ? { ...example, ...patch } : example
            ),
        });
    };

    const addExample = () => activeProfile && updateActiveProfile({ examples: [...activeProfile.examples, { input: "", output: "" }] });
    const removeExample = (index: number) =>
        activeProfile &&
        updateActiveProfile({ examples: activeProfile.examples.filter((_, currentIndex) => currentIndex !== index) });

    const handleTestLlm = async () => {
        if (!config) return;
        setLlmTesting(true);
        setLlmTestResult(null);
        try {
            const result = await api.testLlmConnection(config.llm_config, config.proxy);
            setLlmTestResult({ success: true, message: result });
        } catch (error: any) {
            setLlmTestResult({ success: false, message: error.toString() });
        } finally {
            setLlmTesting(false);
        }
    };

    const handleSwitchDevice = async (deviceName: string) => {
        if (isTesting) {
            await api.stopAudioTest();
            setIsTesting(false);
            setAudioLevel(0);
        }
        setSwitchingDevice(true);
        try {
            await api.switchInputDevice(deviceName);
            setCurrentDevice(deviceName);
            if (config) updateConfig("input_device", deviceName);
        } finally {
            setSwitchingDevice(false);
        }
    };

    const handleRefreshDevices = async () => {
        setSwitchingDevice(true);
        try {
            setInputDevices(await api.getInputDevices());
        } finally {
            setSwitchingDevice(false);
        }
    };

    const toggleAudioTest = async () => {
        if (isTesting) {
            await api.stopAudioTest();
            setIsTesting(false);
            setAudioLevel(0);
        } else {
            await api.startAudioTest();
            setIsTesting(true);
        }
    };

    const isDeviceSelected = !!(config?.input_device && config.input_device !== "");
    const isAsrConfigured = !!(config?.online_asr_config?.app_key && config?.online_asr_config?.access_key);

    const flushPendingSave = async () => {
        if (!pendingConfigRef.current) return;
        if (saveTimeoutRef.current) {
            clearTimeout(saveTimeoutRef.current);
            saveTimeoutRef.current = null;
        }
        try {
            await api.saveConfig(pendingConfigRef.current);
            pendingConfigRef.current = null;
        } catch (error) {
            console.error("Failed to save config on close:", error);
        }
    };

    const handleClose = async () => {
        if (isFirstSetup && (!isDeviceSelected || !isAsrConfigured)) {
            setShowCloseWarning(true);
            return;
        }
        await flushPendingSave();
        onClose();
    };

    if (!isOpen || !config) return null;

    return (
        <div
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/20 p-4 backdrop-blur-sm animate-in fade-in duration-200"
            onClick={handleClose}
        >
            <div
                className="flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl animate-in zoom-in-95 duration-200"
                onClick={(event) => event.stopPropagation()}
            >
                <div className="flex items-center justify-between border-b border-slate-100 bg-slate-50/50 p-6">
                    <div>
                        <h2 className="text-xl font-bold text-slate-800">{isFirstSetup ? "Welcome! Let's Get Started" : "Settings"}</h2>
                        <div className="mt-1 flex items-center gap-2 text-xs text-slate-500">
                            {isSaving && (
                                <span className="flex items-center gap-1">
                                    <Loader2 className="h-3 w-3 animate-spin" />
                                    Saving...
                                </span>
                            )}
                            {saveSuccess && !isSaving && (
                                <span className="flex items-center gap-1 text-green-600">
                                    <Check className="h-3 w-3" />
                                    Saved
                                </span>
                            )}
                        </div>
                    </div>
                    <button
                        onClick={handleClose}
                        className="rounded-full p-2 text-slate-400 transition-colors hover:bg-slate-200/50 hover:text-slate-600"
                    >
                        <X className="h-5 w-5" />
                    </button>
                </div>

                <div className="custom-scrollbar flex-1 space-y-8 overflow-y-auto p-6">
                    {isFirstSetup && (
                        <div className="rounded-xl border border-chinese-indigo/20 bg-gradient-to-r from-chinese-indigo/10 to-chinese-indigo/5 p-4">
                            <div className="flex items-start gap-3">
                                <Info className="mt-0.5 h-5 w-5 flex-shrink-0 text-chinese-indigo" />
                                <div>
                                    <h3 className="mb-1 font-semibold text-slate-800">Setup Required</h3>
                                    <p className="text-sm text-slate-600">Please complete these steps before using the app:</p>
                                    <div className="mt-2 space-y-1.5 text-sm">
                                        <div className={`flex items-center gap-2 ${isDeviceSelected ? "text-green-600" : "text-slate-600"}`}>
                                            {isDeviceSelected ? <Check className="h-4 w-4" /> : <span className="h-4 w-4 rounded-full border-2 border-current" />}
                                            Select an input device
                                        </div>
                                        <div className={`flex items-center gap-2 ${isAsrConfigured ? "text-green-600" : "text-slate-600"}`}>
                                            {isAsrConfigured ? <Check className="h-4 w-4" /> : <span className="h-4 w-4 rounded-full border-2 border-current" />}
                                            Configure online ASR credentials
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    )}

                    <section>
                        <SectionHeader icon={Keyboard} title="Triggers" />
                        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
                            <TriggerToggle
                                label="Mouse Mode"
                                desc="Middle Click Hold"
                                explanation="Hold middle mouse to talk, release to type."
                                active={config.trigger_mouse}
                                onClick={() => updateConfig("trigger_mouse", !config.trigger_mouse)}
                            />
                            <TriggerToggle
                                label="Hold Mode"
                                desc="Ctrl + Win Hold"
                                explanation="Hold to talk, release to trigger a skill."
                                active={config.trigger_hold}
                                onClick={() => updateConfig("trigger_hold", !config.trigger_hold)}
                            />
                            <TriggerToggle
                                label="Toggle Mode"
                                desc="Right Alt Press"
                                explanation="Press once to start and again to stop."
                                active={config.trigger_toggle}
                                onClick={() => updateConfig("trigger_toggle", !config.trigger_toggle)}
                            />
                        </div>
                    </section>

                    <section>
                        <SectionHeader icon={Zap} title="Skills" />
                        <p className="mb-4 text-xs text-slate-500">Ctrl+Win 模式下，识别文本会按下列关键词匹配并执行对应技能。</p>
                        <div className="space-y-3">
                            {config.skills.map((skill) => (
                                <div key={skill.id} className="rounded-xl border border-slate-200 bg-slate-50 p-4">
                                    <div className="flex items-center gap-4">
                                        <button
                                            onClick={() => updateSkillConfig(skill.id, "enabled", !skill.enabled)}
                                            className={`relative h-5 w-10 flex-shrink-0 rounded-full transition-colors ${skill.enabled ? "bg-chinese-indigo" : "bg-slate-300"}`}
                                        >
                                            <div className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${skill.enabled ? "left-5" : "left-0.5"}`} />
                                        </button>
                                        <div className="w-24 flex-shrink-0">
                                            <div className="text-sm font-medium text-slate-800">{skill.name}</div>
                                        </div>
                                        <input
                                            type="text"
                                            value={skill.keywords}
                                            onChange={(event) => updateSkillConfig(skill.id, "keywords", event.target.value)}
                                            placeholder="关键词，逗号分隔"
                                            className="flex-1 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-chinese-indigo"
                                        />
                                    </div>
                                </div>
                            ))}
                        </div>
                    </section>

                    <section>
                        <SectionHeader icon={Mic} title="Audio" />
                        <div className="space-y-4">
                            <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
                                <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                                    <div className="flex items-center gap-3">
                                        <Mic className="h-5 w-5 text-slate-400" />
                                        <div>
                                            <div className="font-medium text-slate-800">Input Device</div>
                                            <div className="text-xs text-slate-500">Current microphone</div>
                                        </div>
                                    </div>
                                    <div className="flex w-full items-center gap-2 md:w-auto">
                                        {switchingDevice && <Loader2 className="h-4 w-4 animate-spin text-chinese-indigo" />}
                                        <select
                                            value={currentDevice}
                                            onChange={(event) => handleSwitchDevice(event.target.value)}
                                            disabled={switchingDevice || isTesting}
                                            className="flex-1 rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-chinese-indigo disabled:opacity-50 md:w-64"
                                        >
                                            {inputDevices.length === 0 ? (
                                                <option value="">No input devices found</option>
                                            ) : (
                                                inputDevices.map((device) => (
                                                    <option key={device.id} value={device.name}>
                                                        {device.name}
                                                    </option>
                                                ))
                                            )}
                                        </select>
                                        <button
                                            onClick={handleRefreshDevices}
                                            disabled={switchingDevice}
                                            className="rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-600 transition-colors hover:border-chinese-indigo hover:text-chinese-indigo disabled:opacity-50"
                                        >
                                            Refresh
                                        </button>
                                    </div>
                                </div>
                            </div>

                            <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
                                <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                                    <div>
                                        <div className="font-medium text-slate-800">Audio Test</div>
                                        <div className="text-xs text-slate-500">Check whether the selected microphone receives input.</div>
                                    </div>
                                    <button
                                        onClick={toggleAudioTest}
                                        className={`rounded-lg px-4 py-2 text-sm font-medium text-white transition-colors ${isTesting ? "bg-red-500 hover:bg-red-500/90" : "bg-chinese-indigo hover:bg-chinese-indigo/90"}`}
                                    >
                                        {isTesting ? "Stop Test" : "Start Test"}
                                    </button>
                                </div>
                                <div className="mt-4 h-2 overflow-hidden rounded-full bg-slate-200">
                                    <div className="h-full rounded-full bg-chinese-indigo transition-all duration-100" style={{ width: `${audioLevel * 100}%` }} />
                                </div>
                            </div>
                        </div>
                    </section>

                    <section>
                        <SectionHeader icon={Mic} title="Online ASR" />
                        <div className="space-y-3">
                            <Field label="App Key" value={config.online_asr_config.app_key} onChange={(value) => updateConfig("online_asr_config", { ...config.online_asr_config, app_key: value })} placeholder="Your ASR app key" />
                            <Field label="Access Key" value={config.online_asr_config.access_key} onChange={(value) => updateConfig("online_asr_config", { ...config.online_asr_config, access_key: value })} placeholder="Your ASR access key" />
                            <Field label="Resource ID" value={config.online_asr_config.resource_id} onChange={(value) => updateConfig("online_asr_config", { ...config.online_asr_config, resource_id: value })} placeholder="volc.bigasr.sauc.duration" />
                        </div>
                    </section>

                    <section>
                        <SectionHeader icon={Sparkles} title="LLM Scenes" />
                        <div className="space-y-4">
                            <ToggleRow icon={Sparkles} title="Enable LLM Correction" description="Use OpenAI Responses API with strict structured output." active={config.llm_config.enabled} onToggle={() => updateLlmConfig("enabled", !config.llm_config.enabled)} />
                            {config.llm_config.enabled && (
                                <div className="space-y-3">
                                    <Field label="Base URL" value={config.llm_config.base_url} onChange={(value) => updateLlmConfig("base_url", value)} placeholder="https://api.openai.com/v1" />
                                    <Field label="API Key" type="password" value={config.llm_config.api_key} onChange={(value) => updateLlmConfig("api_key", value)} placeholder="sk-..." />
                                    <Field label="Model" value={config.llm_config.model} onChange={(value) => updateLlmConfig("model", value)} placeholder="gpt-4o-mini" />
                                    <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
                                        <div className="mb-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
                                            FastSP 现在要求目标服务支持 `POST /v1/responses` 和 strict JSON schema 输出，不再依赖文本括号提取。
                                        </div>
                                        <button onClick={() => setShowSceneEditor((value) => !value)} className="flex w-full items-center justify-between text-sm font-medium text-slate-700">
                                            <span>Scene Profiles</span>
                                            {showSceneEditor ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
                                        </button>
                                        {showSceneEditor && activeProfile && (
                                            <div className="mt-4 space-y-4">
                                                <div className="flex items-center gap-2">
                                                    <select value={config.llm_config.active_profile_id} onChange={(event) => switchProfile(event.target.value)} className="flex-1 rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-chinese-indigo">
                                                        {config.llm_config.profiles.map((profile) => (
                                                            <option key={profile.id} value={profile.id}>
                                                                {profile.name}
                                                            </option>
                                                        ))}
                                                    </select>
                                                    <button onClick={createProfile} className="rounded-lg border border-slate-200 bg-white p-2 transition-colors hover:border-chinese-indigo hover:text-chinese-indigo" title="新建场景">
                                                        <Plus className="h-4 w-4" />
                                                    </button>
                                                    <button onClick={deleteActiveProfile} disabled={config.llm_config.profiles.length <= 1} className="rounded-lg border border-slate-200 bg-white p-2 transition-colors hover:border-red-500 hover:text-red-500 disabled:cursor-not-allowed disabled:opacity-30" title="删除当前场景">
                                                        <Trash2 className="h-4 w-4" />
                                                    </button>
                                                    <button onClick={resetActiveProfile} className="rounded-lg border border-slate-200 bg-white p-2 transition-colors hover:border-amber-500 hover:text-amber-500" title="重置当前场景">
                                                        <RotateCcw className="h-4 w-4" />
                                                    </button>
                                                </div>

                                                <div className="flex flex-wrap items-center gap-2">
                                                    <span className="text-xs font-medium uppercase tracking-wide text-slate-500">模板</span>
                                                    {SCENE_TEMPLATES.map((template) => (
                                                        <button key={template.key} onClick={() => applySceneTemplate(template.key)} className={`rounded-full px-3 py-1 text-xs transition-colors ${activeProfile.task_kind === template.key ? "bg-chinese-indigo text-white" : "bg-white text-slate-600 hover:bg-slate-100"}`} title={template.description}>
                                                            {template.label}
                                                        </button>
                                                    ))}
                                                </div>

                                                <div className="rounded-xl border border-slate-200 bg-white p-4">
                                                    <div className="flex flex-wrap items-center gap-2">
                                                        <span className="text-sm font-medium text-slate-800">{activeProfile.name}</span>
                                                        {activeProfile.legacy_imported && <span className="rounded-full bg-amber-100 px-2 py-0.5 text-[11px] font-medium text-amber-700">Legacy Imported</span>}
                                                    </div>
                                                    <p className="mt-1 text-xs text-slate-500">最终只会上屏 `final_text`。Scene 字段用于生成受控的 developer/user 指令与 JSON schema。</p>
                                                </div>

                                                <Field label="Scene Name" value={activeProfile.name} onChange={(value) => updateActiveProfile({ name: value })} placeholder="场景名称" />
                                                <SelectField
                                                    label="Task Kind"
                                                    value={activeProfile.task_kind}
                                                    options={[
                                                        { value: "plain_correction", label: "plain_correction" },
                                                        { value: "email", label: "email" },
                                                        { value: "meeting_notes", label: "meeting_notes" },
                                                        { value: "customer_service", label: "customer_service" },
                                                        { value: "custom_transform", label: "custom_transform" },
                                                    ]}
                                                    onChange={(value) => applySceneTemplate(value as SceneTaskKind)}
                                                />
                                                <TextareaField label="Goal" value={activeProfile.goal} onChange={(value) => updateActiveProfile({ goal: value })} rows={3} placeholder="这个场景希望模型完成什么任务" />
                                                <Field label="Tone" value={activeProfile.tone} onChange={(value) => updateActiveProfile({ tone: value })} placeholder="例如：专业、友好、克制" />
                                                <TextareaField label="Format Style" value={activeProfile.format_style} onChange={(value) => updateActiveProfile({ format_style: value })} rows={3} placeholder="例如：单段文本、邮件正文、项目符号纪要" />
                                                <TextareaField label="Preserve Rules" value={toMultiline(activeProfile.preserve_rules)} onChange={(value) => updateActiveProfile({ preserve_rules: fromMultiline(value) })} rows={4} placeholder="每行一条，例如：保留所有专有名词" />
                                                <TextareaField label="Glossary" value={toMultiline(activeProfile.glossary)} onChange={(value) => updateActiveProfile({ glossary: fromMultiline(value) })} rows={4} placeholder="每行一条术语，例如：SenseVoice => SenseVoice" />

                                                <div className="rounded-xl border border-slate-200 bg-white p-4">
                                                    <div className="mb-3 flex items-center justify-between">
                                                        <div>
                                                            <div className="text-sm font-medium text-slate-800">Examples</div>
                                                            <div className="text-xs text-slate-500">Few-shot 示例用于稳定场景输出。</div>
                                                        </div>
                                                        <button onClick={addExample} className="rounded-lg border border-slate-200 px-3 py-1.5 text-xs text-slate-600 transition-colors hover:border-chinese-indigo hover:text-chinese-indigo">
                                                            Add Example
                                                        </button>
                                                    </div>
                                                    <div className="space-y-3">
                                                        {activeProfile.examples.length === 0 && <div className="rounded-lg bg-slate-50 px-3 py-2 text-xs text-slate-500">暂无示例。可选填，用于提高特定场景稳定性。</div>}
                                                        {activeProfile.examples.map((example, index) => (
                                                            <div key={`${activeProfile.id}-${index}`} className="rounded-lg border border-slate-200 bg-slate-50 p-3">
                                                                <div className="mb-2 flex items-center justify-between">
                                                                    <span className="text-xs font-medium uppercase tracking-wide text-slate-500">Example {index + 1}</span>
                                                                    <button onClick={() => removeExample(index)} className="text-xs text-red-500 transition-colors hover:text-red-600">
                                                                        Remove
                                                                    </button>
                                                                </div>
                                                                <TextareaField label="Input" value={example.input} onChange={(value) => updateExample(index, { input: value })} rows={3} placeholder="原始口述 / 识别文本" />
                                                                <div className="mt-3" />
                                                                <TextareaField label="Output" value={example.output} onChange={(value) => updateExample(index, { output: value })} rows={3} placeholder="期望输出" />
                                                            </div>
                                                        ))}
                                                    </div>
                                                </div>

                                                <ToggleRow icon={Sparkles} title="Expert Mode" description="仅开放附加说明，不允许修改底层 developer contract 或 JSON schema。" active={activeProfile.expert_mode} onToggle={() => updateActiveProfile({ expert_mode: !activeProfile.expert_mode })} />
                                                {activeProfile.expert_mode && <TextareaField label="Advanced Instruction" value={activeProfile.advanced_instruction} onChange={(value) => updateActiveProfile({ advanced_instruction: value })} rows={6} placeholder="附加说明。这里会作为受控场景配置的一部分发送给模型。" />}
                                            </div>
                                        )}
                                    </div>

                                    <div className="flex flex-wrap items-center gap-3">
                                        <button onClick={handleTestLlm} disabled={llmTesting || !config.llm_config.api_key} className="flex items-center gap-2 rounded-lg bg-chinese-indigo px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-chinese-indigo/90 disabled:cursor-not-allowed disabled:opacity-50">
                                            {llmTesting && <Loader2 className="h-4 w-4 animate-spin" />}
                                            {llmTesting ? "Testing..." : "Test Structured Connection"}
                                        </button>
                                        {llmTestResult && <span className={`text-sm ${llmTestResult.success ? "text-green-600" : "text-red-600"}`}>{llmTestResult.message}</span>}
                                    </div>
                                </div>
                            )}
                        </div>
                    </section>

                    <section>
                        <SectionHeader icon={Globe} title="Network" />
                        <div className="space-y-4">
                            <ToggleRow icon={Globe} title="Enable Proxy" description="Used for both ASR and LLM requests." active={config.proxy.enabled} onToggle={() => updateProxyConfig("enabled", !config.proxy.enabled)} />
                            {config.proxy.enabled && <Field label="Proxy URL" value={config.proxy.url} onChange={(value) => updateProxyConfig("url", value)} placeholder="http://127.0.0.1:7890 or socks5://127.0.0.1:1080" hint="Supports HTTP and SOCKS5 proxies." />}
                        </div>
                    </section>
                </div>
            </div>

            {showCloseWarning && (
                <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30 p-4 backdrop-blur-sm animate-in fade-in duration-150" onClick={(event) => { event.stopPropagation(); setShowCloseWarning(false); }}>
                    <div className="w-full max-w-sm rounded-xl bg-white p-6 shadow-2xl animate-in zoom-in-95 duration-150" onClick={(event) => event.stopPropagation()}>
                        <div className="mb-4 flex items-center gap-3">
                            <div className="flex h-10 w-10 items-center justify-center rounded-full bg-amber-100">
                                <AlertCircle className="h-5 w-5 text-amber-600" />
                            </div>
                            <h3 className="font-semibold text-slate-800">Setup Incomplete</h3>
                        </div>
                        <p className="mb-4 text-sm text-slate-600">
                            {!isDeviceSelected && !isAsrConfigured
                                ? "Please select an input device and configure your online ASR credentials to use the app."
                                : !isDeviceSelected
                                    ? "Please select an input device to use the app."
                                    : "Please configure your online ASR credentials to use the app."}
                        </p>
                        <div className="flex justify-end gap-3">
                            <button onClick={() => { setShowCloseWarning(false); onClose(); }} className="rounded-lg px-4 py-2 text-sm text-slate-600 transition-colors hover:bg-slate-100">
                                Close Anyway
                            </button>
                            <button onClick={() => setShowCloseWarning(false)} className="rounded-lg bg-chinese-indigo px-4 py-2 text-sm text-white transition-colors hover:bg-chinese-indigo/90">
                                Continue Setup
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );

}

function SectionHeader({ icon: Icon, title }: { icon: any; title: string }) {
    return (
        <div className="mb-4 flex items-center gap-2 text-chinese-indigo">
            <Icon className="h-4 w-4" />
            <h3 className="text-sm font-bold uppercase tracking-wider">{title}</h3>
        </div>
    );
}

function TriggerToggle({
    label,
    desc,
    explanation,
    active,
    onClick,
}: {
    label: string;
    desc: string;
    explanation: string;
    active: boolean;
    onClick: () => void;
}) {
    return (
        <button onClick={onClick} className={`rounded-xl p-4 text-left transition-all duration-200 ${active ? "bg-chinese-indigo/50 text-white shadow-lg shadow-chinese-indigo/25" : "bg-white hover:bg-slate-200"}`}>
            <div className="mb-2 flex w-full items-center justify-between">
                <div className={`h-2 w-2 rounded-full ${active ? "bg-white shadow-[0_0_8px_rgba(255,255,255,0.6)]" : "bg-slate-300"}`} />
                {active && <Check className="h-3 w-3 text-white" />}
            </div>
            <div className={`font-semibold ${active ? "text-white" : "text-slate-800"}`}>{label}</div>
            <div className={`mt-1 text-xs ${active ? "text-white/80" : "text-slate-400"}`}>{desc}</div>
            <div className={`mt-2 text-[10px] leading-snug ${active ? "text-white/70" : "text-slate-500"}`}>{explanation}</div>
        </button>
    );
}

function ToggleRow({
    icon: Icon,
    title,
    description,
    active,
    onToggle,
}: {
    icon: any;
    title: string;
    description: string;
    active: boolean;
    onToggle: () => void;
}) {
    return (
        <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
            <div className="flex items-center justify-between gap-4">
                <div className="flex items-center gap-3">
                    <Icon className="h-5 w-5 text-slate-400" />
                    <div>
                        <div className="font-medium text-slate-800">{title}</div>
                        <div className="text-xs text-slate-500">{description}</div>
                    </div>
                </div>
                <button onClick={onToggle} className={`relative h-6 w-12 rounded-full transition-colors ${active ? "bg-chinese-indigo" : "bg-slate-300"}`}>
                    <div className={`absolute top-1 h-4 w-4 rounded-full bg-white shadow transition-all ${active ? "left-7" : "left-1"}`} />
                </button>
            </div>
        </div>
    );
}

function Field({
    label,
    value,
    onChange,
    placeholder,
    type = "text",
    hint,
}: {
    label: string;
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    type?: string;
    hint?: string;
}) {
    return (
        <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
            <label className="mb-2 block text-sm font-medium text-slate-700">{label}</label>
            <input type={type} value={value} onChange={(event) => onChange(event.target.value)} placeholder={placeholder} className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-chinese-indigo" />
            {hint && <p className="mt-2 text-xs text-slate-400">{hint}</p>}
        </div>
    );
}

function TextareaField({
    label,
    value,
    onChange,
    rows,
    placeholder,
}: {
    label: string;
    value: string;
    onChange: (value: string) => void;
    rows: number;
    placeholder?: string;
}) {
    return (
        <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
            <label className="mb-2 block text-sm font-medium text-slate-700">{label}</label>
            <textarea value={value} onChange={(event) => onChange(event.target.value)} rows={rows} placeholder={placeholder} className="w-full resize-none rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-chinese-indigo" />
        </div>
    );
}

function SelectField({
    label,
    value,
    options,
    onChange,
}: {
    label: string;
    value: string;
    options: Array<{ value: string; label: string }>;
    onChange: (value: string) => void;
}) {
    return (
        <div className="rounded-xl border border-slate-200 bg-slate-50 p-4">
            <label className="mb-2 block text-sm font-medium text-slate-700">{label}</label>
            <select value={value} onChange={(event) => onChange(event.target.value)} className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-chinese-indigo">
                {options.map((option) => (
                    <option key={option.value} value={option.value}>
                        {option.label}
                    </option>
                ))}
            </select>
        </div>
    );
}
