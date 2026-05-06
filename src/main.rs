mod data;

use data::{compute_app_data, load_history_timestamps, AppData};
use iced::{
    alignment,
    mouse,
    widget::canvas::{self, Cache, Path, Stroke},
    Color, Element, Length, Pixels, Point, Radians, Rectangle, Renderer, Size, Subscription,
    Task, Theme,
};
use std::f32::consts::PI;
use std::time::Duration;

// Arc: 8-o'clock (150°) → clockwise 240° → 4-o'clock
const START_ANGLE: f32 = 5.0 * PI / 6.0;
const TOTAL_SWEEP: f32 = 4.0 * PI / 3.0;

const SESSION_CAP: f32 = 200_000.0;
const WEEKLY_CAP: f32 = 1_800_000.0;

const COLOR_BG: Color = Color { r: 0.051, g: 0.051, b: 0.051, a: 1.0 };
const COLOR_TRACK: Color = Color { r: 0.200, g: 0.200, b: 0.200, a: 1.0 };
const COLOR_GREEN: Color = Color { r: 0.220, g: 0.737, b: 0.388, a: 1.0 };
const COLOR_YELLOW: Color = Color { r: 0.941, g: 0.706, b: 0.161, a: 1.0 };
const COLOR_RED: Color = Color { r: 0.957, g: 0.263, b: 0.212, a: 1.0 };
const COLOR_BLOCK: Color = Color { r: 0.098, g: 0.098, b: 0.098, a: 1.0 };
const COLOR_DIM: Color = Color { r: 0.45, g: 0.45, b: 0.45, a: 1.0 };

// Number of arc segments used to fake a gradient stroke
const GRADIENT_SEGS: usize = 120;

struct ClaudeMeter {
    timestamps: Vec<u64>,
    data: AppData,
    cache: Cache,
    dot_visible: bool,
    displayed_frac: f32, // animated, interpolates toward target each frame
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Reload,
    DotOff,
    Animate,
}

impl Default for ClaudeMeter {
    fn default() -> Self {
        let timestamps = load_history_timestamps();
        let data = compute_app_data(&timestamps);
        let displayed_frac = (data.tokens_session as f32 / SESSION_CAP).min(1.0);
        Self { timestamps, data, cache: Cache::default(), dot_visible: false, displayed_frac }
    }
}

impl ClaudeMeter {
    fn target_frac(&self) -> f32 {
        (self.data.tokens_session as f32 / SESSION_CAP).min(1.0)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                self.data = compute_app_data(&self.timestamps);
                self.dot_visible = true;
                self.cache.clear();
                Task::perform(
                    tokio::time::sleep(Duration::from_millis(400)),
                    |_| Message::DotOff,
                )
            }
            Message::Reload => {
                self.timestamps = load_history_timestamps();
                self.data = compute_app_data(&self.timestamps);
                self.dot_visible = true;
                self.cache.clear();
                Task::perform(
                    tokio::time::sleep(Duration::from_millis(400)),
                    |_| Message::DotOff,
                )
            }
            Message::DotOff => {
                self.dot_visible = false;
                self.cache.clear();
                Task::none()
            }
            Message::Animate => {
                let target = self.target_frac();
                let delta = target - self.displayed_frac;
                if delta.abs() > 0.0005 {
                    self.displayed_frac += delta * 0.12;
                    self.cache.clear();
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        iced::widget::canvas(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_secs(2)).map(|_| Message::Tick),
            iced::time::every(Duration::from_secs(30)).map(|_| Message::Reload),
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Animate),
        ])
    }
}

impl canvas::Program<Message> for ClaudeMeter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let data = self.data.clone();
        let dot_visible = self.dot_visible;
        let displayed_frac = self.displayed_frac;

        let geo = self.cache.draw(renderer, bounds.size(), move |frame| {
            frame.fill(&Path::rectangle(Point::ORIGIN, frame.size()), COLOR_BG);

            let w = frame.width();
            let h = frame.height();
            let cx = w / 2.0;
            let cy = h * 0.38;
            let center = Point::new(cx, cy);
            let radius = w.min(h) * 0.30;

            // Track (background arc)
            frame.stroke(
                &arc_path(center, radius, START_ANGLE, START_ANGLE + TOTAL_SWEEP),
                Stroke::default().with_color(COLOR_TRACK).with_width(8.0),
            );

            // Gradient fill arc — drawn as GRADIENT_SEGS small segments
            let seg_sweep = TOTAL_SWEEP / GRADIENT_SEGS as f32;
            let filled_segs = (displayed_frac * GRADIENT_SEGS as f32).ceil() as usize;
            for i in 0..filled_segs {
                // position t ∈ [0,1] across the full arc range
                let t = (i as f32 + 0.5) / GRADIENT_SEGS as f32;
                let seg_start = START_ANGLE + i as f32 * seg_sweep;
                // last segment clips to exact displayed_frac
                let seg_end = if i + 1 >= filled_segs {
                    START_ANGLE + displayed_frac * TOTAL_SWEEP
                } else {
                    seg_start + seg_sweep
                };
                if seg_end > seg_start {
                    frame.stroke(
                        &arc_path(center, radius, seg_start, seg_end),
                        Stroke::default().with_color(arc_gradient(t)).with_width(8.0),
                    );
                }
            }

            // Center: token count
            frame.fill_text(canvas::Text {
                content: fmt_tokens(data.tokens_session),
                position: Point::new(cx, cy - radius * 0.06),
                color: Color::WHITE,
                size: Pixels(radius * 0.50),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..canvas::Text::default()
            });

            frame.fill_text(canvas::Text {
                content: "THIS SESSION".to_string(),
                position: Point::new(cx, cy + radius * 0.24),
                color: COLOR_DIM,
                size: Pixels(radius * 0.13),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Top,
                ..canvas::Text::default()
            });

            frame.fill_text(canvas::Text {
                content: format!("Out of {} Tokens", fmt_tokens(SESSION_CAP as u64)),
                position: Point::new(cx, cy + radius * 0.42),
                color: Color { r: 0.30, g: 0.30, b: 0.30, a: 1.0 },
                size: Pixels(radius * 0.11),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Top,
                ..canvas::Text::default()
            });

            let bw = (w - 50.0) / 2.0;
            let bh = h * 0.20;
            let by = h * 0.72;
            let b1x = 15.0;
            let b2x = b1x + bw + 20.0;

            frame.fill(
                &Path::rectangle(Point::new(b1x, by), Size::new(bw, bh)),
                COLOR_BLOCK,
            );
            frame.fill_text(canvas::Text {
                content: format_time(data.remaining_secs),
                position: Point::new(b1x + bw / 2.0, by + bh * 0.34),
                color: Color::WHITE,
                size: Pixels(bh * 0.42),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
            frame.fill_text(canvas::Text {
                content: "UNTIL RESET".to_string(),
                position: Point::new(b1x + bw / 2.0, by + bh * 0.72),
                color: COLOR_DIM,
                size: Pixels(bh * 0.20),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..canvas::Text::default()
            });

            let weekly_pct = (data.tokens_this_week as f32 / WEEKLY_CAP * 100.0).min(100.0);
            frame.fill(
                &Path::rectangle(Point::new(b2x, by), Size::new(bw, bh)),
                COLOR_BLOCK,
            );
            frame.fill_text(canvas::Text {
                content: format!("{:.0}%", weekly_pct),
                position: Point::new(b2x + bw / 2.0, by + bh * 0.34),
                color: Color::WHITE,
                size: Pixels(bh * 0.42),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
            frame.fill_text(canvas::Text {
                content: "WEEKLY TOKENS".to_string(),
                position: Point::new(b2x + bw / 2.0, by + bh * 0.72),
                color: COLOR_DIM,
                size: Pixels(bh * 0.20),
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..canvas::Text::default()
            });

            // Sparkline — 7-day bar chart below info blocks
            let sp_x = 15.0;
            let sp_w = w - 30.0;
            let sp_y = by + bh + 10.0;
            let sp_h = h - sp_y - 8.0;
            let label_h = 11.0;
            let bar_area_h = sp_h - label_h;
            let bar_gap = 3.0;
            let bar_w = (sp_w - 6.0 * bar_gap) / 7.0;
            let max_daily = data.daily_tokens.iter().copied().max().unwrap_or(1).max(1) as f32;
            let now_day = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
                .as_secs() / 86400;
            let day_names = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
            for i in 0..7usize {
                let bx = sp_x + i as f32 * (bar_w + bar_gap);
                let tokens = data.daily_tokens[i] as f32;
                let frac = tokens / max_daily;
                let bar_h = (frac * bar_area_h).max(1.0);
                let bar_y = sp_y + bar_area_h - bar_h;
                let bar_color = if tokens < 1.0 {
                    Color { r: 0.13, g: 0.13, b: 0.13, a: 1.0 }
                } else {
                    arc_gradient(frac)
                };
                frame.fill(
                    &Path::rectangle(Point::new(bx, bar_y), Size::new(bar_w, bar_h)),
                    bar_color,
                );
                let day_num = now_day.saturating_sub(6 - i as u64);
                let dow = ((day_num % 7) + 4) % 7;
                let label = day_names[dow as usize];
                let label_color = if i == 6 { Color::WHITE } else { COLOR_DIM };
                frame.fill_text(canvas::Text {
                    content: label.to_string(),
                    position: Point::new(bx + bar_w / 2.0, sp_y + sp_h - label_h / 2.0),
                    color: label_color,
                    size: Pixels(9.0),
                    horizontal_alignment: alignment::Horizontal::Center,
                    vertical_alignment: alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }

            let dot_color = if dot_visible {
                COLOR_RED
            } else {
                Color { r: 0.18, g: 0.08, b: 0.08, a: 1.0 }
            };
            frame.fill(
                &Path::circle(Point::new(w - 14.0, 14.0), 5.0),
                dot_color,
            );
        });
        vec![geo]
    }
}

fn arc_path(center: Point, radius: f32, start: f32, end: f32) -> Path {
    use iced::widget::canvas::path::arc;
    Path::new(|b| {
        b.arc(arc::Arc {
            center,
            radius,
            start_angle: Radians(start),
            end_angle: Radians(end),
        });
    })
}

/// Green → Yellow → Red across t ∈ [0, 1]
fn arc_gradient(t: f32) -> Color {
    let (a, b, s) = if t < 0.5 {
        (COLOR_GREEN, COLOR_YELLOW, t * 2.0)
    } else {
        (COLOR_YELLOW, COLOR_RED, (t - 0.5) * 2.0)
    };
    Color { r: a.r + (b.r - a.r) * s, g: a.g + (b.g - a.g) * s, b: a.b + (b.b - a.b) * s, a: 1.0 }
}

fn format_time(secs: u64) -> String {
    (chrono::Local::now() + chrono::Duration::seconds(secs as i64)).format("%-I:%M %p").to_string()
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn main() -> iced::Result {
    use iced::window;
    iced::application("Claude Meter", ClaudeMeter::update, ClaudeMeter::view)
        .subscription(ClaudeMeter::subscription)
        .window(window::Settings {
            size: Size::new(380.0, 390.0),
            resizable: false,
            ..window::Settings::default()
        })
        .antialiasing(true)
        .run()
}
