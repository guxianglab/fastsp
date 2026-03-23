import { useState, useEffect } from "react";
import { AlertCircle, Settings, Wrench, X } from "lucide-react";
import { HistoryList } from "./components/HistoryList";
import { SettingsModal } from "./components/SettingsModal";
import { StatusSection } from "./components/StatusSection";
import { TitleBar } from "./components/TitleBar";
import { api, events } from "./lib/api";
import "./index.css";

function App() {
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [asrConfigured, setAsrConfigured] = useState(false);
  const [needsSetup, setNeedsSetup] = useState(false);
  const [llmError, setLlmError] = useState<string | null>(null);
  const [runtimeNotice, setRuntimeNotice] = useState<string | null>(null);

  useEffect(() => {
    api.getAsrStatus().then(status => {
      setAsrConfigured(status.configured);
    });

    api.takeRuntimeNotice().then((notice) => {
      if (notice) {
        setRuntimeNotice(notice);
      }
    });

    Promise.all([api.getConfig(), api.getAsrStatus()]).then(([config, asrStatus]) => {
      const noDevice = !config.input_device || config.input_device === "";
      const noAsr = !asrStatus.configured;
      if (noDevice || noAsr) {
        setNeedsSetup(true);
        setIsSettingsOpen(true);
      }
    });

    const unsubRecording = events.onRecordingStatus(setIsRecording);
    const unsubLlmError = events.onLlmError((message) => {
      setLlmError(message);
    });

    return () => {
      unsubRecording.then(f => f());
      unsubLlmError.then(f => f());
    };
  }, []);

  useEffect(() => {
    if (!llmError) return;

    const timeout = window.setTimeout(() => {
      setLlmError(null);
    }, 8000);

    return () => window.clearTimeout(timeout);
  }, [llmError]);

  const handleSettingsClose = () => {
    if (needsSetup) {
      Promise.all([api.getConfig(), api.getAsrStatus()]).then(([config, asrStatus]) => {
        const hasDevice = config.input_device && config.input_device !== "";
        const hasAsr = asrStatus.configured;
        setAsrConfigured(hasAsr);
        if (hasDevice && hasAsr) {
          setNeedsSetup(false);
          setIsSettingsOpen(false);
        }
      });
    } else {
      api.getAsrStatus().then(status => setAsrConfigured(status.configured));
      setIsSettingsOpen(false);
    }
  };

  return (
    <div className="flex h-screen w-full bg-white text-slate-800 overflow-hidden selection:bg-chinese-indigo/20">
      <TitleBar />

      {(runtimeNotice || llmError) && (
        <div className="fixed inset-x-0 top-12 z-40 mx-auto flex max-w-5xl flex-col gap-2 px-4">
          {runtimeNotice && (
            <div className="flex items-start gap-3 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900 shadow-sm">
              <Wrench className="mt-0.5 h-4 w-4 flex-shrink-0" />
              <div className="flex-1">{runtimeNotice}</div>
              <button onClick={() => setRuntimeNotice(null)} className="text-amber-700 transition-colors hover:text-amber-900">
                <X className="h-4 w-4" />
              </button>
            </div>
          )}
          {llmError && (
            <div className="flex items-start gap-3 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-900 shadow-sm">
              <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0" />
              <div className="flex-1">{llmError}</div>
              <button onClick={() => setLlmError(null)} className="text-red-700 transition-colors hover:text-red-900">
                <X className="h-4 w-4" />
              </button>
            </div>
          )}
        </div>
      )}

      {/* Content Container - Padded for TitleBar (h-10 = 40px) */}
      <div className="flex-1 flex flex-col md:flex-row h-full max-w-7xl mx-auto w-full pt-14 pb-4 px-4 md:px-6 gap-6">

        {/* LEFT / TOP: Status & Controls */}
        <div className="flex-1 md:flex-[0.8] flex flex-col items-center justify-center relative min-h-[300px]">
          {/* Settings Button - Now integrated into main view or floating? 
              User removed header, let's keep Settings button but maybe position it better or rely on TitleBar?
              Actually, the mock showed Settings in header. I'll put it in the top-right of the content area or keep it floating.
              Let's keep it floating for now but styled differently.
           */}
          <div className="absolute top-0 right-0 z-10">
            <button
              onClick={() => setIsSettingsOpen(true)}
              className="p-2 hover:bg-slate-100 rounded-lg transition-colors group"
              title="Settings"
            >
              <Settings className="w-5 h-5 text-slate-400 group-hover:text-chinese-indigo transition-colors" />
            </button>
          </div>

          {/* The Main "Eye" */}
          <StatusSection
            isRecording={isRecording}
            asrConfigured={asrConfigured}
            onSettingsClick={() => setIsSettingsOpen(true)}
          />
        </div>

        {/* RIGHT / BOTTOM: History Feed */}
        <div className="flex-[1.2] h-full min-h-[300px] flex flex-col">
          <HistoryList />
        </div>

      </div>

      {/* Modals */}
      <SettingsModal isOpen={isSettingsOpen} onClose={handleSettingsClose} isFirstSetup={needsSetup} />
    </div>
  );
}

export default App;
