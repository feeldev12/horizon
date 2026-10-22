//! Input from Nerion's touch overlay (Android): the touch controls, the soft keyboard and a
//! captured physical mouse send the same events Nerion's Java game gets (its `android/input.rs`),
//! with GLFW key and button codes. The overlay's thread pushes them into a lock-free
//! single-producer ring; the game drains it once per frame and takes the paths a real keyboard
//! and mouse take (`press_key`, `press_mouse`, `type_text`, `MouseHandler.onMove`). Neither side
//! locks or allocates per event, and camera motion is summed and applied once per frame.
// The overlay's side (`push`, the cursor mode, the window size) is only called from Android's
// entry point (android.rs).
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use winit::{
    dpi::PhysicalPosition,
    event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::KeyCode,
};

use super::{Game, Screen};

pub const KEY_DOWN: i32 = 0;
pub const KEY_UP: i32 = 1;
pub const MOUSE_DOWN: i32 = 2;
pub const MOUSE_UP: i32 = 3;
/// `a0`/`a1`: the cursor. In a menu, a position in window pixels; while playing, the overlay's
/// virtual cursor, whose movement turns the camera.
pub const CURSOR_POS: i32 = 4;
/// `a0`/`a1`: horizontal and vertical wheel steps.
pub const SCROLL: i32 = 5;
/// `code`: a Unicode codepoint typed on the soft keyboard.
pub const CHAR: i32 = 6;

/// GLFW's cursor modes, what `nativeGetCursorMode` reports to the overlay.
pub const GLFW_CURSOR_NORMAL: i32 = 0x0003_4001;
pub const GLFW_CURSOR_DISABLED: i32 = 0x0003_4003;

const CAP: usize = 512;

/// Single producer (the activity's UI thread), single consumer (the game thread). `head` and
/// `tail` only grow; their difference is the number of queued events.
struct Ring {
    slots: [[AtomicI32; 4]; CAP],
    head: AtomicU32,
    tail: AtomicU32,
}

static RING: Ring = Ring {
    slots: [const { [const { AtomicI32::new(0) }; 4] }; CAP],
    head: AtomicU32::new(0),
    tail: AtomicU32::new(0),
};

/// Whether the game holds the pointer (playing): the overlay turns drags into camera motion
/// instead of a cursor.
static GRABBED: AtomicBool = AtomicBool::new(false);
/// `width << 32 | height` of the game's surface, `0` before the first frame.
static WINDOW_SIZE: AtomicU64 = AtomicU64::new(0);
/// Minecraft's `guiScale` (0 = auto) | `forceUnicodeFont` << 8, for the overlay to find the hotbar;
/// -1 before the first frame.
static GUI_OPTIONS: AtomicI32 = AtomicI32::new(-1);
/// The overlay's virtual cursor at the last frame while playing (`f32` bits), and whether set.
static LOOK_X: AtomicU32 = AtomicU32::new(0);
static LOOK_Y: AtomicU32 = AtomicU32::new(0);
static LOOK_SET: AtomicBool = AtomicBool::new(false);

/// Queues one event; `false` when the ring is full (the event is dropped).
pub fn push(kind: i32, code: i32, a0: f32, a1: f32) -> bool {
    let tail = RING.tail.load(Ordering::Relaxed);
    let head = RING.head.load(Ordering::Acquire);
    if tail.wrapping_sub(head) as usize >= CAP {
        return false;
    }
    let slot = &RING.slots[tail as usize % CAP];
    slot[0].store(kind, Ordering::Relaxed);
    slot[1].store(code, Ordering::Relaxed);
    slot[2].store(a0.to_bits() as i32, Ordering::Relaxed);
    slot[3].store(a1.to_bits() as i32, Ordering::Relaxed);
    RING.tail.store(tail.wrapping_add(1), Ordering::Release);
    true
}

fn drain(mut each: impl FnMut(i32, i32, f32, f32)) {
    let mut head = RING.head.load(Ordering::Relaxed);
    let tail = RING.tail.load(Ordering::Acquire);
    while head != tail {
        let slot = &RING.slots[head as usize % CAP];
        each(
            slot[0].load(Ordering::Relaxed),
            slot[1].load(Ordering::Relaxed),
            f32::from_bits(slot[2].load(Ordering::Relaxed) as u32),
            f32::from_bits(slot[3].load(Ordering::Relaxed) as u32),
        );
        head = head.wrapping_add(1);
    }
    RING.head.store(head, Ordering::Release);
}

pub fn cursor_mode() -> i32 {
    if GRABBED.load(Ordering::Relaxed) {
        GLFW_CURSOR_DISABLED
    } else {
        GLFW_CURSOR_NORMAL
    }
}

pub fn gui_options() -> i32 {
    GUI_OPTIONS.load(Ordering::Relaxed)
}

fn set_gui_options(settings: &horizon_client::settings::Settings) {
    GUI_OPTIONS.store(
        settings.gui_scale as i32 | i32::from(settings.force_unicode) << 8,
        Ordering::Relaxed,
    );
}

pub fn window_size() -> i64 {
    WINDOW_SIZE.load(Ordering::Relaxed) as i64
}

fn set_window_size(width: u32, height: u32) {
    WINDOW_SIZE.store(
        u64::from(width) << 32 | u64::from(height),
        Ordering::Relaxed,
    );
}

/// A key or a typed character as the keyboard sends it (`WindowEvent::KeyboardInput`), so every
/// screen sees it as it sees a real key: text fields in menus and in game, key binds, the hotbar.
/// A key is one event; a typed character is a press and its release. `None` for anything else,
/// and off Android (see `key_event`).
fn keyboard_events(kind: i32, code: i32) -> Option<Vec<WindowEvent>> {
    use winit::keyboard::{Key, NativeKeyCode, PhysicalKey, SmolStr};
    let (physical, logical, text) = match kind {
        KEY_DOWN | KEY_UP => {
            let key = glfw_key(code)?;
            (PhysicalKey::Code(key), logical_key(key), None)
        }
        CHAR => {
            let typed = SmolStr::new(char::from_u32(code as u32)?.encode_utf8(&mut [0; 4]));
            (
                PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
                Key::Character(typed.clone()),
                Some(typed),
            )
        }
        _ => return None,
    };
    let event = |pressed: bool, text: Option<SmolStr>| {
        Some(WindowEvent::KeyboardInput {
            device_id: DeviceId::dummy(),
            event: key_event(physical, logical.clone(), text, pressed)?,
            is_synthetic: false,
        })
    };
    Some(match kind {
        CHAR => vec![event(true, text)?, event(false, None)?],
        _ => vec![event(kind == KEY_DOWN, None)?],
    })
}

/// winit's `KeyEvent`. Only buildable on Android: its one non-public field, `platform_specific`,
/// is an empty struct there (`KeyEventExtra {}` in winit 0.30), so the public fields are all of
/// it. Elsewhere it carries data this can't fill, and the overlay doesn't run there anyway.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
fn key_event(
    physical: winit::keyboard::PhysicalKey,
    logical: winit::keyboard::Key,
    text: Option<winit::keyboard::SmolStr>,
    pressed: bool,
) -> Option<winit::event::KeyEvent> {
    use std::{mem::MaybeUninit, ptr::addr_of_mut};
    let mut event = MaybeUninit::<winit::event::KeyEvent>::uninit();
    let p = event.as_mut_ptr();
    // SAFETY: every field with bytes is written below; the only other one, `platform_specific`,
    // is zero-sized on Android (see above), so the value is fully initialized.
    unsafe {
        addr_of_mut!((*p).physical_key).write(physical);
        addr_of_mut!((*p).logical_key).write(logical);
        addr_of_mut!((*p).text).write(text);
        addr_of_mut!((*p).location).write(winit::keyboard::KeyLocation::Standard);
        addr_of_mut!((*p).state).write(if pressed {
            ElementState::Pressed
        } else {
            ElementState::Released
        });
        addr_of_mut!((*p).repeat).write(false);
        Some(event.assume_init())
    }
}

#[cfg(not(target_os = "android"))]
fn key_event(
    _physical: winit::keyboard::PhysicalKey,
    _logical: winit::keyboard::Key,
    _text: Option<winit::keyboard::SmolStr>,
    _pressed: bool,
) -> Option<winit::event::KeyEvent> {
    None
}

/// The logical key a US layout gives `key` (what winit reports alongside the physical one).
fn logical_key(key: KeyCode) -> winit::keyboard::Key {
    use winit::keyboard::{Key, NamedKey, SmolStr};
    let named = match key {
        KeyCode::Escape => NamedKey::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => NamedKey::Enter,
        KeyCode::Tab => NamedKey::Tab,
        KeyCode::Backspace => NamedKey::Backspace,
        KeyCode::Delete => NamedKey::Delete,
        KeyCode::Insert => NamedKey::Insert,
        KeyCode::Space => NamedKey::Space,
        KeyCode::ArrowLeft => NamedKey::ArrowLeft,
        KeyCode::ArrowRight => NamedKey::ArrowRight,
        KeyCode::ArrowUp => NamedKey::ArrowUp,
        KeyCode::ArrowDown => NamedKey::ArrowDown,
        KeyCode::Home => NamedKey::Home,
        KeyCode::End => NamedKey::End,
        KeyCode::PageUp => NamedKey::PageUp,
        KeyCode::PageDown => NamedKey::PageDown,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => NamedKey::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => NamedKey::Control,
        KeyCode::AltLeft | KeyCode::AltRight => NamedKey::Alt,
        KeyCode::CapsLock => NamedKey::CapsLock,
        KeyCode::F1 => NamedKey::F1,
        KeyCode::F2 => NamedKey::F2,
        KeyCode::F3 => NamedKey::F3,
        KeyCode::F4 => NamedKey::F4,
        KeyCode::F5 => NamedKey::F5,
        KeyCode::F6 => NamedKey::F6,
        KeyCode::F7 => NamedKey::F7,
        KeyCode::F8 => NamedKey::F8,
        KeyCode::F9 => NamedKey::F9,
        KeyCode::F10 => NamedKey::F10,
        KeyCode::F11 => NamedKey::F11,
        KeyCode::F12 => NamedKey::F12,
        _ => {
            // KeyA -> "a", Digit1 -> "1"; punctuation keeps its name (screens read the physical key).
            let name = format!("{key:?}");
            let short = name
                .strip_prefix("Key")
                .or_else(|| name.strip_prefix("Digit"))
                .unwrap_or(&name)
                .to_lowercase();
            return Key::Character(SmolStr::new(short));
        }
    };
    Key::Named(named)
}

/// A pointer event for a screen with a free cursor (menus), as the mouse would send it.
fn pointer_event(kind: i32, code: i32, a0: f32, a1: f32) -> Option<WindowEvent> {
    // Not a real device; the game never compares device ids.
    let device_id = DeviceId::dummy();
    Some(match kind {
        CURSOR_POS => WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(f64::from(a0), f64::from(a1)),
        },
        MOUSE_DOWN | MOUSE_UP => WindowEvent::MouseInput {
            device_id,
            state: if kind == MOUSE_DOWN {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
            button: glfw_button(code)?,
        },
        SCROLL => WindowEvent::MouseWheel {
            device_id,
            delta: MouseScrollDelta::LineDelta(a0, a1),
            phase: TouchPhase::Moved,
        },
        _ => return None,
    })
}

/// The loading screen and title menu (no game yet): the pointer, keys and typed text, as window
/// events.
pub fn drain_pointer(
    mut each: impl FnMut(WindowEvent),
    size: Option<[u32; 2]>,
    settings: &horizon_client::settings::Settings,
) {
    GRABBED.store(false, Ordering::Relaxed);
    LOOK_SET.store(false, Ordering::Relaxed);
    set_gui_options(settings);
    if let Some([width, height]) = size {
        set_window_size(width, height);
    }
    drain(|kind, code, a0, a1| {
        if let Some(events) = keyboard_events(kind, code) {
            events.into_iter().for_each(&mut each);
        } else if let Some(event) = pointer_event(kind, code, a0, a1) {
            each(event);
        }
    });
}

impl Game<'_> {
    /// This frame's overlay events, before the frame is built.
    pub(crate) fn apply_overlay_input(&mut self, event_loop: &ActiveEventLoop) {
        let playing = self.screen == Screen::Playing && self.captured;
        GRABBED.store(playing, Ordering::Relaxed);
        if !playing {
            LOOK_SET.store(false, Ordering::Relaxed);
        }
        if let Some(renderer) = &self.renderer {
            set_window_size(renderer.config.width, renderer.config.height);
        }
        set_gui_options(&self.settings);
        let in_world = self.require_world().is_ok();
        let window_id = self.window.as_ref().map(|w| w.id());
        let (mut look_x, mut look_y) = (0.0f64, 0.0f64);
        let mut pointer = Vec::new();
        drain(|kind, code, a0, a1| match kind {
            CURSOR_POS if playing => {
                if LOOK_SET.swap(true, Ordering::Relaxed) {
                    let x = f32::from_bits(LOOK_X.load(Ordering::Relaxed));
                    let y = f32::from_bits(LOOK_Y.load(Ordering::Relaxed));
                    look_x += f64::from(a0 - x);
                    look_y += f64::from(a1 - y);
                }
                LOOK_X.store(a0.to_bits(), Ordering::Relaxed);
                LOOK_Y.store(a1.to_bits(), Ordering::Relaxed);
            }
            // As real keyboard events where they can be built (Android): menus' text fields too.
            KEY_DOWN | KEY_UP | CHAR if cfg!(target_os = "android") => {
                pointer.extend(keyboard_events(kind, code).into_iter().flatten());
            }
            KEY_DOWN | KEY_UP => {
                if let (true, Some(key)) = (in_world, glfw_key(code)) {
                    let action = if kind == KEY_DOWN { "down" } else { "up" };
                    if let Err(e) = self.press_key(key, action) {
                        log::debug!("Horizon: overlay key {code}: {e}");
                    }
                }
            }
            MOUSE_DOWN | MOUSE_UP if playing => {
                if let Some(button) = glfw_button(code) {
                    let action = if kind == MOUSE_DOWN { "down" } else { "up" };
                    if let Err(e) = self.press_mouse(button, action) {
                        log::debug!("Horizon: overlay button {code}: {e}");
                    }
                }
            }
            CHAR => {
                if let (true, Some(c)) = (in_world, char::from_u32(code as u32)) {
                    let mut buf = [0; 4];
                    let _ = self.type_text(c.encode_utf8(&mut buf));
                }
            }
            _ => pointer.extend(pointer_event(kind, code, a0, a1)),
        });
        if let Some(id) = window_id {
            for event in pointer {
                winit::application::ApplicationHandler::window_event(self, event_loop, id, event);
            }
        }
        if look_x != 0.0 || look_y != 0.0 {
            // MouseHandler.onMove with the pointer grabbed, as a real mouse's motion.
            self.mouse.on_move(0.0, 0.0, look_x, look_y, true, true);
            self.last_input = Instant::now();
        }
    }
}

/// GLFW's mouse buttons (`GLFW_MOUSE_BUTTON_LEFT` = 0, right 1, middle 2).
fn glfw_button(code: i32) -> Option<MouseButton> {
    Some(match code {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        _ => return None,
    })
}

/// GLFW key codes (`glfw3.h`) as winit's physical keys.
fn glfw_key(code: i32) -> Option<KeyCode> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    const NUMPAD: [KeyCode; 10] = [
        Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    ];
    const FUNCTION: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    Some(match code {
        65..=90 => LETTERS[(code - 65) as usize],
        48..=57 => DIGITS[(code - 48) as usize],
        320..=329 => NUMPAD[(code - 320) as usize],
        290..=301 => FUNCTION[(code - 290) as usize],
        32 => Space,
        39 => Quote,
        44 => Comma,
        45 => Minus,
        46 => Period,
        47 => Slash,
        59 => Semicolon,
        61 => Equal,
        91 => BracketLeft,
        92 => Backslash,
        93 => BracketRight,
        96 => Backquote,
        256 => Escape,
        257 => Enter,
        258 => Tab,
        259 => Backspace,
        260 => Insert,
        261 => Delete,
        262 => ArrowRight,
        263 => ArrowLeft,
        264 => ArrowDown,
        265 => ArrowUp,
        266 => PageUp,
        267 => PageDown,
        268 => Home,
        269 => End,
        280 => CapsLock,
        330 => NumpadDecimal,
        331 => NumpadDivide,
        332 => NumpadMultiply,
        333 => NumpadSubtract,
        334 => NumpadAdd,
        335 => NumpadEnter,
        340 => ShiftLeft,
        341 => ControlLeft,
        342 => AltLeft,
        344 => ShiftRight,
        345 => ControlRight,
        346 => AltRight,
        _ => return None,
    })
}
