import { useEffect, useState, useRef } from "react";
import { Copy, Trash2, Clock } from "lucide-react";
import { api, events, HistoryItem } from "../lib/api";

export function HistoryList() {
    const [items, setItems] = useState<HistoryItem[]>([]);
    const scrollRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        api.getHistory().then(setItems);
        const u = events.onTranscriptionUpdate((item) => {
            setItems(prev => [item, ...prev]);
        });
        return () => { u.then(f => f()); };
    }, []);

    const copyText = (text: string) => {
        navigator.clipboard.writeText(text);
    };

    const clearAll = async () => {
        if (confirm("Clear all history?")) {
            await api.clearHistory();
            setItems([]);
        }
    }

    return (
        <div className="flex flex-col h-full bg-white/80 backdrop-blur-md rounded-2xl border border-slate-200 shadow-sm overflow-hidden">
            <div className="p-4 border-b border-slate-100 flex justify-between items-center bg-slate-50/50">
                <h2 className="text-sm font-semibold text-slate-500 uppercase tracking-wider">Recent Transcriptions</h2>
                {items.length > 0 && (
                    <button onClick={clearAll} className="p-2 text-slate-400 hover:text-red-500 hover:bg-red-50 rounded-lg transition-colors">
                        <Trash2 className="w-4 h-4" />
                    </button>
                )}
            </div>

            <div className="flex-1 overflow-y-auto p-3 space-y-2 custom-scrollbar" ref={scrollRef}>
                {items.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-48 text-slate-400">
                        <Clock className="w-8 h-8 mb-2 opacity-60 text-chinese-indigo" />
                        <p className="text-sm">No history yet</p>
                    </div>
                ) : (
                    items.map((item, index) => (
                        <div
                            key={item.id}
                            className="group relative bg-white hover:bg-slate-50 px-3 py-2 rounded-lg border border-slate-100 hover:border-chinese-indigo/30 shadow-sm hover:shadow transition-all duration-200 animate-in fade-in slide-in-from-top-2"
                            style={{ animationDelay: `${index * 30}ms` }}
                        >
                            <div className="flex justify-between items-center gap-2">
                                <p className="text-slate-800 text-sm leading-snug flex-1 line-clamp-2">{item.text}</p>
                                <div className="flex items-center gap-1 shrink-0">
                                    <span className="text-xs text-slate-400 hidden group-hover:inline">{item.timestamp.split(' ')[1]}</span>
                                    <button onClick={() => copyText(item.text)} className="opacity-0 group-hover:opacity-100 p-1 hover:text-chinese-indigo transition-opacity">
                                        <Copy className="w-3 h-3" />
                                    </button>
                                </div>
                            </div>
                        </div>
                    ))
                )}
            </div>
        </div>
    );
}
