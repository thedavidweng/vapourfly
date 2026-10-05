//! Controller input for the Steam Deck and other gamepads.
//!
//! A background thread owns [`gilrs::Gilrs`] (it is not `Send`), turns raw
//! button and axis events into [`NavCommand`]s through [`NavMapper`], and
//! sends them over a channel. The UI interprets the commands: directions
//! move focus, A activates, B backs out, bumpers switch pages.
//!
//! The thread blocks on the device until an event arrives, and only wakes on
//! a timer while a direction is held (key repeat) or the right stick is
//! deflected (smooth scroll), so an idle controller costs no CPU.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// A focus or scroll direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// What the UI should do in response to controller input.
#[derive(Clone, Debug, PartialEq)]
pub enum NavCommand {
    /// D-pad or left stick.
    Move(Direction),
    /// A: press the focused control.
    Activate,
    /// B: close the top overlay, else return to the sidebar.
    Back,
    /// Y: jump to the Library search field.
    Search,
    /// X: show Steam's on-screen keyboard.
    Keyboard,
    /// LB / RB: previous / next sidebar destination.
    PrevView,
    NextView,
    /// LT / RT: scroll a page up (`-1`) or down (`1`).
    Page(i8),
    /// Right stick: scroll by this many pixels (positive is down).
    Scroll(f32),
    /// Start (≡): open Settings.
    Menu,
    /// Select (⧉): show or hide the sidebar.
    ToggleSidebar,
    /// A usable controller was connected; carries its name.
    Connected(String),
    /// A controller went away.
    Disconnected,
}

/// Toolkit-neutral button identity, mapped from gilrs in the reader thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadButton {
    South,
    East,
    North,
    West,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Start,
    Select,
    DPad(Direction),
}

/// Stick axes the mapper consumes. Y is positive up, as gilrs reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadAxis {
    LeftX,
    LeftY,
    RightY,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadInput {
    Button(PadButton, bool),
    Axis(PadAxis, f32),
}

/// Delay before a held direction starts repeating.
pub const REPEAT_DELAY: Duration = Duration::from_millis(380);
/// Interval between repeats while held.
pub const REPEAT_INTERVAL: Duration = Duration::from_millis(110);
/// Right-stick scroll tick (about 60 Hz).
pub const SCROLL_TICK: Duration = Duration::from_millis(16);

/// Left-stick deflection that counts as a direction press, and the lower
/// level it must fall below to release (hysteresis avoids chatter).
const STICK_PRESS: f32 = 0.6;
const STICK_RELEASE: f32 = 0.35;
/// Right-stick dead zone and full-deflection scroll speed.
const SCROLL_DEADZONE: f32 = 0.18;
const SCROLL_MAX_PX_PER_SEC: f32 = 2200.;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Repeatable {
    Move(Direction),
    Page(i8),
}

impl Repeatable {
    fn command(self) -> NavCommand {
        match self {
            Self::Move(d) => NavCommand::Move(d),
            Self::Page(p) => NavCommand::Page(p),
        }
    }
}

/// Turns button / axis changes into navigation commands with key repeat,
/// stick hysteresis and smooth right-stick scrolling. Pure state machine:
/// time is passed in so it is deterministic under test.
#[derive(Debug, Default)]
pub struct NavMapper {
    dpad: [bool; 4],
    stick_x: f32,
    stick_y: f32,
    stick_dir: Option<Direction>,
    triggers: [bool; 2],
    repeat: Option<(Repeatable, Instant)>,
    scroll_y: f32,
    last_scroll: Option<Instant>,
}

impl NavMapper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one input change and return the commands it produces right away.
    pub fn input(&mut self, input: PadInput, now: Instant) -> Vec<NavCommand> {
        let mut out = Vec::new();
        match input {
            PadInput::Button(button, pressed) => self.button(button, pressed, now, &mut out),
            PadInput::Axis(PadAxis::LeftX, v) => {
                self.stick_x = v;
                self.update_stick(now, &mut out);
            }
            PadInput::Axis(PadAxis::LeftY, v) => {
                self.stick_y = v;
                self.update_stick(now, &mut out);
            }
            PadInput::Axis(PadAxis::RightY, v) => {
                let was_idle = self.scroll_y == 0.;
                self.scroll_y = if v.abs() < SCROLL_DEADZONE { 0. } else { v };
                if was_idle && self.scroll_y != 0. {
                    self.last_scroll = Some(now);
                } else if self.scroll_y == 0. {
                    self.last_scroll = None;
                }
            }
        }
        out
    }

    /// Emit any repeats or scroll steps due at `now`.
    pub fn tick(&mut self, now: Instant) -> Vec<NavCommand> {
        let mut out = Vec::new();
        if let Some((what, due)) = self.repeat {
            if now >= due {
                out.push(what.command());
                self.repeat = Some((what, now + REPEAT_INTERVAL));
            }
        }
        if self.scroll_y != 0. {
            if let Some(last) = self.last_scroll {
                let dt = now.saturating_duration_since(last).as_secs_f32();
                if dt > 0. {
                    // Quadratic response: fine control near center, fast at the rim.
                    let magnitude =
                        (self.scroll_y.abs() - SCROLL_DEADZONE) / (1. - SCROLL_DEADZONE);
                    let speed = magnitude.clamp(0., 1.).powi(2) * SCROLL_MAX_PX_PER_SEC;
                    // Stick down (negative y) scrolls the content down.
                    let px = -self.scroll_y.signum() * speed * dt;
                    if px != 0. {
                        out.push(NavCommand::Scroll(px));
                    }
                    self.last_scroll = Some(now);
                }
            }
        }
        out
    }

    /// How long the reader may block before [`Self::tick`] has work, or
    /// `None` when nothing is pending.
    pub fn next_deadline(&self, now: Instant) -> Option<Duration> {
        let repeat = self
            .repeat
            .map(|(_, due)| due.saturating_duration_since(now));
        let scroll = (self.scroll_y != 0.).then_some(SCROLL_TICK);
        match (repeat, scroll) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn button(
        &mut self,
        button: PadButton,
        pressed: bool,
        now: Instant,
        out: &mut Vec<NavCommand>,
    ) {
        match button {
            PadButton::DPad(d) => {
                self.dpad[d.index()] = pressed;
                if pressed {
                    self.press(Repeatable::Move(d), now, out);
                } else {
                    self.release_move(d);
                }
            }
            PadButton::LeftTrigger | PadButton::RightTrigger => {
                let (slot, page) = if button == PadButton::LeftTrigger {
                    (0, -1)
                } else {
                    (1, 1)
                };
                let was = self.triggers[slot];
                self.triggers[slot] = pressed;
                if pressed && !was {
                    self.press(Repeatable::Page(page), now, out);
                } else if !pressed
                    && self
                        .repeat
                        .is_some_and(|(r, _)| r == Repeatable::Page(page))
                {
                    self.repeat = None;
                }
            }
            _ if !pressed => {}
            PadButton::South => out.push(NavCommand::Activate),
            PadButton::East => out.push(NavCommand::Back),
            PadButton::North => out.push(NavCommand::Search),
            PadButton::West => out.push(NavCommand::Keyboard),
            PadButton::LeftBumper => out.push(NavCommand::PrevView),
            PadButton::RightBumper => out.push(NavCommand::NextView),
            PadButton::Start => out.push(NavCommand::Menu),
            PadButton::Select => out.push(NavCommand::ToggleSidebar),
        }
    }

    fn press(&mut self, what: Repeatable, now: Instant, out: &mut Vec<NavCommand>) {
        out.push(what.command());
        self.repeat = Some((what, now + REPEAT_DELAY));
    }

    fn release_move(&mut self, d: Direction) {
        let still_held = self.dpad[d.index()] || self.stick_dir == Some(d);
        if !still_held && self.repeat.is_some_and(|(r, _)| r == Repeatable::Move(d)) {
            self.repeat = None;
        }
    }

    fn update_stick(&mut self, now: Instant, out: &mut Vec<NavCommand>) {
        let (x, y) = (self.stick_x, self.stick_y);
        let along = |d: Direction| match d {
            Direction::Up => y,
            Direction::Down => -y,
            Direction::Left => -x,
            Direction::Right => x,
        };
        if let Some(current) = self.stick_dir {
            if along(current) >= STICK_RELEASE {
                return;
            }
            self.stick_dir = None;
            self.release_move(current);
        }
        let pick = Direction::ALL
            .into_iter()
            .map(|d| (d, along(d)))
            .filter(|(_, v)| *v >= STICK_PRESS)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(d, _)| d);
        if let Some(d) = pick {
            self.stick_dir = Some(d);
            self.press(Repeatable::Move(d), now, out);
        }
    }
}

/// Steam's controller hand-off: for games it launches with Steam Input on,
/// Steam lists the physical pads in `SDL_GAMECONTROLLER_IGNORE_DEVICES` so
/// only its virtual gamepad is read. Honouring it avoids double input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceFilter {
    ignore: Vec<(u16, u16)>,
    only: Option<Vec<(u16, u16)>>,
}

impl DeviceFilter {
    pub fn from_env() -> Self {
        Self::parse(
            std::env::var("SDL_GAMECONTROLLER_IGNORE_DEVICES")
                .ok()
                .as_deref(),
            std::env::var("SDL_GAMECONTROLLER_IGNORE_DEVICES_EXCEPT")
                .ok()
                .as_deref(),
        )
    }

    /// Parse the SDL lists (`0x28de/0x1205,0x045e/0x028e`).
    pub fn parse(ignore: Option<&str>, except: Option<&str>) -> Self {
        Self {
            ignore: ignore.map(parse_id_list).unwrap_or_default(),
            only: except.map(parse_id_list).filter(|l| !l.is_empty()),
        }
    }

    /// Whether a pad with these USB ids should drive the UI. Pads that do
    /// not report ids are always allowed.
    pub fn allows(&self, vendor: Option<u16>, product: Option<u16>) -> bool {
        let (Some(v), Some(p)) = (vendor, product) else {
            return true;
        };
        if let Some(only) = &self.only {
            return only.contains(&(v, p));
        }
        !self.ignore.contains(&(v, p))
    }
}

fn parse_id_list(list: &str) -> Vec<(u16, u16)> {
    let hex = |s: &str| {
        let s = s.trim();
        let s = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        u16::from_str_radix(s, 16).ok()
    };
    list.split(',')
        .filter_map(|pair| {
            let (v, p) = pair.split_once('/')?;
            Some((hex(v)?, hex(p)?))
        })
        .collect()
}

/// Start the reader thread. Returns `None` when controller input is
/// disabled with `VAPOURFLY_NO_GAMEPAD=1`. If the platform has no gamepad
/// backend the thread exits and the channel closes.
pub fn spawn() -> Option<Receiver<NavCommand>> {
    if std::env::var("VAPOURFLY_NO_GAMEPAD").is_ok_and(|v| v.trim() == "1") {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("vapourfly-gamepad".into())
        .spawn(move || reader(tx))
        .ok()?;
    Some(rx)
}

fn reader(tx: Sender<NavCommand>) {
    use std::collections::HashMap;

    use gilrs::{EventType, GamepadId, GilrsBuilder};

    let Ok(mut gilrs) = GilrsBuilder::new().set_update_state(false).build() else {
        return;
    };
    let filter = DeviceFilter::from_env();
    let mut allowed: HashMap<GamepadId, bool> = HashMap::new();
    let mut connected = 0usize;
    for (id, pad) in gilrs.gamepads() {
        let ok = filter.allows(pad.vendor_id(), pad.product_id());
        allowed.insert(id, ok);
        if ok {
            connected += 1;
            if tx
                .send(NavCommand::Connected(pad.name().to_string()))
                .is_err()
            {
                return;
            }
        }
    }

    let mut mapper = NavMapper::new();
    loop {
        let timeout = mapper.next_deadline(Instant::now());
        let mut commands = Vec::new();
        if let Some(ev) = gilrs.next_event_blocking(timeout) {
            let ok = *allowed.entry(ev.id).or_insert_with(|| {
                let pad = gilrs.gamepad(ev.id);
                filter.allows(pad.vendor_id(), pad.product_id())
            });
            if ok {
                let now = Instant::now();
                match ev.event {
                    EventType::Connected => {
                        connected += 1;
                        commands.push(NavCommand::Connected(
                            gilrs.gamepad(ev.id).name().to_string(),
                        ));
                    }
                    EventType::Disconnected => {
                        connected = connected.saturating_sub(1);
                        mapper = NavMapper::new();
                        if connected == 0 {
                            commands.push(NavCommand::Disconnected);
                        }
                    }
                    EventType::ButtonPressed(b, _) => {
                        if let Some(b) = map_button(b) {
                            commands.extend(mapper.input(PadInput::Button(b, true), now));
                        }
                    }
                    EventType::ButtonReleased(b, _) => {
                        if let Some(b) = map_button(b) {
                            commands.extend(mapper.input(PadInput::Button(b, false), now));
                        }
                    }
                    EventType::AxisChanged(a, v, _) => {
                        if let Some(a) = map_axis(a) {
                            commands.extend(mapper.input(PadInput::Axis(a, v), now));
                        }
                    }
                    _ => {}
                }
            }
            if matches!(ev.event, EventType::Disconnected) {
                allowed.remove(&ev.id);
            }
        }
        commands.extend(mapper.tick(Instant::now()));
        for command in commands {
            if tx.send(command).is_err() {
                return;
            }
        }
    }
}

fn map_button(b: gilrs::Button) -> Option<PadButton> {
    use gilrs::Button as B;
    Some(match b {
        B::South => PadButton::South,
        B::East => PadButton::East,
        B::North => PadButton::North,
        B::West => PadButton::West,
        B::LeftTrigger => PadButton::LeftBumper,
        B::RightTrigger => PadButton::RightBumper,
        B::LeftTrigger2 => PadButton::LeftTrigger,
        B::RightTrigger2 => PadButton::RightTrigger,
        B::Start => PadButton::Start,
        B::Select => PadButton::Select,
        B::DPadUp => PadButton::DPad(Direction::Up),
        B::DPadDown => PadButton::DPad(Direction::Down),
        B::DPadLeft => PadButton::DPad(Direction::Left),
        B::DPadRight => PadButton::DPad(Direction::Right),
        _ => return None,
    })
}

fn map_axis(a: gilrs::Axis) -> Option<PadAxis> {
    match a {
        gilrs::Axis::LeftStickX => Some(PadAxis::LeftX),
        gilrs::Axis::LeftStickY => Some(PadAxis::LeftY),
        gilrs::Axis::RightStickY => Some(PadAxis::RightY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Vec<NavCommand> = Vec::new();

    fn press(m: &mut NavMapper, b: PadButton, at: Instant) -> Vec<NavCommand> {
        m.input(PadInput::Button(b, true), at)
    }

    fn release(m: &mut NavMapper, b: PadButton, at: Instant) -> Vec<NavCommand> {
        m.input(PadInput::Button(b, false), at)
    }

    #[test]
    fn face_buttons_fire_once_on_press() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        assert_eq!(
            press(&mut m, PadButton::South, t),
            vec![NavCommand::Activate]
        );
        assert_eq!(release(&mut m, PadButton::South, t), NONE);
        assert_eq!(press(&mut m, PadButton::East, t), vec![NavCommand::Back]);
        assert_eq!(press(&mut m, PadButton::North, t), vec![NavCommand::Search]);
        assert_eq!(
            press(&mut m, PadButton::West, t),
            vec![NavCommand::Keyboard]
        );
        assert_eq!(
            press(&mut m, PadButton::LeftBumper, t),
            vec![NavCommand::PrevView]
        );
        assert_eq!(
            press(&mut m, PadButton::RightBumper, t),
            vec![NavCommand::NextView]
        );
        assert_eq!(press(&mut m, PadButton::Start, t), vec![NavCommand::Menu]);
        assert_eq!(
            press(&mut m, PadButton::Select, t),
            vec![NavCommand::ToggleSidebar]
        );
        assert!(m.next_deadline(t).is_none(), "face buttons never repeat");
    }

    #[test]
    fn held_dpad_repeats_after_delay_and_stops_on_release() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        let down = PadButton::DPad(Direction::Down);
        assert_eq!(
            press(&mut m, down, t),
            vec![NavCommand::Move(Direction::Down)]
        );
        assert_eq!(m.tick(t + REPEAT_DELAY / 2), NONE);
        assert_eq!(
            m.tick(t + REPEAT_DELAY),
            vec![NavCommand::Move(Direction::Down)]
        );
        assert_eq!(m.tick(t + REPEAT_DELAY + REPEAT_INTERVAL / 2), NONE);
        assert_eq!(
            m.tick(t + REPEAT_DELAY + REPEAT_INTERVAL),
            vec![NavCommand::Move(Direction::Down)]
        );
        release(&mut m, down, t + REPEAT_DELAY * 2);
        assert_eq!(m.tick(t + REPEAT_DELAY * 4), NONE);
        assert!(m.next_deadline(t).is_none());
    }

    #[test]
    fn newest_direction_owns_the_repeat() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        press(&mut m, PadButton::DPad(Direction::Down), t);
        press(&mut m, PadButton::DPad(Direction::Right), t);
        // Releasing the older direction keeps the newer one repeating.
        release(&mut m, PadButton::DPad(Direction::Down), t);
        assert_eq!(
            m.tick(t + REPEAT_DELAY),
            vec![NavCommand::Move(Direction::Right)]
        );
    }

    #[test]
    fn left_stick_has_hysteresis() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        assert_eq!(m.input(PadInput::Axis(PadAxis::LeftY, -0.4), t), NONE);
        assert_eq!(
            m.input(PadInput::Axis(PadAxis::LeftY, -0.8), t),
            vec![NavCommand::Move(Direction::Down)]
        );
        // Easing off but above the release level does not re-fire.
        assert_eq!(m.input(PadInput::Axis(PadAxis::LeftY, -0.45), t), NONE);
        assert_eq!(m.input(PadInput::Axis(PadAxis::LeftY, -0.1), t), NONE);
        assert!(m.next_deadline(t).is_none());
        assert_eq!(
            m.input(PadInput::Axis(PadAxis::LeftX, 0.9), t),
            vec![NavCommand::Move(Direction::Right)]
        );
    }

    #[test]
    fn stick_up_is_positive_y() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        assert_eq!(
            m.input(PadInput::Axis(PadAxis::LeftY, 0.95), t),
            vec![NavCommand::Move(Direction::Up)]
        );
    }

    #[test]
    fn triggers_page_with_repeat() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        assert_eq!(
            press(&mut m, PadButton::RightTrigger, t),
            vec![NavCommand::Page(1)]
        );
        // Analog triggers can report pressed again while held.
        assert_eq!(press(&mut m, PadButton::RightTrigger, t), NONE);
        assert_eq!(m.tick(t + REPEAT_DELAY), vec![NavCommand::Page(1)]);
        release(&mut m, PadButton::RightTrigger, t);
        assert_eq!(m.tick(t + REPEAT_DELAY * 3), NONE);
        assert_eq!(
            press(&mut m, PadButton::LeftTrigger, t),
            vec![NavCommand::Page(-1)]
        );
    }

    #[test]
    fn right_stick_scrolls_smoothly_and_stops_in_deadzone() {
        let mut m = NavMapper::new();
        let t = Instant::now();
        assert_eq!(m.input(PadInput::Axis(PadAxis::RightY, 0.1), t), NONE);
        assert!(m.next_deadline(t).is_none(), "inside the dead zone");
        m.input(PadInput::Axis(PadAxis::RightY, -1.0), t);
        assert_eq!(m.next_deadline(t), Some(SCROLL_TICK));
        let step = m.tick(t + Duration::from_millis(100));
        let [NavCommand::Scroll(px)] = step.as_slice() else {
            panic!("expected one scroll step, got {step:?}");
        };
        assert!(
            (*px - SCROLL_MAX_PX_PER_SEC * 0.1).abs() < 1.,
            "full deflection down scrolls down at max speed: {px}"
        );
        m.input(
            PadInput::Axis(PadAxis::RightY, 0.5),
            t + Duration::from_millis(100),
        );
        let step = m.tick(t + Duration::from_millis(200));
        assert!(matches!(step.as_slice(), [NavCommand::Scroll(px)] if *px < 0.));
        m.input(PadInput::Axis(PadAxis::RightY, 0.0), t);
        assert_eq!(m.tick(t + Duration::from_secs(1)), NONE);
    }

    #[test]
    fn device_filter_honours_steam_lists() {
        let f = DeviceFilter::parse(Some("0x28de/0x1205, 0x045E/0x028E,junk"), None);
        assert!(!f.allows(Some(0x28de), Some(0x1205)));
        assert!(!f.allows(Some(0x045e), Some(0x028e)));
        assert!(f.allows(Some(0x28de), Some(0x11ff)));
        assert!(f.allows(None, None), "pads without ids are allowed");

        let only = DeviceFilter::parse(Some("0x28de/0x1205"), Some("0x28de/0x11ff"));
        assert!(only.allows(Some(0x28de), Some(0x11ff)));
        assert!(!only.allows(Some(0x045e), Some(0x028e)));

        assert_eq!(DeviceFilter::parse(None, Some("")), DeviceFilter::default());
    }
}
