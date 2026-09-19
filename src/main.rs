#![no_std]
#![no_main]

extern crate alloc;

use core::fmt::Write;

use embassy_futures::select::{select4, Either4};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    geometry::Size,
    text::Text,
};
use esp_alloc as _;
use esp_backtrace as _;
use esp_bootloader_esp_idf::esp_app_desc;
use esp_println::println;
use esp_hal::{
    clock::CpuClock,
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf}, dma_buffers,
    gpio::{AnyPin, Input, InputConfig, Level, Output, OutputConfig, Pin, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    interrupt::software::SoftwareInterruptControl,
    main,
    rtc_cntl::{sleep::{Ext0WakeupSource, WakeupLevel}, Rtc},
    spi::{master::{Config as SpiConfig, Spi}, Mode},
    time::Rate,
    timer::timg::TimerGroup,
    usb_serial_jtag::UsbSerialJtag,
};
use heapless::String;
use sh8601_rs::{framebuffer_size, ColorMode, ControllerInterface, DisplaySize, ResetInterface, Sh8601Driver, Ws18AmoledDriver, DMA_CHUNK_SIZE};
use static_cell::StaticCell;

mod drivers;
mod assets;
mod ui;
use drivers::{pmu::Axp2101, rtc::{DateTime, Pcf85063a}, touch::Ft5x06};

const I2C_ADDR_PMU: u8 = 0x34;
const I2C_ADDR_RTC: u8 = 0x51;
const I2C_ADDR_TOUCH: u8 = 0x38;
const BRIGHTNESS_40_PERCENT: u8 = 102;
const BRIGHTNESS_10_PERCENT: u8 = 26;
const CLOCK_DIM_AFTER_MS: u64 = 5_000;
const CLOCK_SLEEP_AFTER_MS: u64 = 65_000;
static PANEL_INIT: &[(u8, &[u8], u16)] = &[
    // Secuencia completa validada para el SH8601 del reloj. 0x29 solo no
    // alcanza: 0x30/0x44/0xc4 configuran la ventana y el modo AMOLED.
    (0x01, &[], 10), (0x11, &[], 120), (0x3a, &[0x77], 5),
    (0x36, &[0x00], 0), (0x44, &[0x01, 0xc5], 0), (0x35, &[0x00], 0),
    (0x29, &[], 120), (0x30, &[0x00, 0x80, 0x00, 0x02], 10),
    (0xc4, &[0x80], 0), (0x44, &[0x01, 0xd1], 0), (0x53, &[0x20], 0),
    (0x63, &[0xff], 0), (0x51, &[BRIGHTNESS_40_PERCENT], 0),
];
struct Page { title: &'static str, lines: &'static [&'static str] }
const PAGES: [Page; 9] = [
    Page { title: "MRU", lines: &["MOVIMIENTO RECTILINEO UNIFORME", "a(t) = 0", "v(t) = v0 = constante", "x(t) = x0 + v0*t", "", "dx = x(t)-x0 = v0*t", "vm = dx/dt = v0"] },
    Page { title: "MRUV", lines: &["MOVIMIENTO RECTILINEO UNIFORMEMENTE VARIADO", "a(t) = a = constante", "v(t) = v0 + a*t", "x(t) = x0 + v0*t + 0.5*a*t^2", "", "dx = v0*t + 0.5*a*t^2", "v^2 = v0^2 + 2*a*(x-x0)", "am = (vf-v0)/t"] },
    Page { title: "CAIDA LIBRE", lines: &["Eje positivo hacia abajo; a(t) = g", "v(t) = v0 + g*t", "y(t) = y0 + v0*t + 0.5*g*t^2", "", "Si se suelta (v0=0):", "v(t) = g*t", "y(t) = y0 + 0.5*g*t^2", "g = 9.8 m/s^2 (o 10 si lo indica el ejercicio)"] },
    Page { title: "TIRO VERTICAL", lines: &["Eje positivo hacia arriba; a(t) = -g", "v(t) = v0 - g*t", "y(t) = y0 + v0*t - 0.5*g*t^2", "", "v^2 = v0^2 - 2*g*(y-y0)", "Punto mas alto: v(t)=0", "t subida = v0/g", "y max = y0 + v0^2/(2*g)", "Si regresa al nivel inicial: t total=2*v0/g"] },
    Page { title: "MCU", lines: &["MOVIMIENTO CIRCULAR UNIFORME", "alfa(t) = 0", "w(t) = w0 = constante", "O(t) = O0 + w0*t", "Oa - Ob = 2*pi*N", "arco(t) = R*O(t)", "v(t) = R*w0", "ac(t) = v(t)^2/R", "", "PERIODO", "T = 1/f", "T = 60/RPM", "T = 2*pi/w", "w = 2*pi*f", "", "CONVERSION DE RPM", "w (rad/s) = RPM*2*pi/60", "Oa = angulo final; Ob = angulo inicial", "N = cantidad de vueltas"] },
    Page { title: "MCUV", lines: &["MOVIMIENTO CIRCULAR VARIADO", "alfa(t) = constante", "w(t) = w0 + alfa*t", "O(t) = O0 + w0*t", "+ 0.5*alfa*t^2", "v(t) = R*w(t)", "at(t) = R*alfa", "ac(t) = R*w(t)^2", "", "SIN TIEMPO", "w^2 = w0^2 + 2*alfa*(O-O0)"] },
    Page { title: "MOV. RELATIVO", lines: &["Para A respecto de B:", "xA/B(t) = xA(t) - xB(t)", "vA/B(t) = vA(t) - vB(t)", "aA/B(t) = aA(t) - aB(t)", "", "MRU de cada movil:", "xA(t)=xA0+vA*t", "xB(t)=xB0+vB*t", "Encuentro: xA(t)=xB(t)", "t = (xB0-xA0)/(vA-vB)", "", "MRUV: x(t)=x0+v0*t+0.5*a*t^2 para cada movil"] },
    Page { title: "PROYECTIL", lines: &["V0x = V0*cos(ang)", "V0y = V0*sin(ang)", "x(t) = x0 + V0x*t", "y(t) = y0 + V0y*t - 0.5*g*t^2", "vx(t) = V0x", "vy(t) = V0y - g*t", "", "Truco: COS -> X, SIN -> Y", "Si el angulo se mide desde la vertical, se invierten."] },
    Page { title: "SIMBOLOS Y SIGNOS", lines: &["x,y posicion; dx desplazamiento; t tiempo", "v velocidad; a aceleracion; g gravedad", "angulo = posicion angular; R = radio", "velocidad angular y aceleracion angular", "", "Elegir sentido positivo y conservar los signos.", "Usar SI: m, s, m/s, m/s^2.", "km/h a m/s: dividir por 3.6; m/s a km/h: multiplicar por 3.6."] },
];

type MyI2c = I2c<'static, esp_hal::Blocking>;
type MyDisplay = Sh8601Driver<Panel, GpioReset, Rgb888>;
static EXEC: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
static TIME_UPDATE: Channel<CriticalSectionRawMutex, DateTime, 1> = Channel::new();

esp_app_desc!();

struct Panel { inner: Ws18AmoledDriver }
impl Panel { fn new(inner: Ws18AmoledDriver) -> Self { Self { inner } } }
impl ControllerInterface for Panel {
    type Error = esp_hal::spi::Error;
    fn send_command(&mut self, c: u8) -> Result<(), Self::Error> { self.inner.send_command(c) }
    fn send_command_with_data(&mut self, c: u8, d: &[u8]) -> Result<(), Self::Error> {
        // El área visible empieza en columna 22 del controlador SH8601.
        if c == 0x2a && d.len() == 4 {
            let a = u16::from_be_bytes([d[0], d[1]]) + 22;
            let b = u16::from_be_bytes([d[2], d[3]]) + 22;
            self.inner.send_command_with_data(c, &[(a>>8) as u8, a as u8, (b>>8) as u8, b as u8])
        } else { self.inner.send_command_with_data(c, d) }
    }
    fn send_pixels(&mut self, p: &[u8]) -> Result<(), Self::Error> { self.inner.send_pixels(p) }
}

struct GpioReset { pin: Output<'static>, delay: Delay }
impl ResetInterface for GpioReset {
    type Error = core::convert::Infallible;
    fn reset(&mut self) -> Result<(), Self::Error> { self.pin.set_low(); self.delay.delay_millis(20); self.pin.set_high(); self.delay.delay_millis(150); Ok(()) }
}

struct Hardware { display: MyDisplay, i2c: MyI2c, button: AnyPin<'static>, touch_reset: Output<'static>, touch_int: AnyPin<'static> }

fn turn_off(mut hw: Hardware) -> ! {
    hw.display.send_command_with_data(0x51, &[0]).ok();
    hw.display.send_command_with_data(0x28, &[]).ok();
    hw.display.send_command_with_data(0x10, &[]).ok();
    Ft5x06::new(&mut hw.i2c, I2C_ADDR_TOUCH, &mut hw.touch_reset).sleep().ok();
    Axp2101::new(&mut hw.i2c, I2C_ADDR_PMU).screen_power(false).ok();
    hw.touch_reset.set_low();
    let wake = Ext0WakeupSource::new(hw.button, WakeupLevel::Low);
    let mut rtc = unsafe { Rtc::new(esp_hal::peripherals::LPWR::steal()) };
    rtc.sleep_deep(&[&wake]);
}

fn clear(display: &mut MyDisplay) { display.clear(Rgb888::BLACK).ok(); }
fn draw_power_saver_clock(display: &mut MyDisplay, time: DateTime) {
    clear(display);
    let number_font = ui::font::Font {
        glyphs: assets::font_numbers_160::GLYPHS,
        bitmap: assets::font_numbers_160::BITMAP,
        line_height: 171,
        base_line: 40,
    };
    let mut hour: String<3> = String::new();
    let _ = write!(hour, "{:02}", time.hour);
    let mut minute: String<3> = String::new();
    let _ = write!(minute, "{:02}", time.minute);
    let hour_width = ui::font::calculate_text_width(&number_font, &hour) as i32;
    let minute_width = ui::font::calculate_text_width(&number_font, &minute) as i32;
    ui::font::draw_text_on_color(display, &number_font, &hour, Point::new((410 - hour_width) / 2, 220), Rgb888::WHITE, Rgb888::BLACK).ok();
    ui::font::draw_text_on_color(display, &number_font, &minute, Point::new((410 - minute_width) / 2, 390), Rgb888::WHITE, Rgb888::BLACK).ok();

    display.flush().ok();
}

fn draw_blue_watchface(display: &mut MyDisplay, time: DateTime, battery_percent: Option<u8>) {
    const PIXELS: usize = 410 * 502;
    let background = assets::BACKGROUND_WF_2;
    clear(display);
    // Fondo original de la watchface: RGB565 seguido de su canal alfa.
    // Se compone sobre negro para conservar el cielo, las estrellas y el auto.
    for index in 0..PIXELS {
        let alpha = background[PIXELS * 2 + index];
        if alpha == 0 { continue; }
        let value = u16::from_le_bytes([background[index * 2], background[index * 2 + 1]]);
        let r = (((value >> 11) & 0x1f) as u32 * 255 / 31 * alpha as u32 / 255) as u8;
        let g = (((value >> 5) & 0x3f) as u32 * 255 / 63 * alpha as u32 / 255) as u8;
        let b = ((value & 0x1f) as u32 * 255 / 31 * alpha as u32 / 255) as u8;
        Pixel(Point::new((index % 410) as i32, (index / 410) as i32), Rgb888::new(r, g, b)).draw(display).ok();
    }

    let number_font = ui::font::Font {
        glyphs: assets::font_numbers_160::GLYPHS,
        bitmap: assets::font_numbers_160::BITMAP,
        line_height: 171,
        base_line: 40,
    };
    let mut hour: String<3> = String::new();
    let _ = write!(hour, "{:02}", time.hour);
    let mut minute: String<3> = String::new();
    let _ = write!(minute, "{:02}", time.minute);
    let hour_width = ui::font::calculate_text_width(&number_font, &hour) as i32;
    let minute_width = ui::font::calculate_text_width(&number_font, &minute) as i32;
    ui::font::draw_text(display, &background[..PIXELS * 2], &number_font, &hour, Point::new((410 - hour_width) / 2, 220), Rgb888::WHITE).ok();
    ui::font::draw_text(display, &background[..PIXELS * 2], &number_font, &minute, Point::new((410 - minute_width) / 2, 390), Rgb888::WHITE).ok();

    let text = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);
    let mut date: String<16> = String::new();
    let _ = write!(date, "{:02}/{:02}/{}", time.day, time.month, time.year);
    Text::with_alignment(&date, Point::new(205, 465), text, embedded_graphics::text::Alignment::Center).draw(display).ok();
    if let Some(percent) = battery_percent {
        let mut battery: String<5> = String::new();
        let _ = write!(battery, "{}%", percent);
        Text::with_alignment(&battery, Point::new(360, 62), text, embedded_graphics::text::Alignment::Right).draw(display).ok();
    }
    display.flush().ok();
}
const SUMMARY_COLUMNS: usize = 40;

fn section_heading(text: &str) -> bool {
    let mut has_letter = false;
    text.chars().all(|c| {
        if c.is_ascii_alphabetic() { has_letter = true; !c.is_ascii_lowercase() } else { true }
    }) && has_letter
}

fn standalone_line(text: &str) -> bool {
    section_heading(text) || text.contains('=')
}

fn draw_summary_line(display: &mut MyDisplay, text: &str, p: Point, style: MonoTextStyle<Rgb888>) {
    // FONT_10X20 no conoce Unicode y dibujaria '?' para α/ω/θ.
    // Sustituimos esos caracteres por espacios y luego pintamos sus glifos.
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

fn wrapped_page_height(lines: &[&str]) -> i32 {
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
                if columns > 0 && columns + needed > SUMMARY_COLUMNS {
                    height += 25;
                    columns = 0;
                }
                columns += word.len() + usize::from(columns > 0);
            }
        }
    }
    if columns > 0 { height += 25; }
    height
}

fn draw_wrapped_page(display: &mut MyDisplay, lines: &[&str], mut y: i32, body: MonoTextStyle<Rgb888>) {
    let mut line: String<48> = String::new();
    for entry in lines {
        if entry.is_empty() {
            if !line.is_empty() {
                draw_summary_line(display, &line, Point::new(4, y), body);
                y += 25; line.clear();
            }
            y += 10;
        } else if standalone_line(entry) {
            if !line.is_empty() {
                Text::new(&line, Point::new(4, y), body).draw(display).ok();
                y += 25; line.clear();
            }
            Text::new(entry, Point::new(4, y), body).draw(display).ok();
            y += 27;
        } else {
            for word in entry.split_whitespace() {
                let needed = word.len() + usize::from(!line.is_empty());
                if !line.is_empty() && line.len() + needed > SUMMARY_COLUMNS {
                    draw_summary_line(display, &line, Point::new(4, y), body);
                    y += 25; line.clear();
                }
                if !line.is_empty() { let _ = line.push(' '); }
                let _ = line.push_str(word);
            }
        }
    }
    if !line.is_empty() { draw_summary_line(display, &line, Point::new(4, y), body); }
}

fn summary_max_scroll(page: usize) -> i32 {
    let content_height = 34 + wrapped_page_height(PAGES[page].lines) + 38;
    // Zona visible entre y=58 y y=440, con un pequeño margen final. Cada
    // página tiene su propia posición de scroll.
    (content_height - 382).max(0)
}

fn draw_summary(display: &mut MyDisplay, page: usize, scroll: i32) {
    clear(display);
    let title = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);
    // Zona segura: la pantalla es redondeada. Dejamos aire arriba/abajo y
    // márgenes laterales para que fórmulas y paginador no choquen con bordes.
    let body = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);
    let page_data = &PAGES[page];
    let y = 64 - scroll;
    Text::new(page_data.title, Point::new(8, y), title).draw(display).ok();
    draw_wrapped_page(display, page_data.lines, y + 34, body);
    let dots_width = (PAGES.len() as i32 - 1) * 15 + 7;
    let dots_x = 205 - dots_width / 2;
    for i in 0..PAGES.len() {
        let color = if i == page { Rgb888::WHITE } else { Rgb888::new(70, 70, 70) };
        Circle::new(Point::new(dots_x + i as i32 * 15, 442), 7)
            .into_styled(PrimitiveStyle::with_fill(color)).draw(display).ok();
    }
    display.flush().ok();
}

fn parse_time(line: &[u8]) -> Option<DateTime> {
    let s = core::str::from_utf8(line).ok()?.trim();
    let v = s.strip_prefix("TIME,")?;
    if v.len() != 19 || &v[10..11] != "T" { return None; }
    let part = |a: usize, b: usize| v.get(a..b)?.parse::<u8>().ok();
    let year = v.get(0..4)?.parse::<u16>().ok()?;
    Some(DateTime { year, month: part(5,7)?, day: part(8,10)?, weekday: 0, hour: part(11,13)?, minute: part(14,16)?, second: part(17,19)? })
}

#[embassy_executor::task]
async fn usb_time_task(mut usb: UsbSerialJtag<'static, esp_hal::Async>) {
    use embedded_io_async::Read;
    let mut line = [0u8; 32]; let mut len = 0; let mut buf = [0u8; 16];
    loop {
        let n = Read::read(&mut usb, &mut buf).await.unwrap_or(0);
        for &b in &buf[..n] {
            if b == b'\n' || b == b'\r' { if let Some(t) = parse_time(&line[..len]) { println!("TIME received {:02}:{:02}", t.hour, t.minute); TIME_UPDATE.send(t).await; } len = 0; }
            else if len < line.len() { line[len] = b; len += 1; } else { len = 0; }
        }
    }
}

#[embassy_executor::task]
async fn ui_task(mut hw: Hardware) {
    let mut showing_summary = false;
    let mut summary_page = 0usize;
    let mut page_scroll = [0i32; PAGES.len()];
    let mut touch_start: Option<(u16, u16)> = None;
    let mut last_touch: Option<(u16, u16)> = None;
    let mut page_changed_this_touch = false;
    let mut summary_hold_deadline: Option<Instant> = None;
    let mut clock_idle_since = Instant::now();
    let mut clock_dimmed = false;
    let mut shown_minute = 255;
    // El programador USB usa GPIO0 durante el reset. Darle tiempo para soltar
    // la línea evita interpretar ese pulso técnico como un apagado del reloj.
    Timer::after(Duration::from_secs(2)).await;
    clock_idle_since = Instant::now();
    loop {
        if !showing_summary {
            let clock_idle_ms = (Instant::now() - clock_idle_since).as_millis();
            if clock_idle_ms >= CLOCK_SLEEP_AFTER_MS {
                turn_off(hw);
            }
            if !clock_dimmed && clock_idle_ms >= CLOCK_DIM_AFTER_MS {
                hw.display.send_command_with_data(0x51, &[BRIGHTNESS_10_PERCENT]).ok();
                clock_dimmed = true;
                shown_minute = 255;
            }
            let time = Pcf85063a::new(&mut hw.i2c, I2C_ADDR_RTC).get().unwrap_or(DateTime { year: 2026, month: 1, day: 1, weekday: 0, hour: 0, minute: 0, second: 0 });
            if time.minute != shown_minute {
                if clock_dimmed {
                    draw_power_saver_clock(&mut hw.display, time);
                } else {
                    let battery = Axp2101::new(&mut hw.i2c, I2C_ADDR_PMU).battery_percent().ok();
                    draw_blue_watchface(&mut hw.display, time, battery);
                }
                shown_minute = time.minute;
            }
        }
        let event = {
            let mut button = Input::new(hw.button.reborrow(), InputConfig::default().with_pull(Pull::Up));
            let mut touch = Input::new(hw.touch_int.reborrow(), InputConfig::default().with_pull(Pull::Up));
            // Al apoyar el dedo sondeamos sólo durante el gesto (25 ms); en
            // reposo seguimos durmiendo hasta IRQ, hora nueva o el minuto.
            let timeout = if touch_start.is_some() {
                Duration::from_millis(25)
            } else if !showing_summary && !clock_dimmed {
                Duration::from_millis(CLOCK_DIM_AFTER_MS.saturating_sub((Instant::now() - clock_idle_since).as_millis()))
            } else if !showing_summary {
                Duration::from_millis(CLOCK_SLEEP_AFTER_MS.saturating_sub((Instant::now() - clock_idle_since).as_millis()))
            } else {
                Duration::from_secs(30)
            };
            select4(button.wait_for_low(), touch.wait_for_falling_edge(), TIME_UPDATE.receive(), Timer::after(timeout)).await
        };
        let mut read_touch = false;
        match event {
            Either4::First(_) => {
                // GPIO0 también es la fuente de wake. Esperar a que el usuario
                // lo suelte antes de deep sleep evita un wake/reset instantáneo.
                let confirmed_press = {
                    let mut button = Input::new(hw.button.reborrow(), InputConfig::default().with_pull(Pull::Up));
                    Timer::after(Duration::from_millis(40)).await;
                    if button.is_low() {
                        button.wait_for_high().await;
                        true
                    } else {
                        false
                    }
                };
                if confirmed_press {
                    Timer::after(Duration::from_millis(80)).await;
                    turn_off(hw);
                }
            }
            Either4::Second(_) => read_touch = true,
            Either4::Third(time) => { Pcf85063a::new(&mut hw.i2c, I2C_ADDR_RTC).set(time).ok(); println!("RTC updated from USB"); shown_minute = 255; }
            Either4::Fourth(_) => read_touch = touch_start.is_some(),
        }
        if read_touch {
            let point = Ft5x06::new(&mut hw.i2c, I2C_ADDR_TOUCH, &mut hw.touch_reset).read().ok().flatten();
            match point {
                Some((x, y)) if !showing_summary => {
                    clock_idle_since = Instant::now();
                    if clock_dimmed {
                        hw.display.send_command_with_data(0x51, &[BRIGHTNESS_40_PERCENT]).ok();
                        clock_dimmed = false;
                        shown_minute = 255;
                    }
                    if touch_start.is_none() {
                        touch_start = Some((x, y));
                        summary_hold_deadline = Some(Instant::now() + Duration::from_millis(600));
                    }
                    // Entrar sólo por pulsación sostenida; un toque breve no
                    // cambia la esfera del reloj por accidente.
                    if summary_hold_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        showing_summary = true;
                        touch_start = Some((x, y)); last_touch = Some((x, y));
                        summary_hold_deadline = None;
                        page_changed_this_touch = false;
                        draw_summary(&mut hw.display, summary_page, page_scroll[summary_page]);
                    }
                }
                Some((x, y)) => {
                    // Cada nuevo dedo sobre una página debe iniciar un gesto.
                    // Antes sólo se inicializaba el toque que abría el resumen.
                    if touch_start.is_none() {
                        touch_start = Some((x, y));
                        last_touch = Some((x, y));
                        page_changed_this_touch = false;
                    } else if let (Some((start_x, _)), Some((_, previous_y))) = (touch_start, last_touch) {
                        let total_dx = x as i32 - start_x as i32;
                        // El cambio de página se decide directamente desde X;
                        // así no queda bloqueado por ruido vertical del FT5x06.
                        if !page_changed_this_touch && total_dx.abs() >= 20 {
                                summary_page = if total_dx < 0 { (summary_page + 1) % PAGES.len() } else { (summary_page + PAGES.len() - 1) % PAGES.len() };
                                page_changed_this_touch = true;
                                draw_summary(&mut hw.display, summary_page, page_scroll[summary_page]);
                        } else if !page_changed_this_touch {
                            // Si no hubo swipe lateral, el arrastre vertical
                            // desplaza únicamente la página actual.
                            let dy = y as i32 - previous_y as i32;
                            if dy != 0 {
                                page_scroll[summary_page] = (page_scroll[summary_page] - dy)
                                    .clamp(0, summary_max_scroll(summary_page));
                                draw_summary(&mut hw.display, summary_page, page_scroll[summary_page]);
                            }
                        }
                    }
                    last_touch = Some((x, y));
                }
                None => { touch_start = None; last_touch = None; summary_hold_deadline = None; page_changed_this_touch = false; }
            }
        }
    }
}

#[main]
fn main() -> ! {
    // No radio crate is linked: Wi-Fi y Bluetooth quedan físicamente sin inicializar.
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz));
    esp_alloc::psram_allocator!(p.PSRAM, esp_hal::psram); esp_alloc::heap_allocator!(size: 48 * 1024);
    let timg = TimerGroup::new(p.TIMG0); let sw = SoftwareInterruptControl::new(p.SW_INTERRUPT); esp_rtos::start(timg.timer0, sw.software_interrupt0);
    let mut delay = Delay::new();
    let mut i2c = I2c::new(p.I2C0, I2cConfig::default().with_frequency(Rate::from_khz(400))).unwrap().with_sda(p.GPIO15).with_scl(p.GPIO14);
    Axp2101::new(&mut i2c, I2C_ADDR_PMU).screen_power(true).ok();
    // Los rails BLDO/DLDO del panel necesitan asentarse antes del reset SPI.
    delay.delay_millis(20);
    Pcf85063a::new(&mut i2c, I2C_ADDR_RTC).init().ok();
    let mut touch_reset = Output::new(p.GPIO9, Level::High, OutputConfig::default());
    Ft5x06::new(&mut i2c, I2C_ADDR_TOUCH, &mut touch_reset).init(&mut delay).ok();
    let (rx, rxd, tx, txd) = dma_buffers!(DMA_CHUNK_SIZE); let rx = DmaRxBuf::new(rxd, rx).unwrap(); let tx = DmaTxBuf::new(txd, tx).unwrap();
    let spi = Spi::new(p.SPI2, SpiConfig::default().with_frequency(Rate::from_mhz(40)).with_mode(Mode::_0)).unwrap().with_sio0(p.GPIO4).with_sio1(p.GPIO5).with_sio2(p.GPIO6).with_sio3(p.GPIO7).with_cs(p.GPIO12).with_sck(p.GPIO11).with_dma(p.DMA_CH0).with_buffers(rx, tx);
    let reset = GpioReset { pin: Output::new(p.GPIO8, Level::High, OutputConfig::default()), delay };
    const SIZE: DisplaySize = DisplaySize::new(410, 502); const FB: usize = framebuffer_size(SIZE, ColorMode::Rgb888);
    let mut d = Delay::new();
    let mut display = Sh8601Driver::<_,_,Rgb888>::builder(Panel::new(Ws18AmoledDriver::new(spi)), reset, SIZE).with_init_commands(PANEL_INIT).build_heap::<_, FB>(ColorMode::Rgb888, &mut d).unwrap();
    display.send_command_with_data(0x51, &[BRIGHTNESS_40_PERCENT]).ok();
    println!("resumen-watch ready: 80MHz, radios off, brightness 40%");
    let usb = UsbSerialJtag::new(p.USB_DEVICE).into_async();
    let hw = Hardware { display, i2c, button: p.GPIO0.degrade(), touch_reset, touch_int: p.GPIO38.degrade() };
    let exec = EXEC.init(esp_rtos::embassy::Executor::new());
    exec.run(|s| { s.spawn(usb_time_task(usb).unwrap()); s.spawn(ui_task(hw).unwrap()); });
}
