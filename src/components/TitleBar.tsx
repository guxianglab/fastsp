import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, X } from "lucide-react";

export function TitleBar() {
    const minimize = async () => {
        await getCurrentWindow().minimize();
    };

    const close = async () => {
        await getCurrentWindow().close();
    };

    return (
        <div className="h-10 bg-white flex justify-between items-center select-none fixed top-0 left-0 right-0 z-50 border-b border-slate-100">
            {/* Drag Region - Covers the empty space */}
            <div
                data-tauri-drag-region
                className="absolute inset-0 z-0"
            />

            {/* Logo / Title Area */}
            <div
                className="flex items-center gap-3 px-4 relative z-10 pointer-events-none"
                data-tauri-drag-region
            >
                <img src="/icons/32x32.png" alt="Logo" className="w-5 h-5" />
                <span className="font-semibold text-slate-700 text-sm tracking-wide">FASTSP</span>
            </div>

            {/* Window Controls - Higher Z-index to receive clicks */}
            <div className="flex h-full relative z-10">
                <button
                    onClick={minimize}
                    className="w-12 h-full inline-flex items-center justify-center hover:bg-slate-100 text-slate-500 transition-colors"
                >
                    <Minus className="w-4 h-4" />
                </button>
                <button
                    onClick={close}
                    className="w-12 h-full inline-flex items-center justify-center hover:bg-red-500 hover:text-white text-slate-500 transition-colors"
                >
                    <X className="w-4 h-4" />
                </button>
            </div>
        </div>
    );
}

