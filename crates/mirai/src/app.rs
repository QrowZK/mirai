use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Instant;

use euclid::Scale;
use libservo::{
    DevicePoint, EventLoopWaker, InputEvent, MouseButton, MouseButtonAction, MouseButtonEvent,
    MouseMoveEvent, OffscreenRenderingContext, RenderingContext, Servo, ServoBuilder, WebView,
    WebViewBuilder, WebViewId, WheelDelta, WheelEvent, WheelMode, WindowRenderingContext,
};
use mirai_privacy::Blocker;
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::ui::Gui;

/// Actions requested by the chrome, applied between egui frames.
pub enum Command {
    Go(String),
    Back,
    Forward,
    Reload,
    NewTab,
    CloseTab(usize),
    CloseActiveTab,
    ActivateTab(usize),
}

pub struct AppState {
    pub window: Window,
    pub servo: Servo,
    pub rendering_context: Rc<OffscreenRenderingContext>,
    pub tabs: RefCell<Vec<WebView>>,
    pub active_tab: Cell<usize>,
    pub blocker: Blocker,
    pub blocking_enabled: AtomicBool,
    pub blocked_count: AtomicUsize,
    pub blocked_counts_per_tab: RefCell<HashMap<WebViewId, usize>>,
    commands: RefCell<Vec<Command>>,
}

impl AppState {
    pub fn active_webview(&self) -> Option<WebView> {
        self.tabs.borrow().get(self.active_tab.get()).cloned()
    }

    pub fn queue_command(&self, command: Command) {
        self.commands.borrow_mut().push(command);
    }

    pub fn new_tab(self: &Rc<Self>, url: Url) -> WebView {
        let webview = WebViewBuilder::new(&self.servo, self.rendering_context.clone())
            .url(url)
            .hidpi_scale_factor(Scale::new(self.window.scale_factor() as f32))
            .delegate(self.clone())
            .build();
        webview.focus();
        webview
    }

    fn activate(&self, index: usize) {
        let tabs = self.tabs.borrow();
        let Some(webview) = tabs.get(index) else {
            return;
        };
        if let Some(old) = tabs.get(self.active_tab.get()) {
            old.hide();
        }
        self.active_tab.set(index);
        webview.show();
        webview.focus();
        self.window.request_redraw();
    }
}

pub enum App {
    Initial(Waker, Url),
    Running { state: Rc<AppState>, gui: Gui },
}

impl App {
    pub fn new(event_loop: &EventLoop<WakerEvent>, url: Url) -> Self {
        Self::Initial(Waker::new(event_loop), url)
    }

    /// Apply chrome commands queued during the last egui frame.
    fn process_commands(state: &Rc<AppState>) {
        let commands = std::mem::take(&mut *state.commands.borrow_mut());
        for command in commands {
            match command {
                Command::Go(input) => {
                    if let Some(url) = parse_location_input(&input) {
                        if let Some(webview) = state.active_webview() {
                            webview.load(url);
                        }
                    }
                }
                Command::Back => {
                    if let Some(webview) = state.active_webview() {
                        if webview.can_go_back() {
                            webview.go_back(1);
                        }
                    }
                }
                Command::Forward => {
                    if let Some(webview) = state.active_webview() {
                        if webview.can_go_forward() {
                            webview.go_forward(1);
                        }
                    }
                }
                Command::Reload => {
                    if let Some(webview) = state.active_webview() {
                        webview.reload();
                    }
                }
                Command::NewTab => {
                    let webview = state
                        .new_tab(Url::parse(crate::DEFAULT_URL).expect("default URL is valid"));
                    state.tabs.borrow_mut().push(webview);
                    let last = state.tabs.borrow().len() - 1;
                    state.activate(last);
                }
                Command::CloseTab(index) => Self::close_tab(state, index),
                Command::CloseActiveTab => Self::close_tab(state, state.active_tab.get()),
                Command::ActivateTab(index) => state.activate(index),
            }
        }
        state.window.request_redraw();
    }

    fn close_tab(state: &Rc<AppState>, index: usize) {
        let mut tabs = state.tabs.borrow_mut();
        if index >= tabs.len() {
            return;
        }
        let webview = tabs.remove(index);
        state
            .blocked_counts_per_tab
            .borrow_mut()
            .remove(&webview.id());
        drop(webview);
        let len = tabs.len();
        drop(tabs);
        if len == 0 {
            // Closing the last tab opens a fresh one rather than a blank window.
            let webview =
                state.new_tab(Url::parse(crate::DEFAULT_URL).expect("default URL is valid"));
            state.tabs.borrow_mut().push(webview);
            state.activate(0);
        } else {
            state.activate(index.min(len - 1));
        }
    }
}

impl ApplicationHandler<WakerEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Self::Initial(waker, url) = self else {
            return;
        };

        let start = Instant::now();
        let blocker = match cache_dir() {
            Some(cache_dir) => Blocker::with_default_lists_cached(&cache_dir),
            None => Blocker::with_default_lists(),
        };
        let blocker_load_time = start.elapsed();

        let display_handle = event_loop
            .display_handle()
            .expect("Failed to get display handle");
        let icon = winit::window::Icon::from_rgba(
            include_bytes!("../../../assets/icon-32.rgba").to_vec(),
            32,
            32,
        )
        .ok();
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("Mirai")
                    .with_window_icon(icon),
            )
            .expect("Failed to create winit Window");
        let window_handle = window.window_handle().expect("Failed to get window handle");

        let window_rendering_context = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, window.inner_size())
                .expect("Could not create RenderingContext for window."),
        );
        let rendering_context =
            Rc::new(window_rendering_context.offscreen_context(window.inner_size()));
        let _ = rendering_context.make_current();

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(waker.clone()))
            .build();
        servo.setup_logging();
        log::info!("filter lists loaded in {blocker_load_time:?}");

        let gui = Gui::new(event_loop, rendering_context.clone(), url);

        let app_state = Rc::new(AppState {
            window,
            servo,
            rendering_context,
            tabs: Default::default(),
            active_tab: Cell::new(0),
            blocker,
            blocking_enabled: AtomicBool::new(true),
            blocked_count: AtomicUsize::new(0),
            blocked_counts_per_tab: Default::default(),
            commands: Default::default(),
        });

        let webview = app_state.new_tab(url.clone());
        webview.show();
        app_state.tabs.borrow_mut().push(webview);
        app_state.window.request_redraw();

        *self = Self::Running {
            state: app_state,
            gui,
        };
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: WakerEvent) {
        if let Self::Running { state, .. } = self {
            state.servo.spin_event_loop();
            state.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Self::Running { state, gui } = self else {
            return;
        };
        state.servo.spin_event_loop();

        // The chrome gets first refusal on every event.
        let response = gui.on_window_event(&state.window, &event);
        if response.repaint {
            state.window.request_redraw();
        }

        let scale = state.window.scale_factor() as f32;
        let toolbar_offset = gui.toolbar_height().get() * scale;

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                gui.update_location(state);
                gui.update(&state.window, state);
                Self::process_commands(state);
                gui.paint(&state.window);
            }
            WindowEvent::CursorMoved { position, .. } if !response.consumed => {
                let point = DevicePoint::new(position.x as f32, position.y as f32 - toolbar_offset);
                LAST_CURSOR.set((point.x, point.y));
                if point.y >= 0.0 {
                    if let Some(webview) = state.active_webview() {
                        webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                            point.into(),
                        )));
                    }
                }
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } if !response.consumed => {
                let Some(button) = winit_button_to_servo(button) else {
                    return;
                };
                let action = match element_state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                let (x, y) = LAST_CURSOR.get();
                if y >= 0.0 {
                    if let Some(webview) = state.active_webview() {
                        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                            action,
                            button,
                            DevicePoint::new(x, y).into(),
                        )));
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !response.consumed => {
                let (x, y) = LAST_CURSOR.get();
                if y < 0.0 {
                    return;
                }
                if let Some(webview) = state.active_webview() {
                    let (delta_x, delta_y, mode) = match delta {
                        MouseScrollDelta::LineDelta(dx, dy) => {
                            ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                        }
                        MouseScrollDelta::PixelDelta(delta) => {
                            (delta.x, delta.y, WheelMode::DeltaPixel)
                        }
                    };
                    webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(
                        WheelDelta {
                            x: delta_x,
                            y: delta_y,
                            z: 0.0,
                            mode,
                        },
                        DevicePoint::new(x, y).into(),
                    )));
                }
            }
            WindowEvent::Resized(new_size) => {
                state.rendering_context.parent_context().resize(new_size);
                state.window.request_redraw();
            }
            _ => (),
        }
    }
}

/// The platform cache directory for Mirai (compiled filter-list cache, etc.).
fn cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    Some(base?.join("mirai"))
}

/// Interpret URL-bar input: a URL, a bare domain, or otherwise a search query.
fn parse_location_input(input: &str) -> Option<Url> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(input) {
        return Some(url);
    }
    if input.contains('.') && !input.contains(' ') {
        if let Ok(url) = Url::parse(&format!("https://{input}")) {
            return Some(url);
        }
    }
    Url::parse(&format!(
        "https://duckduckgo.com/?q={}",
        url::form_urlencoded::byte_serialize(input.as_bytes()).collect::<String>()
    ))
    .ok()
}

fn winit_button_to_servo(button: winit::event::MouseButton) -> Option<MouseButton> {
    match button {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

thread_local! {
    static LAST_CURSOR: Cell<(f32, f32)> = const { Cell::new((0.0, 0.0)) };
}

#[derive(Clone)]
pub struct Waker(winit::event_loop::EventLoopProxy<WakerEvent>);

#[derive(Debug)]
pub struct WakerEvent;

impl Waker {
    fn new(event_loop: &EventLoop<WakerEvent>) -> Self {
        Self(event_loop.create_proxy())
    }
}

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(Self(self.0.clone()))
    }

    fn wake(&self) {
        if self.0.send_event(WakerEvent).is_err() {
            log::warn!("Failed to wake event loop");
        }
    }
}
