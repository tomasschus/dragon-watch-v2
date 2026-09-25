use core::fmt::Write as _;
use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle, RoundedRectangle},
    text::{Alignment, Text},
};

use super::{AppAction, AppDescriptor, AppDisplay, AppState, PowerProfile, TouchEvent};
use crate::MyDisplay;

const SCREEN_W: i32 = 410;
const SCREEN_H: i32 = 502;
const FIELD_X: i32 = 10;
const FIELD_Y: i32 = 66;
const FIELD_W: i32 = SCREEN_W - FIELD_X * 2;
const FIELD_H: i32 = 420;
const FIELD_BOTTOM: i32 = FIELD_Y + FIELD_H;
const PADDLE_W: i32 = 92;
const PADDLE_H: i32 = 14;
const PLAYER_Y: i32 = FIELD_BOTTOM - 34;
const CPU_Y: i32 = FIELD_Y + 20;
const BALL: i32 = 16;
const WIN_SCORE: i32 = 7;
const SERVE_FREEZE: i32 = 16;
const BALL_SPEED: i32 = 14;

const BLACK: Rgb888 = Rgb888::new(0, 0, 0);
const WHITE: Rgb888 = Rgb888::new(255, 255, 255);
const GREY: Rgb888 = Rgb888::new(138, 138, 142);
const FIELD_BG: Rgb888 = Rgb888::new(18, 20, 24);
const NET: Rgb888 = Rgb888::new(44, 47, 54);
const PLAYER: Rgb888 = Rgb888::new(74, 200, 255);
const CPU: Rgb888 = Rgb888::new(255, 122, 69);

// Slots del AppState: pelota x/y, velocidad x/y, paletas, marcador,
// pausa/fin, saque, RNG, posiciones anteriores, redibujado y toque inicial.
const BX: usize = 0;
const BY: usize = 1;
const VX: usize = 2;
const VY: usize = 3;
const PLAYER_X: usize = 4;
const CPU_X: usize = 5;
const PLAYER_SCORE: usize = 6;
const CPU_SCORE: usize = 7;
const PAUSED: usize = 8;
const OVER: usize = 9;
const SERVE: usize = 10;
const RNG: usize = 11;
const PREV_BX: usize = 12;
const PREV_BY: usize = 13;
const PREV_PLAYER_X: usize = 14;
const PREV_CPU_X: usize = 15;
const NEEDS_FULL: usize = 16;
const TOUCH_X: usize = 17;
const TOUCH_Y: usize = 18;
const FRAME_DIVIDER: usize = 19;

#[derive(Clone, Copy)]
struct Rect { x0: i32, y0: i32, x1: i32, y1: i32 }

impl Rect {
    fn union(self, other: Self) -> Self {
        Self { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }
    fn intersects(self, other: Self) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }
    fn draw(self, display: &mut MyDisplay, color: Rgb888) {
        Rectangle::new(Point::new(self.x0, self.y0), Size::new((self.x1 - self.x0 + 1) as u32, (self.y1 - self.y0 + 1) as u32))
            .into_styled(PrimitiveStyle::with_fill(color)).draw(display).ok();
    }
    fn clip_to_screen(self) -> Self {
        Self { x0: self.x0.max(0), y0: self.y0.max(0), x1: self.x1.min(SCREEN_W - 1), y1: self.y1.min(SCREEN_H - 1) }
    }
}

fn ball_rect(x: i32, y: i32) -> Rect { Rect { x0: x, y0: y, x1: x + BALL - 1, y1: y + BALL - 1 } }
fn paddle_rect(center: i32, y: i32) -> Rect {
    Rect { x0: center - PADDLE_W / 2, y0: y, x1: center + PADDLE_W / 2 - 1, y1: y + PADDLE_H - 1 }
}
fn field_rect() -> Rect { Rect { x0: FIELD_X, y0: FIELD_Y, x1: FIELD_X + FIELD_W - 1, y1: FIELD_BOTTOM - 1 } }
fn net_y() -> i32 { FIELD_Y + FIELD_H / 2 - 2 }

fn draw_icon(display: &mut MyDisplay, center: Point) {
    Rectangle::new(Point::new(center.x - 28, center.y - 20), Size::new(56, 40))
        .into_styled(PrimitiveStyle::with_stroke(WHITE, 3)).draw(display).ok();
    Rectangle::new(Point::new(center.x - 21, center.y + 4), Size::new(10, 4))
        .into_styled(PrimitiveStyle::with_fill(PLAYER)).draw(display).ok();
    Circle::new(Point::new(center.x + 4, center.y - 5), 10)
        .into_styled(PrimitiveStyle::with_fill(WHITE)).draw(display).ok();
}

fn next_random(state: &mut AppState) -> u32 {
    let mut value = state.data[RNG] as u32 | 1;
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    state.data[RNG] = value as i32;
    value
}

fn serve(state: &mut AppState, toward_player: bool) {
    state.data[BX] = SCREEN_W / 2 - BALL / 2;
    state.data[BY] = FIELD_Y + FIELD_H / 2 - BALL / 2;
    state.data[VX] = (next_random(state) % 9) as i32 - 4;
    state.data[VY] = if toward_player { BALL_SPEED } else { -BALL_SPEED };
    state.data[SERVE] = SERVE_FREEZE;
    state.data[NEEDS_FULL] = 1;
}

fn initialize(state: &mut AppState) {
    state.data = [0; 24];
    state.data[PLAYER_X] = SCREEN_W / 2;
    state.data[CPU_X] = SCREEN_W / 2;
    state.data[RNG] = 0x51f1_2025;
    serve(state, true);
    state.data[PREV_BX] = state.data[BX];
    state.data[PREV_BY] = state.data[BY];
    state.data[PREV_PLAYER_X] = state.data[PLAYER_X];
    state.data[PREV_CPU_X] = state.data[CPU_X];
}

fn draw_paddle(display: &mut MyDisplay, rect: Rect, color: Rgb888) {
    RoundedRectangle::with_equal_corners(
        Rectangle::new(Point::new(rect.x0, rect.y0), Size::new((rect.x1 - rect.x0 + 1) as u32, (rect.y1 - rect.y0 + 1) as u32)),
        Size::new(7, 7),
    ).into_styled(PrimitiveStyle::with_fill(color)).draw(display).ok();
}

fn draw_pieces(display: &mut MyDisplay, state: &AppState) {
    draw_paddle(display, paddle_rect(state.data[CPU_X], CPU_Y), CPU);
    draw_paddle(display, paddle_rect(state.data[PLAYER_X], PLAYER_Y), PLAYER);
    RoundedRectangle::with_equal_corners(
        Rectangle::new(Point::new(state.data[BX], state.data[BY]), Size::new(BALL as u32, BALL as u32)),
        Size::new(8, 8),
    ).into_styled(PrimitiveStyle::with_fill(WHITE)).draw(display).ok();
}

fn draw_full(display: &mut MyDisplay, state: &AppState) {
    display.clear(BLACK).ok();
    let text = MonoTextStyle::new(&FONT_10X20, WHITE);
    let mut score: heapless::String<16> = heapless::String::new();
    let _ = write!(score, "{}  -  {}", state.data[CPU_SCORE], state.data[PLAYER_SCORE]);
    Text::with_alignment(&score, Point::new(SCREEN_W / 2, 45), text, Alignment::Center).draw(display).ok();

    RoundedRectangle::with_equal_corners(
        Rectangle::new(Point::new(FIELD_X - 4, FIELD_Y - 4), Size::new((FIELD_W + 8) as u32, (FIELD_H + 8) as u32)),
        Size::new(14, 14),
    ).into_styled(PrimitiveStyle::with_fill(FIELD_BG)).draw(display).ok();
    Rectangle::new(Point::new(FIELD_X, FIELD_Y), Size::new(FIELD_W as u32, FIELD_H as u32))
        .into_styled(PrimitiveStyle::with_fill(FIELD_BG)).draw(display).ok();
    draw_net(display, field_rect());
    draw_pieces(display, state);

    if state.data[OVER] != 0 || state.data[PAUSED] != 0 {
        let y = FIELD_Y + FIELD_H / 2 - 62;
        Rectangle::new(Point::new(0, y), Size::new(SCREEN_W as u32, 124))
            .into_styled(PrimitiveStyle::with_fill(BLACK)).draw(display).ok();
        let title = if state.data[OVER] != 0 {
            if state.data[PLAYER_SCORE] > state.data[CPU_SCORE] { "Ganaste" } else { "Perdiste" }
        } else { "Pausa" };
        let detail = if state.data[OVER] != 0 { "Toca para revancha" } else { "Toca arriba para seguir" };
        Text::with_alignment(title, Point::new(SCREEN_W / 2, y + 40), text, Alignment::Center).draw(display).ok();
        Text::with_alignment(detail, Point::new(SCREEN_W / 2, y + 75), MonoTextStyle::new(&FONT_10X20, GREY), Alignment::Center).draw(display).ok();
        Text::with_alignment("Desliza derecha para salir", Point::new(SCREEN_W / 2, y + 105), MonoTextStyle::new(&FONT_10X20, GREY), Alignment::Center).draw(display).ok();
    }
    display.refresh_all();
}

fn draw_net(display: &mut MyDisplay, clip: Rect) {
    let y = net_y();
    if clip.y0 > y + 3 || clip.y1 < y { return; }
    let mut x = FIELD_X + 6;
    while x < FIELD_X + FIELD_W - 12 {
        let segment = Rect { x0: x, y0: y, x1: x + 15, y1: y + 3 };
        if segment.intersects(clip) { segment.draw(display, NET); }
        x += 28;
    }
}

// Reconstruye el fondo y las líneas estáticas dentro del área sucia antes de
// volver a dibujar las piezas completas. Así no quedan rastros al cruzar la red.
fn repaint(display: &mut MyDisplay, state: &AppState, rect: Rect) {
    let clip = rect.clip_to_screen();
    clip.draw(display, BLACK);
    let field = field_rect();
    if clip.intersects(field) {
        Rect { x0: clip.x0.max(field.x0), y0: clip.y0.max(field.y0), x1: clip.x1.min(field.x1), y1: clip.y1.min(field.y1) }.draw(display, FIELD_BG);
    }
    draw_net(display, clip);
    draw_pieces(display, state);
}

fn render(display: &mut MyDisplay, state: &mut AppState) {
    if state.data[NEEDS_FULL] != 0 {
        draw_full(display, state);
        state.data[NEEDS_FULL] = 0;
    } else {
        let dirty = [
            ball_rect(state.data[PREV_BX], state.data[PREV_BY]).union(ball_rect(state.data[BX], state.data[BY])),
            paddle_rect(state.data[PREV_PLAYER_X], PLAYER_Y).union(paddle_rect(state.data[PLAYER_X], PLAYER_Y)),
            paddle_rect(state.data[PREV_CPU_X], CPU_Y).union(paddle_rect(state.data[CPU_X], CPU_Y)),
        ];
        for rect in dirty {
            repaint(display, state, rect);
            if <MyDisplay as AppDisplay>::PARTIAL_REFRESH {
                // El SH8601 del reloj requiere columnas alineadas de a dos píxeles.
                let x0 = (rect.x0.max(0) as u16) & !1;
                let x1 = ((rect.x1.min(SCREEN_W - 1) as u16) | 1).min((SCREEN_W - 1) as u16);
                let y0 = rect.y0.max(0);
                let y1 = rect.y1.min(SCREEN_H - 1);
                display.refresh_rect(x0 as i32, y0, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
            }
        }
        if !<MyDisplay as AppDisplay>::PARTIAL_REFRESH { display.refresh_all(); }
    }
    state.data[PREV_BX] = state.data[BX];
    state.data[PREV_BY] = state.data[BY];
    state.data[PREV_PLAYER_X] = state.data[PLAYER_X];
    state.data[PREV_CPU_X] = state.data[CPU_X];
}

fn open(display: &mut MyDisplay, state: &mut AppState) {
    initialize(state);
    draw_full(display, state);
    state.data[NEEDS_FULL] = 0;
}

fn touch(display: &mut MyDisplay, state: &mut AppState, event: TouchEvent) -> AppAction {
    match event {
        TouchEvent::Down(point) => {
            state.data[TOUCH_X] = point.x;
            state.data[TOUCH_Y] = point.y;
            if point.y >= FIELD_Y && state.data[PAUSED] == 0 && state.data[OVER] == 0 {
                let half = PADDLE_W / 2;
                state.data[PLAYER_X] = point.x.clamp(FIELD_X + half, FIELD_X + FIELD_W - half);
            }
        }
        TouchEvent::Move(point) => {
            if state.data[PAUSED] == 0 && state.data[OVER] == 0 && point.y >= FIELD_Y {
                let half = PADDLE_W / 2;
                state.data[PLAYER_X] = point.x.clamp(FIELD_X + half, FIELD_X + FIELD_W - half);
            }
        }
        TouchEvent::Up(point) => {
            let dx = point.x - state.data[TOUCH_X];
            if (state.data[PAUSED] != 0 || state.data[OVER] != 0) && dx > 70 {
                return AppAction::Close;
            }
            if state.data[OVER] != 0 {
                initialize(state);
                draw_full(display, state);
                state.data[NEEDS_FULL] = 0;
            } else if state.data[TOUCH_Y] < FIELD_Y {
                state.data[PAUSED] ^= 1;
                state.data[NEEDS_FULL] = 1;
            }
        }
    }
    AppAction::Stay
}

fn update(state: &mut AppState) -> bool {
    if state.data[OVER] != 0 || state.data[PAUSED] != 0 { return false; }
    if state.data[SERVE] > 0 {
        state.data[SERVE] -= 1;
        return state.data[SERVE] == 0;
    }

    let cpu_speed = 8 + state.data[PLAYER_SCORE].min(4);
    let target = state.data[BX] + BALL / 2;
    state.data[CPU_X] += (target - state.data[CPU_X]).clamp(-cpu_speed, cpu_speed);
    let half = PADDLE_W / 2;
    state.data[CPU_X] = state.data[CPU_X].clamp(FIELD_X + half, FIELD_X + FIELD_W - half);
    state.data[BX] += state.data[VX];
    state.data[BY] += state.data[VY];

    if state.data[BX] <= FIELD_X {
        state.data[BX] = FIELD_X;
        state.data[VX] = -state.data[VX];
    } else if state.data[BX] + BALL >= FIELD_X + FIELD_W {
        state.data[BX] = FIELD_X + FIELD_W - BALL;
        state.data[VX] = -state.data[VX];
    }
    let ball_center = state.data[BX] + BALL / 2;
    if state.data[VY] > 0
        && state.data[BY] + BALL >= PLAYER_Y
        && state.data[BY] + BALL <= PLAYER_Y + PADDLE_H + state.data[VY]
        && (ball_center - state.data[PLAYER_X]).abs() <= half + BALL / 2
    {
        state.data[BY] = PLAYER_Y - BALL;
        state.data[VY] = -state.data[VY];
        state.data[VX] = ((ball_center - state.data[PLAYER_X]) * 12 / half).clamp(-12, 12);
    }
    if state.data[VY] < 0
        && state.data[BY] <= CPU_Y + PADDLE_H
        && state.data[BY] >= CPU_Y + state.data[VY]
        && (ball_center - state.data[CPU_X]).abs() <= half + BALL / 2
    {
        state.data[BY] = CPU_Y + PADDLE_H;
        state.data[VY] = -state.data[VY];
        state.data[VX] = ((ball_center - state.data[CPU_X]) * 12 / half).clamp(-12, 12);
    }

    if state.data[BY] + BALL > FIELD_BOTTOM {
        state.data[CPU_SCORE] += 1;
        if state.data[CPU_SCORE] >= WIN_SCORE { state.data[OVER] = 1; state.data[NEEDS_FULL] = 1; }
        else { serve(state, false); }
    } else if state.data[BY] < FIELD_Y {
        state.data[PLAYER_SCORE] += 1;
        if state.data[PLAYER_SCORE] >= WIN_SCORE { state.data[OVER] = 1; state.data[NEEDS_FULL] = 1; }
        else { serve(state, true); }
    }
    true
}

fn tick(display: &mut MyDisplay, state: &mut AppState) -> AppAction {
    // Physics remains at the original game's 50 ms cadence while the UI loop
    // and touch response run at 16 ms.
    state.data[FRAME_DIVIDER] += 1;
    let divisor = if <MyDisplay as AppDisplay>::PARTIAL_REFRESH { 3 } else { 1 };
    if state.data[FRAME_DIVIDER] >= divisor {
        state.data[FRAME_DIVIDER] = 0;
        if update(state) { render(display, state); }
    }
    AppAction::Stay
}

pub(super) const APP: AppDescriptor = AppDescriptor {
    name: "Ping pong",
    power: PowerProfile::Performance,
    draw_icon,
    open,
    touch,
    tick,
};
