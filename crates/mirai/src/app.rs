use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use euclid::Scale;
use libservo::DevicePoint;
use libservo::{
    EventLoopWaker, InputEvent, MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent,
    RenderingContext, Servo, ServoBuilder, WebView, WebViewBuilder, WheelDelta, WheelEvent,
    WheelMode, WindowRenderingContext,
};
use mirai_privacy::Blocker;
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

pub struct AppState {
    pub window: Window,
    pub servo: Servo,
    pub rendering_context: Rc<WindowRenderingContext>,
    pub webviews: RefCell<Vec<WebView>>,
    pub blocker: Blocker,
    pub blocked_count: AtomicUsize,
}

impl AppState {
    fn active_webview(&self) -> Option<WebView> {
        self.webviews.borrow().last().cloned()
    }
}

pub enum App {
    Initial(Waker, Url),
    Running(Rc<AppState>),
}

impl App {
    pub fn new(event_loop: &EventLoop<WakerEvent>, url: Url) -> Self {
        Self::Initial(Waker::new(event_loop), url)
    }
}

impl ApplicationHandler<WakerEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Self::Initial(waker, url) = self else {
            return;
        };

        let start = Instant::now();
        let blocker = Blocker::with_default_lists();
        log::info!("filter lists loaded in {:?}", start.elapsed());

        let display_handle = event_loop
            .display_handle()
            .expect("Failed to get display handle");
        let window = event_loop
            .create_window(Window::default_attributes().with_title("Mirai"))
            .expect("Failed to create winit Window");
        let window_handle = window.window_handle().expect("Failed to get window handle");

        let rendering_context = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, window.inner_size())
                .expect("Could not create RenderingContext for window."),
        );
        let _ = rendering_context.make_current();

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(waker.clone()))
            .build();
        servo.setup_logging();

        let app_state = Rc::new(AppState {
            window,
            servo,
            rendering_context,
            webviews: Default::default(),
            blocker,
            blocked_count: AtomicUsize::new(0),
        });

        let webview = WebViewBuilder::new(&app_state.servo, app_state.rendering_context.clone())
            .url(url.clone())
            .hidpi_scale_factor(Scale::new(app_state.window.scale_factor() as f32))
            .delegate(app_state.clone())
            .build();
        webview.focus();

        app_state.webviews.borrow_mut().push(webview);
        *self = Self::Running(app_state);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: WakerEvent) {
        if let Self::Running(state) = self {
            state.servo.spin_event_loop();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Self::Running(state) = self else {
            return;
        };
        state.servo.spin_event_loop();

        match event {
            WindowEvent::CloseRequested => {
                state.servo.start_shutting_down();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(webview) = state.active_webview() {
                    webview.paint();
                    state.rendering_context.present();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let point = DevicePoint::new(position.x as f32, position.y as f32);
                LAST_CURSOR.set((point.x, point.y));
                if let Some(webview) = state.active_webview() {
                    webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                        point.into(),
                    )));
                }
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                let Some(button) = winit_button_to_servo(button) else {
                    return;
                };
                let action = match element_state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                if let Some(webview) = state.active_webview() {
                    let (x, y) = LAST_CURSOR.get();
                    webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                        action,
                        button,
                        DevicePoint::new(x, y).into(),
                    )));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
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
                        DevicePoint::default().into(),
                    )));
                }
            }
            WindowEvent::Resized(new_size) => {
                if let Some(webview) = state.active_webview() {
                    webview.resize(new_size);
                }
            }
            _ => (),
        }
    }
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
    static LAST_CURSOR: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
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
