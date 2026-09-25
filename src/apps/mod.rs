//! Apps del reloj.
//!
//! Para agregar una app: crear `src/apps/nombre.rs`, exportar un `APP` y
//! agregar `mod nombre;` más `nombre::APP` al registro `APPS`.

pub(crate) mod formulas;

use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, PrimitiveStyle},
    text::{Alignment, Text},
};

use crate::MyDisplay;

pub const APPS_PER_PAGE: usize = 9;

#[derive(Clone, Copy)]
pub(crate) enum TouchEvent {
    Down(Point),
    Move(Point),
    Up(Point),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppAction {
    Stay,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PowerProfile {
    /// Mantiene la frecuencia base configurada por el sistema.
    Balanced,
    /// Solicita la frecuencia máxima mientras la app está abierta.
    Performance,
}

/// Scratchpad reinicializado al abrir una app. Mantiene estado por instancia
/// sin asignaciones dinámicas ni estado global mutable.
pub(crate) struct AppState {
    pub data: [i32; 24],
}

/// Adaptador de presentación para que las apps puedan usar actualizaciones
/// parciales donde el controlador las soporte.
pub(crate) trait AppDisplay {
    const PARTIAL_REFRESH: bool;
    fn refresh_rect(&mut self, x: i32, y: i32, width: u32, height: u32);
    fn refresh_all(&mut self);
}

impl AppState {
    pub(crate) const fn new() -> Self { Self { data: [0; 24] } }
}

pub(crate) struct AppDescriptor {
    pub name: &'static str,
    pub power: PowerProfile,
    pub draw_icon: fn(&mut MyDisplay, Point),
    pub open: fn(&mut MyDisplay, &mut AppState),
    pub touch: fn(&mut MyDisplay, &mut AppState, TouchEvent) -> AppAction,
    /// Actualización llamada desde el loop principal (~20 Hz), nunca crea un
    /// hilo propio. Las apps estáticas pueden dejarla vacía.
    pub tick: fn(&mut MyDisplay, &mut AppState) -> AppAction,
}

mod flashlight;
mod ping_pong;

// Registro estático: agregar una app consiste en su módulo y una entrada.
pub(crate) static APPS: &[AppDescriptor] = &[
    flashlight::APP,
    ping_pong::APP,
];

pub(crate) fn page_count() -> usize {
    APPS.len().div_ceil(APPS_PER_PAGE)
}

pub(crate) fn draw_menu(display: &mut MyDisplay, page: usize) {
    display.clear(Rgb888::BLACK).ok();
    let text = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);
    Text::with_alignment("APPS", Point::new(205, 55), text, Alignment::Center).draw(display).ok();

    if APPS.is_empty() {
        Text::with_alignment("Sin aplicaciones", Point::new(205, 251), text, Alignment::Center).draw(display).ok();
        display.flush().ok();
        return;
    }

    let first = page.min(page_count().saturating_sub(1)) * APPS_PER_PAGE;
    let end = (first + APPS_PER_PAGE).min(APPS.len());
    for index in first..end {
        let local = index - first;
        let col = (local % 3) as i32;
        let row = (local / 3) as i32;
        let center = Point::new(68 + col * 137, 139 + row * 104);
        (APPS[index].draw_icon)(display, center);
        Text::with_alignment(APPS[index].name, Point::new(center.x, center.y + 43), text, Alignment::Center).draw(display).ok();
    }

    if page_count() > 1 {
        let width = (page_count() as i32 - 1) * 15 + 7;
        let left = 205 - width / 2;
        for index in 0..page_count() {
            let color = if index == page { Rgb888::WHITE } else { Rgb888::new(70, 70, 70) };
            Circle::new(Point::new(left + index as i32 * 15, 455), 7)
                .into_styled(PrimitiveStyle::with_fill(color)).draw(display).ok();
        }
    }
    display.flush().ok();
}

pub(crate) fn hit_test(page: usize, point: Point) -> Option<usize> {
    if point.x < 0 || point.x >= 410 || !(95..430).contains(&point.y) {
        return None;
    }
    let col = (point.x / 137) as usize;
    let row = ((point.y - 95) / 112) as usize;
    if col >= 3 || row >= 3 {
        return None;
    }
    let index = page * APPS_PER_PAGE + row * 3 + col;
    (index < APPS.len()).then_some(index)
}

pub(crate) fn get(index: usize) -> Option<&'static AppDescriptor> {
    APPS.get(index)
}
