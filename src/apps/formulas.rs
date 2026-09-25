use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::Text,
};
use heapless::String;

use crate::{apps::TouchEvent, pages::PAGES, MyDisplay};

const SUMMARY_COLUMNS: usize = 40;

pub(crate) struct State {
    page: usize,
    scroll: [i32; PAGES.len()],
    touch_start: Option<Point>,
    last_touch: Option<Point>,
    page_changed_this_touch: bool,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self { page: 0, scroll: [0; PAGES.len()], touch_start: None, last_touch: None, page_changed_this_touch: false }
    }

    pub(crate) fn open(&mut self, display: &mut MyDisplay, held_at: Point) {
        self.touch_start = Some(held_at);
        self.last_touch = Some(held_at);
        self.page_changed_this_touch = false;
        self.draw(display);
    }

    fn draw(&self, display: &mut MyDisplay) {
        draw_page(display, self.page, self.scroll[self.page]);
    }

    pub(crate) fn touch(&mut self, display: &mut MyDisplay, event: TouchEvent) {
        match event {
            TouchEvent::Down(point) => {
                self.touch_start = Some(point);
                self.last_touch = Some(point);
                self.page_changed_this_touch = false;
            }
            TouchEvent::Move(point) => {
                if let (Some(start), Some(previous)) = (self.touch_start, self.last_touch) {
                    let total_dx = point.x - start.x;
                    if !self.page_changed_this_touch && total_dx.abs() >= 20 {
                        self.page = if total_dx < 0 { (self.page + 1) % PAGES.len() } else { (self.page + PAGES.len() - 1) % PAGES.len() };
                        self.page_changed_this_touch = true;
                        self.draw(display);
                    } else if !self.page_changed_this_touch {
                        let dy = point.y - previous.y;
                        if dy != 0 {
                            self.scroll[self.page] = (self.scroll[self.page] - dy).clamp(0, max_scroll(self.page));
                            self.draw(display);
                        }
                    }
                }
                self.last_touch = Some(point);
            }
            TouchEvent::Up(_) => {
                self.touch_start = None;
                self.last_touch = None;
                self.page_changed_this_touch = false;
            }
        }
    }
}

fn section_heading(text: &str) -> bool {
    let mut has_letter = false;
    text.chars().all(|c| {
        if c.is_ascii_alphabetic() { has_letter = true; !c.is_ascii_lowercase() } else { true }
    }) && has_letter
}

fn standalone_line(text: &str) -> bool {
    section_heading(text) || text.contains('=')
}

fn draw_line(display: &mut MyDisplay, text: &str, p: Point, style: MonoTextStyle<Rgb888>) {
    // FONT_10X20 no incluye α/ω/θ: se reservan sus espacios y se dibujan aquí.
    let mut ascii_line: String<64> = String::new();
    for ch in text.chars() {
        if matches!(ch, 'α' | 'ω' | 'θ') { let _ = ascii_line.push(' '); }
        else { let _ = ascii_line.push(ch); }
    }
    Text::new(&ascii_line, p, style).draw(display).ok();
    for (i, ch) in text.chars().enumerate() {
        let rows: &[u8] = match ch {
            'α' => &[0b01110, 0b00001, 0b01111, 0b10001, 0b01111, 0, 0],
            'ω' => &[0b10001, 0b10001, 0b10101, 0b10101, 0b01010, 0, 0],
            'θ' => &[0b01110, 0b10001, 0b11111, 0b10001, 0b01110, 0, 0],
            _ => continue,
        };
        let x = p.x + (i as i32) * 10;
        for (ry, bits) in rows.iter().enumerate() {
            for rx in 0..5 {
                if bits & (1 << (4 - rx)) != 0 {
                    Rectangle::new(Point::new(x + rx * 2, p.y - 15 + ry as i32 * 2), Size::new(2, 2))
                        .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE)).draw(display).ok();
                }
            }
        }
    }
}

fn wrapped_height(lines: &[&str]) -> i32 {
    let mut height = 0;
    let mut columns = 0usize;
    for entry in lines {
        if entry.is_empty() {
            if columns > 0 { height += 25; columns = 0; }
            height += 10;
        } else if standalone_line(entry) {
            if columns > 0 { height += 25; columns = 0; }
            height += 27;
        } else {
            for word in entry.split_whitespace() {
                let needed = word.len() + usize::from(columns > 0);
                if columns > 0 && columns + needed > SUMMARY_COLUMNS { height += 25; columns = 0; }
                columns += word.len() + usize::from(columns > 0);
            }
        }
    }
    if columns > 0 { height += 25; }
    height
}

fn draw_wrapped(display: &mut MyDisplay, lines: &[&str], mut y: i32, body: MonoTextStyle<Rgb888>) {
    let mut line: String<48> = String::new();
    for entry in lines {
        if entry.is_empty() {
            if !line.is_empty() { draw_line(display, &line, Point::new(4, y), body); y += 25; line.clear(); }
            y += 10;
        } else if standalone_line(entry) {
            if !line.is_empty() { Text::new(&line, Point::new(4, y), body).draw(display).ok(); y += 25; line.clear(); }
            Text::new(entry, Point::new(4, y), body).draw(display).ok();
            y += 27;
        } else {
            for word in entry.split_whitespace() {
                let needed = word.len() + usize::from(!line.is_empty());
                if !line.is_empty() && line.len() + needed > SUMMARY_COLUMNS { draw_line(display, &line, Point::new(4, y), body); y += 25; line.clear(); }
                if !line.is_empty() { let _ = line.push(' '); }
                let _ = line.push_str(word);
            }
        }
    }
    if !line.is_empty() { draw_line(display, &line, Point::new(4, y), body); }
}

fn max_scroll(page: usize) -> i32 {
    let content_height = 34 + wrapped_height(PAGES[page].lines) + 38;
    (content_height - 382).max(0)
}

fn draw_page(display: &mut MyDisplay, page: usize, scroll: i32) {
    display.clear(Rgb888::BLACK).ok();
    let style = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);
    let page_data = &PAGES[page];
    let y = 64 - scroll;
    Text::new(page_data.title, Point::new(8, y), style).draw(display).ok();
    draw_wrapped(display, page_data.lines, y + 34, style);
    let width = (PAGES.len() as i32 - 1) * 15 + 7;
    let left = 205 - width / 2;
    for index in 0..PAGES.len() {
        let color = if index == page { Rgb888::WHITE } else { Rgb888::new(70, 70, 70) };
        Circle::new(Point::new(left + index as i32 * 15, 442), 7)
            .into_styled(PrimitiveStyle::with_fill(color)).draw(display).ok();
    }
    display.flush().ok();
}
