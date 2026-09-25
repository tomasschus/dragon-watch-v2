use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};

use super::{AppAction, AppDescriptor, AppState, PowerProfile, TouchEvent};
use crate::MyDisplay;

const WHITE_LEVEL: u8 = 255;
const WATCH_LEVEL: u8 = 102;

fn draw_icon(display: &mut MyDisplay, center: Point) {
    let yellow = Rgb888::new(255, 220, 40);
    Rectangle::new(Point::new(center.x - 12, center.y - 22), Size::new(24, 34))
        .into_styled(PrimitiveStyle::with_fill(yellow)).draw(display).ok();
    Rectangle::new(Point::new(center.x - 8, center.y + 12), Size::new(16, 8))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE)).draw(display).ok();
}

fn open(display: &mut MyDisplay, _state: &mut AppState) {
    let _ = display.send_command_with_data(0x51, &[WHITE_LEVEL]);
    display.clear(Rgb888::WHITE).ok();
    let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(30, 30, 30));
    Text::with_alignment("Toca para salir", Point::new(205, 475), style, Alignment::Center).draw(display).ok();
    display.flush().ok();
}

fn touch(display: &mut MyDisplay, _state: &mut AppState, event: TouchEvent) -> AppAction {
    if matches!(event, TouchEvent::Down(_)) {
        let _ = display.send_command_with_data(0x51, &[WATCH_LEVEL]);
        AppAction::Close
    } else {
        AppAction::Stay
    }
}

fn tick(_display: &mut MyDisplay, _state: &mut AppState) -> AppAction { AppAction::Stay }

pub(super) const APP: AppDescriptor = AppDescriptor {
    name: "Linterna",
    power: PowerProfile::Balanced,
    draw_icon,
    open,
    touch,
    tick,
};
