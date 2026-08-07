//! The egui-based browser chrome: toolbar, URL bar, and tab strip.
//!
//! Rendering follows the servoshell pattern: web content is painted by Servo
//! into an [`OffscreenRenderingContext`], and egui blits that texture into the
//! area below the toolbar via a paint callback while drawing the chrome
//! around it.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use egui::{Key, Modifiers, PaintCallback, Panel, Vec2};
use egui_glow::{CallbackFn, EguiGlow};
use egui_winit::EventResponse;
use euclid::{Length, Point2D, Rect, Scale, Size2D};
use libservo::{
    DeviceIndependentPixel, DevicePixel, LoadStatus, OffscreenRenderingContext, RenderingContext,
};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::app::{AppState, Command};

pub struct Gui {
    rendering_context: std::rc::Rc<OffscreenRenderingContext>,
    context: EguiGlow,
    toolbar_height: Length<f32, DeviceIndependentPixel>,
    location: String,
    /// Whether the location field has been edited by the user and should not
    /// be overwritten by webview URL updates.
    location_dirty: bool,
    settings_open: bool,
}

impl Drop for Gui {
    fn drop(&mut self) {
        let _ = self.rendering_context.make_current();
        self.context.destroy();
    }
}

impl Gui {
    pub fn new(
        event_loop: &ActiveEventLoop,
        rendering_context: std::rc::Rc<OffscreenRenderingContext>,
        initial_url: &url::Url,
    ) -> Self {
        rendering_context
            .make_current()
            .expect("Could not make rendering context current");
        let context = EguiGlow::new(
            event_loop,
            rendering_context.glow_gl_api(),
            None,
            None,
            false,
        );
        context.egui_ctx.options_mut(|options| {
            options.fallback_theme = egui::Theme::Light;
        });
        context.egui_ctx.set_visuals(mirai_visuals());

        Self {
            rendering_context,
            context,
            toolbar_height: Default::default(),
            location: initial_url.to_string(),
            location_dirty: false,
            settings_open: false,
        }
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> EventResponse {
        self.context.on_window_event(window, event)
    }

    /// The height of the chrome above the web content, in logical pixels.
    pub fn toolbar_height(&self) -> Length<f32, DeviceIndependentPixel> {
        self.toolbar_height
    }

    /// Sync the location field with the active webview, unless the user is
    /// mid-edit.
    pub fn update_location(&mut self, state: &AppState) {
        if self.location_dirty {
            return;
        }
        if let Some(url) = state.active_webview().and_then(|webview| webview.url()) {
            let url = url.to_string();
            if url != self.location {
                self.location = url;
            }
        }
    }

    /// Run the egui frame: draw the chrome, size the webview to the remaining
    /// space, and queue the web content blit.
    pub fn update(&mut self, window: &Window, state: &AppState) {
        self.rendering_context
            .make_current()
            .expect("Could not make rendering context current");

        let Self {
            rendering_context,
            context,
            toolbar_height,
            location,
            location_dirty,
            settings_open,
        } = self;

        context.run(window, |ctx| {
            // Keyboard shortcuts, handled at the chrome level.
            ctx.input_mut(|input| {
                if input.consume_key(Modifiers::COMMAND, Key::T) {
                    state.queue_command(Command::NewTab);
                }
                if input.consume_key(Modifiers::COMMAND, Key::W) {
                    state.queue_command(Command::CloseActiveTab);
                }
                if input.consume_key(Modifiers::COMMAND, Key::R) {
                    state.queue_command(Command::Reload);
                }
            });

            let frame = egui::Frame::default()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(4.0);
            Panel::top("toolbar").frame(frame).show_inside(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    ui.available_size(),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let webview = state.active_webview();
                        let (can_go_back, can_go_forward, load_status) = webview
                            .as_ref()
                            .map(|webview| {
                                (
                                    webview.can_go_back(),
                                    webview.can_go_forward(),
                                    webview.load_status(),
                                )
                            })
                            .unwrap_or((false, false, LoadStatus::Complete));

                        if ui.add_enabled(can_go_back, toolbar_button("⏴")).clicked() {
                            *location_dirty = false;
                            state.queue_command(Command::Back);
                        }
                        if ui
                            .add_enabled(can_go_forward, toolbar_button("⏵"))
                            .clicked()
                        {
                            *location_dirty = false;
                            state.queue_command(Command::Forward);
                        }
                        let reload_label = match load_status {
                            LoadStatus::Started | LoadStatus::HeadParsed => "…",
                            LoadStatus::Complete => "↻",
                        };
                        if ui.add(toolbar_button(reload_label)).clicked() {
                            *location_dirty = false;
                            state.queue_command(Command::Reload);
                        }
                        ui.add_space(2.0);

                        ui.allocate_ui_with_layout(
                            ui.available_size(),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.add(toolbar_button("☰")).clicked() {
                                    *settings_open = !*settings_open;
                                }

                                let blocked_in_tab = state
                                    .active_webview()
                                    .and_then(|webview| {
                                        state
                                            .blocked_counts_per_tab
                                            .borrow()
                                            .get(&webview.id())
                                            .copied()
                                    })
                                    .unwrap_or(0);
                                let blocked_total = state.blocked_count.load(Ordering::Relaxed);
                                if blocked_total > 0 {
                                    ui.label(format!("🛡 {blocked_in_tab}"))
                                        .on_hover_text(format!(
                                            "{blocked_in_tab} ads/trackers blocked in this tab \
                                             ({blocked_total} this session)"
                                        ));
                                }

                                let location_id = egui::Id::new("location_input");
                                let location_field = ui.add_sized(
                                    ui.available_size(),
                                    egui::TextEdit::singleline(location)
                                        .id(location_id)
                                        .hint_text("Search or enter address"),
                                );
                                if location_field.changed() {
                                    *location_dirty = true;
                                }
                                if ui.input(|i| i.clone().consume_key(Modifiers::COMMAND, Key::L)) {
                                    location_field.request_focus();
                                }
                                if location_field.lost_focus()
                                    && ui.input(|i| i.clone().key_pressed(Key::Enter))
                                {
                                    *location_dirty = false;
                                    state.queue_command(Command::Go(location.clone()));
                                }
                            },
                        );
                    },
                );
            });

            let outer = Panel::top("tabs").show_inside(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.allocate_ui_with_layout(
                            ui.available_size(),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let tabs = state.tabs.borrow();
                                let active_index = state.active_tab.get();
                                for (index, webview) in tabs.iter().enumerate() {
                                    let label = match (webview.page_title(), webview.url()) {
                                        (Some(title), _) if !title.is_empty() => title,
                                        (_, Some(url)) => url.to_string(),
                                        _ => "New Tab".into(),
                                    };
                                    let tab = ui.add(egui::Button::selectable(
                                        index == active_index,
                                        truncate_with_ellipsis(&label, 20),
                                    ));
                                    let close = ui.add(
                                        egui::Button::new("✕").fill(egui::Color32::TRANSPARENT),
                                    );
                                    if close.clicked() || tab.middle_clicked() {
                                        state.queue_command(Command::CloseTab(index));
                                    } else if tab.clicked() && index != active_index {
                                        *location_dirty = false;
                                        state.queue_command(Command::ActivateTab(index));
                                    }
                                }
                                if ui.add(toolbar_button("+")).clicked() {
                                    state.queue_command(Command::NewTab);
                                }
                            },
                        );
                    })
            });
            *toolbar_height = Length::new(outer.response.rect.max.y);

            if *settings_open {
                egui::Window::new("Settings")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
                    .open(settings_open)
                    .show(ctx, |ui| {
                        let mut blocking = state
                            .blocking_enabled
                            .load(std::sync::atomic::Ordering::Relaxed);
                        if ui.checkbox(&mut blocking, "Block ads & trackers").changed() {
                            state
                                .blocking_enabled
                                .store(blocking, std::sync::atomic::Ordering::Relaxed);
                        }
                        ui.label(
                            egui::RichText::new(
                                "EasyList + EasyPrivacy, evaluated locally. \
                                 Blocked requests never leave this machine.",
                            )
                            .small()
                            .weak(),
                        );
                    });
            }

            // Size the active webview to the space left below the chrome.
            let scale =
                Scale::<_, DeviceIndependentPixel, DevicePixel>::new(ctx.pixels_per_point());
            let available_rect = ctx.available_rect_before_wrap();
            let size = Size2D::new(available_rect.width(), available_rect.height()) * scale;
            if let Some(webview) = state.active_webview() {
                if size != webview.size() && size.width > 0.0 && size.height > 0.0 {
                    webview.resize(winit::dpi::PhysicalSize::new(
                        size.width as u32,
                        size.height as u32,
                    ));
                }
                webview.paint();
            }

            // Blit the web content into the area below the toolbar.
            if let Some(render_to_parent) = rendering_context.render_to_parent_callback() {
                ctx.layer_painter(egui::LayerId::background())
                    .add(PaintCallback {
                        rect: available_rect,
                        callback: Arc::new(CallbackFn::new(move |info, painter| {
                            let clip = info.viewport_in_pixels();
                            let rect_in_parent = Rect::new(
                                Point2D::new(clip.left_px, clip.from_bottom_px),
                                Size2D::new(clip.width_px, clip.height_px),
                            );
                            render_to_parent(painter.gl(), rect_in_parent)
                        })),
                    });
            }
        });

        if self.context.egui_ctx.has_requested_repaint() {
            window.request_redraw();
        }
    }

    /// Paint the chrome (and the queued web-content blit) to the window.
    pub fn paint(&mut self, window: &Window) {
        self.rendering_context
            .make_current()
            .expect("Could not make rendering context current");
        self.rendering_context
            .parent_context()
            .prepare_for_rendering();
        self.context.paint(window);
        self.rendering_context.parent_context().present();
    }
}

fn toolbar_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: 20.0, y: 20.0 })
}

fn truncate_with_ellipsis(input: &str, max_length: usize) -> String {
    if input.chars().count() > max_length {
        let truncated: String = input.chars().take(max_length.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        input.to_string()
    }
}

/// mirai design system: warm graphite neutrals, muted teal accent, jade for
/// privacy. Values from the design team's tokens (`tokens/colors.css`,
/// `tokens/radius.css`), light theme.
const PRIVACY_JADE: egui::Color32 = egui::Color32::from_rgb(0x2F, 0x7D, 0x5B);

fn mirai_visuals() -> egui::Visuals {
    use egui::{Color32, CornerRadius, Stroke};

    let surface_window = Color32::from_rgb(0xF1, 0xEE, 0xE8);
    let surface_sunken = Color32::from_rgb(0xE4, 0xE0, 0xD9);
    let surface_hover = Color32::from_rgb(0xE8, 0xE4, 0xDC);
    let surface_active = Color32::from_rgb(0xDB, 0xD6, 0xCC);
    let surface_field = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    let text_strong = Color32::from_rgb(0x1C, 0x1B, 0x19);
    let text_body = Color32::from_rgb(0x2E, 0x2C, 0x29);
    let text_muted = Color32::from_rgb(0x62, 0x5E, 0x58);
    let border_hairline = Color32::from_rgb(0xE4, 0xE0, 0xD9);
    let border_default = Color32::from_rgb(0xCF, 0xCA, 0xC1);
    let border_focus = Color32::from_rgb(0x12, 0x93, 0x9A);
    let selection_bg = Color32::from_rgb(0xC2, 0xE4, 0xE6);
    let link = Color32::from_rgb(0x0E, 0x7A, 0x80);

    let radius = CornerRadius::same(5);
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(text_body);
    visuals.window_fill = surface_window;
    visuals.panel_fill = surface_window;
    visuals.faint_bg_color = surface_sunken;
    visuals.extreme_bg_color = surface_field;
    visuals.hyperlink_color = link;
    visuals.selection.bg_fill = selection_bg;
    visuals.selection.stroke = Stroke::new(1.0, text_strong);
    visuals.window_stroke = Stroke::new(1.0, border_default);

    let widgets = &mut visuals.widgets;
    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = radius;
        widget.fg_stroke = Stroke::new(1.0, text_body);
    }
    widgets.noninteractive.bg_fill = surface_window;
    widgets.noninteractive.weak_bg_fill = surface_window;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, border_hairline);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, text_muted);
    widgets.inactive.bg_fill = surface_sunken;
    widgets.inactive.weak_bg_fill = Color32::from_rgb(0xE7, 0xE3, 0xDB);
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.hovered.bg_fill = surface_hover;
    widgets.hovered.weak_bg_fill = surface_hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, border_default);
    widgets.active.bg_fill = surface_active;
    widgets.active.weak_bg_fill = surface_active;
    widgets.active.bg_stroke = Stroke::new(1.0, border_focus);
    widgets.active.fg_stroke = Stroke::new(1.0, text_strong);
    widgets.open.bg_fill = surface_hover;
    widgets.open.weak_bg_fill = surface_hover;
    widgets.open.bg_stroke = Stroke::new(1.0, border_default);

    visuals
}
