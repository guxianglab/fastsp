import { Settings2 } from "lucide-react";

interface StatusSectionProps {
  isRecording: boolean;
  asrConfigured: boolean;
  onSettingsClick: () => void;
}

export function StatusSection({ isRecording, asrConfigured, onSettingsClick }: StatusSectionProps) {
  const statusText = isRecording ? "录音中" : asrConfigured ? "待机中" : "需要完成设置";
  const helperText = isRecording ? "松开后自动转写" : asrConfigured ? "按快捷键开始说话" : "先配置麦克风和识别服务";

  return (
    <section className="flex items-center justify-between py-4">
      <div className="flex items-center gap-4">
        {/* Status indicator dot */}
        <div className="relative flex h-3 w-3 items-center justify-center">
          {isRecording ? (
            <>
              <span className="absolute inset-0 animate-ping rounded-full bg-chinese-indigo opacity-75" />
              <span className="relative h-2 w-2 rounded-full bg-chinese-indigo" />
            </>
          ) : asrConfigured ? (
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
          ) : (
            <span className="h-2 w-2 rounded-full bg-neutral-300" />
          )}
        </div>

        {/* Status text */}
        <div>
          <div className="text-base font-medium text-neutral-900">{statusText}</div>
          <div className="text-sm text-neutral-500">{helperText}</div>
        </div>
      </div>

      {/* Settings button */}
      <button
        onClick={onSettingsClick}
        className="inline-flex h-8 w-8 items-center justify-center rounded-lg text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-600"
      >
        <Settings2 className="h-4 w-4" />
      </button>
    </section>
  );
}
