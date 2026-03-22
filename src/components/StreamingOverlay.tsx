import { useEffect, useRef, useState } from "react";
import { events } from "../lib/api";

interface StreamingOverlayProps {
  visible: boolean;
}

export function StreamingOverlay({ visible }: StreamingOverlayProps) {
  const [streamText, setStreamText] = useState("");
  const [isActive, setIsActive] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [isLlmProcessing, setIsLlmProcessing] = useState(false);
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

  return (
    <div className="fixed bottom-20 left-1/2 transform -translate-x-1/2 z-50 pointer-events-none">
      <div
        className={`
          bg-slate-900/90 backdrop-blur-sm
          text-white text-lg font-medium
          px-6 py-3 rounded-2xl
          shadow-2xl shadow-black/20
          border border-slate-700/50
          max-w-lg min-w-[200px]
          transition-all duration-300 ease-out
          ${shouldShow ? "opacity-100 translate-y-0" : "opacity-0 translate-y-4"}
        `}
      >
        <div className="flex items-center gap-3">
          <div className="relative flex-shrink-0">
            <div className="w-2.5 h-2.5 bg-emerald-400 rounded-full animate-pulse" />
            <div className="absolute inset-0 w-2.5 h-2.5 bg-emerald-400 rounded-full animate-ping opacity-75" />
          </div>

          <span className="truncate">{displayText}</span>
        </div>

        <div className="mt-2 h-0.5 bg-slate-700 rounded-full overflow-hidden">
          <div
            className="h-full bg-gradient-to-r from-chinese-indigo to-emerald-400 rounded-full animate-pulse"
            style={{
              width: streamText ? "100%" : isLlmProcessing ? "80%" : isProcessing ? "60%" : "30%",
            }}
          />
        </div>
      </div>
    </div>
  );
}
