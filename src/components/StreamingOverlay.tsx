import { useEffect, useRef, useState } from "react";
import { DictationIntent, events } from "../lib/api";

interface StreamingOverlayProps {
  visible: boolean;
}

type OverlayTheme = {
  panel: string;
  border: string;
  text: string;
  accent: string;
  track: string;
};

const THEMES: Record<DictationIntent, OverlayTheme> = {
  none: {
    panel: "bg-neutral-50/95",
    border: "border-neutral-200",
    text: "text-neutral-900",
    accent: "bg-chinese-indigo",
    track: "bg-neutral-200",
  },
  raw: {
    panel: "bg-stone-50/95",
    border: "border-stone-300/90",
    text: "text-slate-900",
    accent: "bg-slate-400",
    track: "bg-stone-200",
  },
  polish: {
    panel: "bg-neutral-900/92",
    border: "border-chinese-indigo/35",
    text: "text-neutral-50",
    accent: "bg-chinese-indigo",
    track: "bg-white/10",
  },
  skill: {
    panel: "bg-emerald-950/92",
    border: "border-emerald-400/30",
    text: "text-emerald-50",
    accent: "bg-emerald-400",
    track: "bg-white/10",
  },
};

export function StreamingOverlay({ visible }: StreamingOverlayProps) {
  const [streamText, setStreamText] = useState("");
  const [isActive, setIsActive] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [isLlmProcessing, setIsLlmProcessing] = useState(false);
  const [intent, setIntent] = useState<DictationIntent>("none");
  const processingRef = useRef(false);
  const llmProcessingRef = useRef(false);

  useEffect(() => {
    processingRef.current = isProcessing;
  }, [isProcessing]);

  useEffect(() => {
    llmProcessingRef.current = isLlmProcessing;
  }, [isLlmProcessing]);

  useEffect(() => {
    const unsubStream = events.onStreamUpdate((text) => {
      setStreamText(text);
      setIsActive(true);
    });

    const unsubIntent = events.onDictationIntent((nextIntent) => {
      setIntent(nextIntent);
      if (nextIntent !== "none") {
        setIsActive(true);
      }
    });

    const unsubRecording = events.onRecordingStatus((isRecording) => {
      if (isRecording) {
        setIsActive(true);
        return;
      }

      window.setTimeout(() => {
        if (!processingRef.current && !llmProcessingRef.current) {
          setStreamText("");
          setIsActive(false);
        }
      }, 500);
    });

    const unsubProcessing = events.onRecognitionProcessing((processing) => {
      setIsProcessing(processing);
      if (processing) {
        setIsActive(true);
      }
    });

    const unsubLlm = events.onLlmProcessing((processing) => {
      setIsLlmProcessing(processing);
      if (processing) {
        setIsActive(true);
      }
    });

    const unsubTranscription = events.onTranscriptionUpdate((item) => {
      setStreamText(item.text);
      window.setTimeout(() => {
        setStreamText("");
        setIsActive(false);
      }, 1000);
    });

    return () => {
      unsubStream.then((f) => f());
      unsubIntent.then((f) => f());
      unsubRecording.then((f) => f());
      unsubProcessing.then((f) => f());
      unsubLlm.then((f) => f());
      unsubTranscription.then((f) => f());
    };
  }, []);

  if (!visible && !isActive && !isProcessing && !isLlmProcessing) {
    return null;
  }

  const displayText =
    streamText ||
    (isLlmProcessing ? "正在润色..." : isProcessing ? "正在识别..." : "正在听写...");
  const shouldShow = visible || isActive || isProcessing || isLlmProcessing;
  const theme = THEMES[intent];

  return (
    <div className="pointer-events-none fixed bottom-20 left-1/2 z-50 -translate-x-1/2 transform">
      <div
        className={[
          "max-w-lg min-w-[180px] border px-4 py-3 text-sm shadow-[0_18px_45px_-24px_rgba(15,23,42,0.45)] backdrop-blur-sm transition-all duration-300 ease-out",
          theme.panel,
          theme.border,
          theme.text,
          shouldShow ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0",
        ].join(" ")}
      >
        <div className="flex items-center gap-3">
          <div className={`h-4 w-1 flex-shrink-0 transition-colors duration-300 ${theme.accent}`} />
          <span className="truncate">{displayText}</span>
        </div>

        <div className={`mt-2 h-px overflow-hidden transition-colors duration-300 ${theme.track}`}>
          <div
            className={`h-full transition-all duration-300 ${theme.accent}`}
            style={{
              width: streamText ? "100%" : isLlmProcessing ? "80%" : isProcessing ? "60%" : "30%",
            }}
          />
        </div>
      </div>
    </div>
  );
}
