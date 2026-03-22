use rdev::{listen, Button, EventType, Key};
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    Arc,
};
use std::thread;

#[derive(Debug, Clone)]
pub enum InputEvent {
    Start,
    Stop,
    StopForSkill, // Ctrl+Win 专用，用于技能触发
    Toggle,
    MouseMove,
}

/// 将两个屏幕坐标打包成一个 i64（无锁存储）
/// x: 高32位, y: 低32位
#[inline]
fn pack_position(x: f64, y: f64) -> i64 {
    let x_i32 = x as i32;
    let y_i32 = y as i32;
    ((x_i32 as i64) << 32) | (y_i32 as u32 as i64)
}

/// 从 i64 解包出两个屏幕坐标
#[inline]
fn unpack_position(packed: i64) -> (f64, f64) {
    let x = (packed >> 32) as i32 as f64;
    let y = packed as i32 as f64;
    (x, y)
}

pub struct InputListener {
    // Config flags to enable/disable specific triggers
    pub enable_mouse: Arc<AtomicBool>,
    pub enable_hold: Arc<AtomicBool>,
    pub enable_toggle: Arc<AtomicBool>,
    pub track_mouse_position: Arc<AtomicBool>,
    // 存储最新的鼠标位置（使用 AtomicI64 无锁方案）
    pub last_mouse_position: Arc<AtomicI64>,
}

impl InputListener {
    pub fn new() -> Self {
        Self {
            enable_mouse: Arc::new(AtomicBool::new(true)),
            enable_hold: Arc::new(AtomicBool::new(true)),
            enable_toggle: Arc::new(AtomicBool::new(true)),
            track_mouse_position: Arc::new(AtomicBool::new(false)),
            last_mouse_position: Arc::new(AtomicI64::new(0)),
        }
    }

    pub fn get_last_mouse_position(&self) -> (f64, f64) {
        unpack_position(self.last_mouse_position.load(Ordering::Relaxed))
    }

    pub fn start(&self, tx: Sender<InputEvent>) {
        let enable_mouse = self.enable_mouse.clone();
        let enable_hold = self.enable_hold.clone();
        let enable_toggle = self.enable_toggle.clone();
        let track_mouse_position = self.track_mouse_position.clone();
        let last_mouse_position = self.last_mouse_position.clone();

        thread::spawn(move || {
            let mut is_ctrl = false;
            let mut is_win = false;
            let mut combo_active = false;

            if let Err(error) = listen(move |event| {
                match event.event_type {
                    // Mouse Mode
                    EventType::ButtonPress(Button::Middle) => {
                        if enable_mouse.load(Ordering::Relaxed) {
                            tx.send(InputEvent::Start).ok();
                        }
                    }
                    EventType::ButtonRelease(Button::Middle) => {
                        if enable_mouse.load(Ordering::Relaxed) {
                            tx.send(InputEvent::Stop).ok();
                        }
                    }

                    // Toggle Mode (Right Alt)
                    EventType::KeyPress(Key::AltGr) => {
                        // Windows uses AltGr for Right Alt
                        if enable_toggle.load(Ordering::Relaxed) {
                            tx.send(InputEvent::Toggle).ok();
                        }
                    }

                    // Hold Mode (Left Ctrl + Left Win)
                    EventType::KeyPress(Key::ControlLeft) => {
                        is_ctrl = true;
                        check_combo(&enable_hold, &mut combo_active, is_ctrl, is_win, &tx);
                    }
                    EventType::KeyRelease(Key::ControlLeft) => {
                        is_ctrl = false;
                        check_combo(&enable_hold, &mut combo_active, is_ctrl, is_win, &tx);
                    }
                    EventType::KeyPress(Key::MetaLeft) => {
                        is_win = true;
                        check_combo(&enable_hold, &mut combo_active, is_ctrl, is_win, &tx);
                    }
                    EventType::KeyRelease(Key::MetaLeft) => {
                        is_win = false;
                        check_combo(&enable_hold, &mut combo_active, is_ctrl, is_win, &tx);
                    }

                    // Mouse Position Tracking（无锁原子操作）
                    EventType::MouseMove { x, y } => {
                        // 始终更新最新的鼠标位置（原子操作，无锁，不阻塞系统输入）
                        last_mouse_position.store(pack_position(x, y), Ordering::Relaxed);

                        // 只在需要跟踪时发送事件
                        if track_mouse_position.load(Ordering::Relaxed) {
                            tx.send(InputEvent::MouseMove).ok();
                        }
                    }

                    _ => {}
                }
            }) {
                println!("Error in input listener: {:?}", error);
            }
        });
    }
}

fn check_combo(
    enable_hold: &Arc<AtomicBool>,
    active: &mut bool,
    ctrl: bool,
    win: bool,
    tx: &Sender<InputEvent>,
) {
    if !enable_hold.load(Ordering::Relaxed) {
        return;
    }
    let is_combo = ctrl && win;
    if is_combo && !*active {
        *active = true;
        tx.send(InputEvent::Start).ok();
    } else if !is_combo && *active {
        *active = false;
        // Ctrl+Win 释放时发送 StopForSkill 事件
        tx.send(InputEvent::StopForSkill).ok();
    }
}
