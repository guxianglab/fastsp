import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Copy, Loader2, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { api, AppConfig, AudioDevice, LlmConfig, PromptProfile, ProxyConfig, SkillConfig, events } from "../lib/api";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  isFirstSetup?: boolean;
}

type SettingsTab = "audio" | "recognition" | "polish" | "skills";

const TABS: Array<{ key: SettingsTab; label: string }> = [
  { key: "audio", label: "录音" },
  { key: "recognition", label: "识别" },
  { key: "polish", label: "润色" },
  { key: "skills", label: "技能" },
];

const toLines = (items: string[]) => items.join("\n");
const fromLines = (value: string) =>
  value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);

const toInlineList = (items: string[]) => items.join(" / ");
const fromInlineList = (value: string) =>
  value
    .split(/[\n,，/、]+/)
    .map((item) => item.trim())
    .filter(Boolean);

const cloneProfile = (profile: PromptProfile, patch: Partial<PromptProfile> = {}): PromptProfile => ({
  ...profile,
  voice_aliases: [...profile.voice_aliases],
  preserve_rules: [...profile.preserve_rules],
  glossary: [...profile.glossary],
  examples: profile.examples.map((example) => ({ ...example })),
  ...patch,
});

const makeProfileId = () => `profile_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;

const mergeDefaultProfiles = (profiles: PromptProfile[], defaults: PromptProfile[]) => {
  const existingIds = new Set(profiles.map((profile) => profile.id));
  const restored = defaults
    .filter((profile) => !existingIds.has(profile.id))
    .map((profile) => cloneProfile(profile));
  return [...profiles, ...restored];
};

export function SettingsModal({ isOpen, onClose, isFirstSetup = false }: SettingsModalProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [currentDevice, setCurrentDevice] = useState("");
  const [defaultProfiles, setDefaultProfiles] = useState<PromptProfile[]>([]);
  const [blankProfile, setBlankProfile] = useState<PromptProfile | null>(null);
  const [audioLevel, setAudioLevel] = useState(0);
  const [testingAudio, setTestingAudio] = useState(false);
  const [testingLlm, setTestingLlm] = useState(false);
  const [llmResult, setLlmResult] = useState<{ success: boolean; message: string } | null>(null);
  const [showWarning, setShowWarning] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [saveOk, setSaveOk] = useState(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>("audio");
  const timerRef = useRef<number | null>(null);
  const pendingRef = useRef<AppConfig | null>(null);

  const saveLater = useCallback((next: AppConfig) => {
    pendingRef.current = next;
    if (timerRef.current) window.clearTimeout(timerRef.current);
    setIsSaving(true);
    timerRef.current = window.setTimeout(async () => {
      const finalConfig = pendingRef.current;
      if (finalConfig) {
        try {
          await api.saveConfig(finalConfig);
          setSaveOk(true);
          window.setTimeout(() => setSaveOk(false), 1500);
        } catch (error) {
          console.error("save config failed", error);
        }
      }
      pendingRef.current = null;
      setIsSaving(false);
    }, 300);
  }, []);

  useEffect(() => {
    return () => {
      if (timerRef.current) window.clearTimeout(timerRef.current);
    };
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    api.getConfig().then(setConfig);
    api.getInputDevices().then(setDevices);
    api.getCurrentInputDevice().then(setCurrentDevice);
    api.getDefaultSceneProfiles().then(setDefaultProfiles);
    api.getDefaultSceneTemplate().then(setBlankProfile);
    setActiveTab("audio");
    setShowWarning(false);
    setLlmResult(null);
    const interval = window.setInterval(() => api.getInputDevices().then(setDevices), 3000);
    return () => window.clearInterval(interval);
  }, [isOpen]);

  useEffect(() => {
    if (!testingAudio) return;
    const unsub = events.onAudioLevel((level) => setAudioLevel(Math.min(1, level * 5)));
    return () => {
      unsub.then((fn) => fn());
    };
  }, [testingAudio]);

  useEffect(() => {
    if (!isOpen && testingAudio) {
      api.stopAudioTest();
      setTestingAudio(false);
      setAudioLevel(0);
    }
  }, [isOpen, testingAudio]);

  useEffect(() => {
    if (!isOpen) return;
    const unsub = events.onConfigUpdated((nextConfig) => {
      pendingRef.current = null;
      setIsSaving(false);
      setConfig(nextConfig);
      setLlmResult(null);
    });
    return () => {
      unsub.then((fn) => fn());
    };
  }, [isOpen]);

  if (!isOpen || !config) return null;

  const updateConfig = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    const next = { ...config, [key]: value };
    setConfig(next);
    saveLater(next);
  };

  const updateLlm = <K extends keyof LlmConfig>(key: K, value: LlmConfig[K]) => {
    const next = { ...config, llm_config: { ...config.llm_config, [key]: value } };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const updateProxy = <K extends keyof ProxyConfig>(key: K, value: ProxyConfig[K]) => {
    updateConfig("proxy", { ...config.proxy, [key]: value });
  };

  const updateSkill = <K extends keyof SkillConfig>(id: string, key: K, value: SkillConfig[K]) => {
    updateConfig(
      "skills",
      config.skills.map((skill) => (skill.id === id ? { ...skill, [key]: value } : skill)),
    );
  };

  const active =
    config.llm_config.profiles.find((profile) => profile.id === config.llm_config.active_profile_id) ??
    config.llm_config.profiles[0] ??
    null;

  const activeDefaultProfile =
    defaultProfiles.find((profile) => profile.id === active?.id) ??
    defaultProfiles.find((profile) => profile.preset_key === active?.preset_key) ??
    null;

  const updateActive = (patch: Partial<PromptProfile>) => {
    if (!active) return;
    updateLlm(
      "profiles",
      config.llm_config.profiles.map((profile) =>
        profile.id === active.id ? cloneProfile(profile, patch) : profile,
      ),
    );
  };

  const selectScene = (id: string) => {
    updateLlm("active_profile_id", id);
  };

  const createProfile = () => {
    const base = blankProfile ?? active ?? defaultProfiles[0];
    if (!base) return;

    const id = makeProfileId();
    const profile = cloneProfile(base, {
      id,
      name: `场景 ${config.llm_config.profiles.length + 1}`,
      voice_aliases: [],
      advanced_instruction: "",
      expert_mode: false,
      legacy_imported: false,
    });
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles: [...config.llm_config.profiles, profile],
        active_profile_id: id,
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const copyProfile = () => {
    if (!active) return;
    const id = makeProfileId();
    const copied = cloneProfile(active, {
      id,
      name: `${active.name} 副本`,
    });
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles: [...config.llm_config.profiles, copied],
        active_profile_id: id,
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const deleteProfile = () => {
    if (!active || config.llm_config.profiles.length <= 1) return;
    const profiles = config.llm_config.profiles.filter((profile) => profile.id !== active.id);
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles,
        active_profile_id: profiles[0].id,
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const resetProfile = () => {
    if (!active) return;
    const base = activeDefaultProfile ?? blankProfile;
    if (!base) return;

    const keepLabel = !defaultProfiles.some((profile) => profile.id === active.id);
    updateActive({
      name: keepLabel ? active.name : base.name,
      voice_aliases: keepLabel ? [...active.voice_aliases] : [...base.voice_aliases],
      preset_key: base.preset_key,
      goal: base.goal,
      tone: base.tone,
      format_style: base.format_style,
      preserve_rules: [...base.preserve_rules],
      glossary: [],
      examples: [],
      advanced_instruction: "",
      expert_mode: false,
      legacy_imported: false,
    });
  };

  const restoreDefaults = () => {
    if (defaultProfiles.length === 0) return;
    const profiles = mergeDefaultProfiles(config.llm_config.profiles, defaultProfiles);
    const next = {
      ...config,
      llm_config: {
        ...config.llm_config,
        profiles,
        active_profile_id: config.llm_config.active_profile_id || profiles[0]?.id || "",
      },
    };
    setConfig(next);
    saveLater(next);
    setLlmResult(null);
  };

  const testLlm = async () => {
    setTestingLlm(true);
    setLlmResult(null);
    try {
      const message = await api.testLlmConnection(config.llm_config, config.proxy);
      setLlmResult({ success: true, message });
    } catch (error) {
      setLlmResult({ success: false, message: String(error) });
    } finally {
      setTestingLlm(false);
    }
  };

  const switchDevice = async (deviceId: string) => {
    await api.switchInputDevice(deviceId);
    setCurrentDevice(deviceId);
    updateConfig("input_device", deviceId);
  };

  const toggleAudio = async () => {
    if (testingAudio) {
      await api.stopAudioTest();
      setTestingAudio(false);
      setAudioLevel(0);
    } else {
      await api.startAudioTest();
      setTestingAudio(true);
    }
  };

  const flush = async () => {
    if (!pendingRef.current) return;
    if (timerRef.current) window.clearTimeout(timerRef.current);
    await api.saveConfig(pendingRef.current);
    pendingRef.current = null;
    setIsSaving(false);
  };

  const close = async () => {
    const ready = !!config.input_device && !!config.online_asr_config.app_key && !!config.online_asr_config.access_key;
    if (isFirstSetup && !ready) {
      setShowWarning(true);
      return;
    }
    await flush();
    onClose();
  };

  const saveText = isSaving ? "正在保存..." : saveOk ? "已保存" : "自动保存";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-neutral-950/20 p-4 backdrop-blur-sm" onClick={close}>
      <div
        className="flex max-h-[88vh] w-full max-w-5xl flex-col overflow-hidden bg-neutral-50 shadow-[0_4px_24px_rgba(0,0,0,0.06)]"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-neutral-200 px-6 py-5">
          <div>
            <h2 className="text-xl font-semibold text-neutral-900">{isFirstSetup ? "完成设置" : "设置"}</h2>
            <div className="mt-1 text-sm text-neutral-500">
              {isFirstSetup ? "先配置麦克风和识别服务" : saveText}
            </div>
          </div>
          <button
            onClick={close}
            className="inline-flex h-10 w-10 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col md:flex-row">
          <aside className="border-b border-neutral-200 px-3 py-4 md:w-[100px] md:border-b-0 md:border-r">
            <nav className="flex gap-2 md:flex-col">
              {TABS.map((tab) => (
                <button
                  key={tab.key}
                  onClick={() => setActiveTab(tab.key)}
                  className={`flex items-center gap-2 px-2 py-2.5 text-left text-sm transition-colors ${
                    activeTab === tab.key
                      ? "text-neutral-900"
                      : "text-neutral-500 hover:text-neutral-700"
                  }`}
                >
                  <span
                    className={`h-1.5 w-1.5 rounded-full ${
                      activeTab === tab.key ? "bg-neutral-900" : "border border-neutral-300"
                    }`}
                  />
                  <span className={activeTab === tab.key ? "font-medium" : ""}>{tab.label}</span>
                </button>
              ))}
            </nav>
          </aside>

          <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto px-6 py-6">
            {activeTab === "audio" && (
              <div className="space-y-5">
                <Section title="触发方式">
                  <div className="space-y-3">
                    <ToggleRow
                      title="鼠标中键"
                      desc="按住说话，松开转写"
                      active={config.trigger_mouse}
                      onToggle={() => updateConfig("trigger_mouse", !config.trigger_mouse)}
                    />
                    <ToggleRow
                      title="右 Alt"
                      desc="按一次开始，再按一次结束"
                      active={config.trigger_hold}
                      onToggle={() => updateConfig("trigger_hold", !config.trigger_hold)}
                    />
                    <ToggleRow
                      title="Ctrl + Win"
                      desc="只执行技能，不粘贴文本"
                      active={config.trigger_toggle}
                      onToggle={() => updateConfig("trigger_toggle", !config.trigger_toggle)}
                    />
                  </div>
                </Section>

                <Section title="输入设备">
                  <Surface>
                    <label className="mb-2 block text-xs text-neutral-400">设备</label>
                    <div className="flex flex-col gap-3 lg:flex-row">
                      <select
                        value={currentDevice}
                        onChange={(event) => switchDevice(event.target.value)}
                        className="input-underline w-full py-2 text-neutral-900"
                      >
                        <option value="">默认设备</option>
                        {devices.map((device) => (
                          <option key={device.id} value={device.id}>
                            {device.name}
                            {device.is_default ? "（默认）" : ""}
                          </option>
                        ))}
                      </select>
                      <div className="flex gap-3">
                        <ActionButton onClick={() => api.getInputDevices().then(setDevices)}>刷新设备</ActionButton>
                        <PrimaryButton onClick={toggleAudio}>
                          {testingAudio ? "停止测试" : "测试麦克风"}
                        </PrimaryButton>
                      </div>
                    </div>
                    <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-neutral-200">
                      <div
                        className="h-full rounded-full bg-chinese-indigo transition-all"
                        style={{ width: `${audioLevel * 100}%` }}
                      />
                    </div>
                  </Surface>
                </Section>
              </div>
            )}

            {activeTab === "recognition" && (
              <div className="space-y-5">
                <Section title="在线识别">
                  <div className="grid gap-3 md:grid-cols-2">
                    <Field
                      label="应用密钥"
                      value={config.online_asr_config.app_key}
                      onChange={(value) =>
                        updateConfig("online_asr_config", { ...config.online_asr_config, app_key: value })
                      }
                    />
                    <Field
                      label="访问密钥"
                      value={config.online_asr_config.access_key}
                      onChange={(value) =>
                        updateConfig("online_asr_config", { ...config.online_asr_config, access_key: value })
                      }
                    />
                    <Field
                      label="资源 ID"
                      value={config.online_asr_config.resource_id}
                      onChange={(value) =>
                        updateConfig("online_asr_config", { ...config.online_asr_config, resource_id: value })
                      }
                    />
                  </div>
                </Section>

                <Section title="网络代理">
                  <ToggleRow
                    title="启用代理"
                    desc="识别和润色共用同一个代理"
                    active={config.proxy.enabled}
                    onToggle={() => updateProxy("enabled", !config.proxy.enabled)}
                  />
                  {config.proxy.enabled && (
                    <div className="mt-3">
                      <Field label="代理地址" value={config.proxy.url} onChange={(value) => updateProxy("url", value)} />
                    </div>
                  )}
                </Section>
              </div>
            )}

            {activeTab === "polish" && (
              <div className="space-y-5">
                <Section title="启用功能">
                  <ToggleRow
                    title="启用润色"
                    desc="只在听写模式下生效"
                    active={config.llm_config.enabled}
                    onToggle={() => updateLlm("enabled", !config.llm_config.enabled)}
                  />
                </Section>

                <Section title="服务配置">
                  <div className="space-y-3">
                    <div className="grid gap-3 md:grid-cols-2">
                      <Field label="服务地址" value={config.llm_config.base_url} onChange={(value) => updateLlm("base_url", value)} />
                      <Field label="模型" value={config.llm_config.model} onChange={(value) => updateLlm("model", value)} />
                    </div>
                    <Field
                      label="接口密钥"
                      type="password"
                      value={config.llm_config.api_key}
                      onChange={(value) => updateLlm("api_key", value)}
                    />
                    <div className="flex flex-wrap items-center gap-3">
                      <PrimaryButton onClick={testLlm} disabled={testingLlm}>
                        {testingLlm && <Loader2 className="h-4 w-4 animate-spin" />}
                        {testingLlm ? "测试中..." : "测试连接"}
                      </PrimaryButton>
                      {llmResult && (
                        <span className={`text-sm ${llmResult.success ? "text-emerald-600" : "text-red-600"}`}>
                          {llmResult.message}
                        </span>
                      )}
                    </div>
                  </div>
                </Section>

                <Section title="场景设置">
                  {active && (
                    <div className="space-y-4">
                      <Surface>
                        <div className="space-y-4">
                          <div className="flex flex-wrap gap-2">
                            {config.llm_config.profiles.map((profile) => (
                              <button
                                key={profile.id}
                                onClick={() => selectScene(profile.id)}
                                className={`px-3 py-1.5 text-sm transition-colors ${
                                  config.llm_config.active_profile_id === profile.id
                                    ? "bg-neutral-900 text-neutral-50"
                                    : "bg-neutral-100 text-neutral-500 hover:bg-neutral-200 hover:text-neutral-700"
                                }`}
                              >
                                {profile.name}
                              </button>
                            ))}
                          </div>

                          <div className="flex flex-wrap items-center gap-2">
                            <MiniButton onClick={createProfile} icon={<Plus className="h-4 w-4" />} title="新增场景" />
                            <MiniButton onClick={copyProfile} icon={<Copy className="h-4 w-4" />} title="复制当前场景" />
                            <MiniButton onClick={deleteProfile} icon={<Trash2 className="h-4 w-4" />} disabled={config.llm_config.profiles.length <= 1} title="删除当前场景" />
                            <MiniButton onClick={resetProfile} icon={<RotateCcw className="h-4 w-4" />} title="重置当前场景" />
                            <ActionButton onClick={restoreDefaults}>恢复默认场景</ActionButton>
                          </div>
                        </div>
                      </Surface>

                      <div className="flex items-center gap-2 text-xs text-neutral-400">
                        <span>{active.id}</span>
                        <span className="h-1 w-1 rounded-full bg-neutral-300" />
                        <span>{activeDefaultProfile ? "内置场景" : "自定义场景"}</span>
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <Field label="场景名称" value={active.name} onChange={(value) => updateActive({ name: value })} />
                        <Field
                          label="语音别名"
                          value={toInlineList(active.voice_aliases)}
                          onChange={(value) => updateActive({ voice_aliases: fromInlineList(value) })}
                        />
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <Area label="目标" value={active.goal} onChange={(value) => updateActive({ goal: value })} />
                        <Field label="语气" value={active.tone} onChange={(value) => updateActive({ tone: value })} />
                      </div>

                      <Area
                        label="输出格式"
                        value={active.format_style}
                        onChange={(value) => updateActive({ format_style: value })}
                      />

                      <div className="grid gap-3 lg:grid-cols-2">
                        <Area
                          label="保留规则"
                          value={toLines(active.preserve_rules)}
                          onChange={(value) => updateActive({ preserve_rules: fromLines(value) })}
                        />
                        <Area
                          label="术语表"
                          value={toLines(active.glossary)}
                          onChange={(value) => updateActive({ glossary: fromLines(value) })}
                        />
                      </div>

                      <ToggleRow
                        title="高级模式"
                        desc="追加更细的补充指令"
                        active={active.expert_mode}
                        onToggle={() => updateActive({ expert_mode: !active.expert_mode })}
                      />

                      {active.expert_mode && (
                        <Area
                          label="高级指令"
                          value={active.advanced_instruction}
                          onChange={(value) => updateActive({ advanced_instruction: value })}
                          rows={5}
                          placeholder="补充额外规则、语气要求、结构偏好或禁止改写项"
                        />
                      )}
                    </div>
                  )}
                </Section>
              </div>
            )}

            {activeTab === "skills" && (
              <div className="space-y-5">
                <Section title="技能列表">
                  <div className="space-y-3">
                    {config.skills.map((skill) => (
                      <SkillCard
                        key={skill.id}
                        name={skill.name}
                        keywords={skill.keywords}
                        enabled={skill.enabled}
                        onToggle={() => updateSkill(skill.id, "enabled", !skill.enabled)}
                        onKeywordsChange={(value) => updateSkill(skill.id, "keywords", value)}
                      />
                    ))}
                  </div>
                </Section>
              </div>
            )}
          </div>
        </div>
      </div>

      {showWarning && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-neutral-950/25 p-4 backdrop-blur-sm">
          <div
            className="w-full max-w-sm bg-neutral-50 p-6 shadow-[0_4px_24px_rgba(0,0,0,0.06)]"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center bg-amber-100 text-amber-600">
                <AlertCircle className="h-5 w-5" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-neutral-900">还没有完成设置</h3>
                <p className="mt-1 text-sm text-neutral-500">请先选择输入设备并填写识别凭证。</p>
              </div>
            </div>
            <div className="mt-6 flex justify-end">
              <PrimaryButton onClick={() => setShowWarning(false)}>继续设置</PrimaryButton>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-4">
      <div className="border-b border-neutral-200 pb-2 text-xs font-medium uppercase tracking-wider text-neutral-400">{title}</div>
      <div className="space-y-4">{children}</div>
    </section>
  );
}

function Surface({ children }: { children: ReactNode }) {
  return <div className="py-2">{children}</div>;
}

function ToggleRow({
  title,
  desc,
  active,
  onToggle,
}: {
  title: string;
  desc: string;
  active: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      onClick={onToggle}
      className="group flex w-full items-start justify-between gap-4 py-2 text-left"
    >
      <div>
        <div className="text-sm text-neutral-900">{title}</div>
        <div className="mt-0.5 text-xs text-neutral-400">{desc}</div>
      </div>
      <div className="relative mt-1 h-4 w-7 flex-shrink-0">
        <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
        <div
          className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
            active ? "w-6" : "w-0"
          }`}
        />
        <div
          className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
            active
              ? "left-3.5 bg-neutral-900"
              : "left-0 border-2 border-neutral-300 bg-neutral-50"
          }`}
        />
      </div>
    </button>
  );
}

function MiniButton({
  onClick,
  icon,
  disabled = false,
  title,
}: {
  onClick: () => void;
  icon: ReactNode;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="inline-flex h-8 w-8 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {icon}
    </button>
  );
}

function Field({
  label,
  value,
  onChange,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <input
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="input-underline w-full py-2 text-sm text-neutral-900"
      />
    </div>
  );
}

function Area({
  label,
  value,
  onChange,
  rows = 3,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  rows?: number;
  placeholder?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <textarea
        rows={rows}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        className="w-full resize-none bg-neutral-50 px-3 py-2 text-sm text-neutral-900 outline-none transition placeholder:text-neutral-300 focus:bg-white focus:shadow-[inset_0_0_0_1px_#d4d4d4]"
      />
    </div>
  );
}

function ActionButton({
  children,
  onClick,
}: {
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900"
    >
      {children}
    </button>
  );
}

function PrimaryButton({
  children,
  onClick,
  disabled = false,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="inline-flex items-center justify-center gap-2 bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function SkillCard({
  name,
  keywords,
  enabled,
  onToggle,
  onKeywordsChange,
}: {
  name: string;
  keywords: string;
  enabled: boolean;
  onToggle: () => void;
  onKeywordsChange: (value: string) => void;
}) {
  return (
    <div className="border-b border-neutral-200 py-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="text-sm text-neutral-900">{name}</div>
          <div className="mt-0.5 text-xs text-neutral-400">命中后执行，不粘贴文本</div>
        </div>
        <button
          onClick={onToggle}
          className="relative mt-1 h-4 w-7 flex-shrink-0"
        >
          <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
          <div
            className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
              enabled ? "w-6" : "w-0"
            }`}
          />
          <div
            className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
              enabled
                ? "left-3.5 bg-neutral-900"
                : "left-0 border-2 border-neutral-300 bg-neutral-50"
            }`}
          />
        </button>
      </div>
      <div className="mt-3">
        <label className="mb-2 block text-sm font-medium text-neutral-600">关键词</label>
        <input
          value={keywords}
          onChange={(event) => onKeywordsChange(event.target.value)}
          className="input-underline w-full py-2 text-neutral-900"
        />
      </div>
    </div>
  );
}
