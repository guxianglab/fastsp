import { Mic, Loader2 } from "lucide-react";

interface StatusSectionProps {
    isRecording: boolean;
    asrConfigured: boolean;
    onSettingsClick: () => void;
}

export function StatusSection({ isRecording, asrConfigured, onSettingsClick }: StatusSectionProps) {
    return (
        <div className="flex flex-col items-center justify-center py-12 relative">
            {/* Main Pulse Visualizer */}
            <button
                onClick={!asrConfigured ? onSettingsClick : undefined}
                disabled={isRecording}
                className="relative z-10 outline-none"
            >
                <div className={`w-32 h-32 md:w-40 md:h-40 rounded-full flex items-center justify-center transition-all duration-500 relative z-20 ${isRecording
                    ? "bg-gradient-to-br from-chinese-indigo/80 to-chinese-indigo/40 backdrop-blur-sm animate-core-breathe"
                    : !asrConfigured
                            ? "bg-slate-50 shadow-[0_0_30px_rgba(15,23,42,0.1)] hover:bg-slate-100 cursor-pointer animate-pulse"
                            : "bg-white shadow-xl shadow-slate-300/50"
                    }`}>
                    {isRecording ? (
                        <Loader2 className="w-12 h-12 md:w-16 md:h-16 text-white/90 animate-spin" />
                    ) : (
                        <Mic className={`w-12 h-12 md:w-16 md:h-16 transition-colors duration-300 ${isRecording ? "text-white/90 animate-pulse"
                            : !asrConfigured ? "text-slate-400"
                                : "text-slate-300"
                            }`} />
                    )}
                </div>
            </button>

            {/* Status Text */}
            <div className="mt-8 text-center space-y-2 z-10">
                <h1 className={`text-3xl md:text-4xl font-bold tracking-tight transition-all ${isRecording
                    ? "text-transparent bg-clip-text bg-gradient-to-r from-chinese-indigo to-chinese-indigo/60"
                    : !asrConfigured ? "text-slate-500"
                            : "text-slate-800"
                    }`}>
                    {isRecording ? "Listening..." : !asrConfigured ? "Setup Required" : "FastSP"}
                </h1>
                <button
                    onClick={onSettingsClick}
                    className={`text-sm md:text-base transition-colors hover:underline ${isRecording ? "text-chinese-indigo/60"
                        : !asrConfigured ? "text-chinese-indigo/80 font-medium"
                                : "text-slate-400"
                        }`}>
                    {isRecording ? "Speak clearly..." : !asrConfigured ? "Open Settings to configure ASR" : "Ready to transcribe"}
                </button>
            </div>
        </div>
    );
}
