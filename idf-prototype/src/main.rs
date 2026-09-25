use core::fmt::Write as _;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::Text,
};
use esp_idf_hal::{
    gpio::{PinDriver, Pull},
    i2c::{config::Config as I2cConfig, I2cDriver},
    peripherals::Peripherals,
    units::Hertz,
    usb_serial::{UsbSerialConfig, UsbSerialDriver},
};
use esp_idf_sys::*;
use heapless::String;
use std::{thread, time::{Duration, Instant}};

mod display;
mod power;

#[path = "../../src/apps/mod.rs"]
mod apps;
#[path = "../../src/assets/mod.rs"]
mod assets;
#[path = "../../src/drivers/mod.rs"]
mod drivers;
#[path = "../../src/pages.rs"]
mod pages;
#[path = "../../src/ui/mod.rs"]
mod ui;

use drivers::{pmu::Axp2101, rtc::{DateTime, Pcf85063a}, touch::Ft5x06};

pub(crate) type MyDisplay = display::Display;

impl apps::AppDisplay for MyDisplay {
    const PARTIAL_REFRESH: bool = true;
    fn refresh_rect(&mut self, x: i32, y: i32, width: u32, height: u32) {
        let _ = self.flush_rect(x, y, width, height);
    }
    fn refresh_all(&mut self) { let _ = self.flush(); }
}

const ADDR_PMU: u8 = 0x34;
const ADDR_RTC: u8 = 0x51;
const ADDR_TOUCH: u8 = 0x38;
const BRIGHTNESS_40: u8 = 102;
const BRIGHTNESS_10: u8 = 26;
const DIM_AFTER: Duration = Duration::from_secs(5);
const SLEEP_AFTER: Duration = Duration::from_secs(65);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen { Watch, Formula, Apps, RunningApp }

fn clear(display: &mut MyDisplay) {
    let _ = display.clear(Rgb888::BLACK);
}

fn draw_power_saver_clock(display: &mut MyDisplay, time: DateTime) {
    clear(display);
    let font = ui::font::Font {
        glyphs: assets::font_numbers_160::GLYPHS,
        bitmap: assets::font_numbers_160::BITMAP,
        line_height: 171,
        base_line: 40,
    };
    let mut hour: String<3> = String::new();
    let mut minute: String<3> = String::new();
    let _ = write!(hour, "{:02}", time.hour);
    let _ = write!(minute, "{:02}", time.minute);
    let hour_width = ui::font::calculate_text_width(&font, &hour) as i32;
    let minute_width = ui::font::calculate_text_width(&font, &minute) as i32;
    let _ = ui::font::draw_text_on_color(display, &font, &hour, Point::new((410-hour_width)/2, 220), Rgb888::WHITE, Rgb888::BLACK);
    let _ = ui::font::draw_text_on_color(display, &font, &minute, Point::new((410-minute_width)/2, 390), Rgb888::WHITE, Rgb888::BLACK);
    let _ = display.flush();
}

fn draw_watchface(display: &mut MyDisplay, time: DateTime, battery: Option<u8>) {
    const PIXELS: usize = 410 * 502;
    let background = assets::BACKGROUND_WF_2;
    clear(display);
    for index in 0..PIXELS {
        let alpha = background[PIXELS * 2 + index];
        if alpha == 0 { continue; }
        let value = u16::from_le_bytes([background[index * 2], background[index * 2 + 1]]);
        let r = (((value >> 11) & 0x1f) as u32 * 255 / 31 * alpha as u32 / 255) as u8;
        let g = (((value >> 5) & 0x3f) as u32 * 255 / 63 * alpha as u32 / 255) as u8;
        let b = ((value & 0x1f) as u32 * 255 / 31 * alpha as u32 / 255) as u8;
        let _ = Pixel(Point::new((index % 410) as i32, (index / 410) as i32), Rgb888::new(r, g, b)).draw(display);
    }

    let font = ui::font::Font {
        glyphs: assets::font_numbers_160::GLYPHS,
        bitmap: assets::font_numbers_160::BITMAP,
        line_height: 171,
        base_line: 40,
    };
    let mut hour: String<3> = String::new();
    let mut minute: String<3> = String::new();
    let _ = write!(hour, "{:02}", time.hour);
    let _ = write!(minute, "{:02}", time.minute);
    let hour_width = ui::font::calculate_text_width(&font, &hour) as i32;
    let minute_width = ui::font::calculate_text_width(&font, &minute) as i32;
    let _ = ui::font::draw_text(display, &background[..PIXELS * 2], &font, &hour, Point::new((410-hour_width)/2, 220), Rgb888::WHITE);
    let _ = ui::font::draw_text(display, &background[..PIXELS * 2], &font, &minute, Point::new((410-minute_width)/2, 390), Rgb888::WHITE);

    let style = MonoTextStyle::new(&FONT_10X20, Rgb888::WHITE);
    let mut date: String<16> = String::new();
    let _ = write!(date, "{:02}/{:02}/{}", time.day, time.month, time.year);
    let _ = Text::with_alignment(&date, Point::new(205, 465), style, embedded_graphics::text::Alignment::Center).draw(display);
    if let Some(percent) = battery {
        let mut value: String<5> = String::new();
        let _ = write!(value, "{}%", percent);
        let _ = Text::with_alignment(&value, Point::new(360, 62), style, embedded_graphics::text::Alignment::Right).draw(display);
    }
    let _ = display.flush();
}

fn parse_time(line: &[u8]) -> Option<DateTime> {
    let input = core::str::from_utf8(line).ok()?.trim();
    let value = input.strip_prefix("TIME,")?;
    if value.len() != 19 || &value[10..11] != "T" { return None; }
    let part = |start, end| value.get(start..end)?.parse::<u8>().ok();
    Some(DateTime {
        year: value.get(0..4)?.parse::<u16>().ok()?,
        month: part(5,7)?, day: part(8,10)?, weekday: 0,
        hour: part(11,13)?, minute: part(14,16)?, second: part(17,19)?,
    })
}

fn read_touch(i2c: &mut I2cDriver<'static>) -> Option<Point> {
    let mut count = [0u8];
    embedded_hal::i2c::I2c::write_read(&mut *i2c, ADDR_TOUCH, &[0x02], &mut count).ok()?;
    if count[0] & 0x0f == 0 { return None; }
    let mut raw = [0u8; 4];
    embedded_hal::i2c::I2c::write_read(&mut *i2c, ADDR_TOUCH, &[0x03], &mut raw).ok()?;
    Some(Point::new(
        (((raw[0] & 0x0f) as u16) << 8 | raw[1] as u16) as i32,
        (((raw[2] & 0x0f) as u16) << 8 | raw[3] as u16) as i32,
    ))
}

fn turn_off(
    display: &mut MyDisplay,
    i2c: &mut I2cDriver<'static>,
    touch_reset: &mut PinDriver<'static, esp_idf_hal::gpio::Output>,
    button: &PinDriver<'static, esp_idf_hal::gpio::Input>,
) -> ! {
    let _ = display.send_command_with_data(0x51, &[0]);
    let _ = display.send_command_with_data(0x28, &[]);
    let _ = display.send_command_with_data(0x10, &[]);
    let _ = Ft5x06::new(&mut *i2c, ADDR_TOUCH, &mut *touch_reset).sleep();
    let _ = Axp2101::new(&mut *i2c, ADDR_PMU).screen_power(false);
    let _ = embedded_hal::digital::OutputPin::set_low(touch_reset);

    // Espera a que se suelte el botón para que el nivel bajo no despierte al instante.
    while button.is_low() { thread::sleep(Duration::from_millis(20)); }
    thread::sleep(Duration::from_millis(80));
    unsafe {
        let _ = rtc_gpio_init(0);
        let _ = rtc_gpio_set_direction(0, rtc_gpio_mode_t_RTC_GPIO_MODE_INPUT_ONLY);
        let _ = rtc_gpio_pullup_en(0);
        let _ = rtc_gpio_pulldown_dis(0);
        let _ = esp_sleep_enable_ext0_wakeup(0, 0);
        esp_deep_sleep_start();
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    power::configure().map_err(idf_error)?;
    let performance_lock = power::PerformanceLock::new().map_err(idf_error)?;
    let p = Peripherals::take()?;
    let i2c_config = I2cConfig::new().baudrate(Hertz(400_000));
    let mut i2c = I2cDriver::new(p.i2c0, p.pins.gpio15, p.pins.gpio14, &i2c_config)?;
    let mut touch_reset = PinDriver::output(p.pins.gpio9)?;
    let button = PinDriver::input(p.pins.gpio0, Pull::Up)?;

    Axp2101::new(&mut i2c, ADDR_PMU).screen_power(true)?;
    thread::sleep(Duration::from_millis(20));
    Pcf85063a::new(&mut i2c, ADDR_RTC).init()?;
    let mut display = MyDisplay::new().map_err(idf_error)?;

    // Mostrar la hora básica en cuanto el panel queda listo. La esfera completa
    // se compone después, mientras la pantalla ya está encendida.
    let initial = Pcf85063a::new(&mut i2c, ADDR_RTC).get().unwrap_or(DateTime {
        year: 2026, month: 1, day: 1, weekday: 0, hour: 0, minute: 0, second: 0,
    });
    draw_power_saver_clock(&mut display, initial);

    let mut touch_delay = esp_idf_hal::delay::FreeRtos;
    let _ = Ft5x06::new(&mut i2c, ADDR_TOUCH, &mut touch_reset).init(&mut touch_delay);

    let mut usb = UsbSerialDriver::new(
        p.usb_serial,
        p.pins.gpio19,
        p.pins.gpio20,
        &UsbSerialConfig::new(),
    )?;
    println!("Resumen Watch ESP-IDF listo: base 80 MHz, apps hasta 160 MHz");

    let mut screen = Screen::Watch;
    let mut formula = apps::formulas::State::new();
    let mut active_app: Option<usize> = None;
    let mut app_state = apps::AppState::new();
    let mut _app_performance = None;
    let mut menu_page = 0usize;
    let mut touch_start: Option<Point> = None;
    let mut last_touch: Option<Point> = None;
    let mut touch_moved = false;
    let mut long_press_deadline: Option<Instant> = None;
    let mut long_press_opened = false;
    let mut idle_since = Instant::now();
    let mut clock_dimmed = false;
    let mut shown_minute;
    let mut last_clock_update = Instant::now() - Duration::from_secs(2);
    let mut usb_line = [0u8; 32];
    let mut usb_len = 0usize;
    let mut button_was_low = false;

    draw_watchface(&mut display, initial, Axp2101::new(&mut i2c, ADDR_PMU).battery_percent().ok());
    shown_minute = initial.minute;

    loop {
        let now = Instant::now();
        let idle = now.duration_since(idle_since);

        if screen != Screen::Formula && idle >= SLEEP_AFTER {
            _app_performance = None;
            turn_off(&mut display, &mut i2c, &mut touch_reset, &button);
        }
        if screen == Screen::Watch && !clock_dimmed && idle >= DIM_AFTER {
            let _ = display.send_command_with_data(0x51, &[BRIGHTNESS_10]);
            clock_dimmed = true;
            shown_minute = 255;
        }

        if screen == Screen::Watch && last_clock_update.elapsed() >= Duration::from_secs(1) {
            let time = Pcf85063a::new(&mut i2c, ADDR_RTC).get().unwrap_or(initial);
            if time.minute != shown_minute {
                if clock_dimmed { draw_power_saver_clock(&mut display, time); }
                else {
                    let battery = Axp2101::new(&mut i2c, ADDR_PMU).battery_percent().ok();
                    draw_watchface(&mut display, time, battery);
                }
                shown_minute = time.minute;
            }
            last_clock_update = Instant::now();
        }

        let mut usb_buf = [0u8; 32];
        let usb_read = usb.read(&mut usb_buf, 0).unwrap_or(0);
        for &byte in &usb_buf[..usb_read] {
            if byte == b'\n' || byte == b'\r' {
                if let Some(time) = parse_time(&usb_line[..usb_len]) {
                    let _ = Pcf85063a::new(&mut i2c, ADDR_RTC).set(time);
                    println!("RTC actualizado desde USB");
                    shown_minute = 255;
                }
                usb_len = 0;
            } else if usb_len < usb_line.len() {
                usb_line[usb_len] = byte;
                usb_len += 1;
            } else {
                usb_len = 0;
            }
        }

        let button_low = button.is_low();
        if button_low && !button_was_low {
            thread::sleep(Duration::from_millis(40));
            if button.is_low() {
                if screen == Screen::RunningApp {
                    _app_performance = None;
                    active_app = None;
                    screen = Screen::Apps;
                    touch_start = None;
                    last_touch = None;
                    touch_moved = false;
                    long_press_deadline = None;
                    long_press_opened = false;
                    idle_since = Instant::now();
                    apps::draw_menu(&mut display, menu_page);
                } else {
                    _app_performance = None;
                    turn_off(&mut display, &mut i2c, &mut touch_reset, &button);
                }
            }
        }
        button_was_low = button_low;

        let point = read_touch(&mut i2c);
        if let Some(point) = point {
            idle_since = Instant::now();
            if screen == Screen::Watch && clock_dimmed {
                let _ = display.send_command_with_data(0x51, &[BRIGHTNESS_40]);
                clock_dimmed = false;
                shown_minute = 255;
            }

            if touch_start.is_none() {
                touch_start = Some(point);
                last_touch = Some(point);
                touch_moved = false;
                long_press_opened = false;
                long_press_deadline = if matches!(screen, Screen::Watch | Screen::Apps) {
                    Some(Instant::now() + Duration::from_millis(600))
                } else { None };
                match screen {
                    Screen::Formula => formula.touch(&mut display, apps::TouchEvent::Down(point)),
                    Screen::RunningApp => if let Some(app) = active_app.and_then(apps::get) {
                        if (app.touch)(&mut display, &mut app_state, apps::TouchEvent::Down(point)) == apps::AppAction::Close {
                            _app_performance = None;
                            screen = Screen::Apps;
                            active_app = None;
                            apps::draw_menu(&mut display, menu_page);
                        }
                    },
                    _ => {}
                }
            } else {
                if let Some(start) = touch_start {
                    if (point.x-start.x).abs() > 20 || (point.y-start.y).abs() > 20 {
                        touch_moved = true;
                        long_press_deadline = None;
                    }
                }
                match screen {
                    Screen::Formula => formula.touch(&mut display, apps::TouchEvent::Move(point)),
                    Screen::RunningApp => if let Some(app) = active_app.and_then(apps::get) {
                        if (app.touch)(&mut display, &mut app_state, apps::TouchEvent::Move(point)) == apps::AppAction::Close {
                            _app_performance = None;
                            screen = Screen::Apps;
                            active_app = None;
                            apps::draw_menu(&mut display, menu_page);
                        }
                    },
                    _ => {}
                }
                last_touch = Some(point);
            }

            if !touch_moved && !long_press_opened && long_press_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                long_press_deadline = None;
                long_press_opened = true;
                screen = Screen::Formula;
                formula.open(&mut display, point);
            }
        } else if let Some(start) = touch_start {
            let end = last_touch.unwrap_or(start);
            match screen {
                Screen::Formula => formula.touch(&mut display, apps::TouchEvent::Up(end)),
                Screen::RunningApp => if let Some(app) = active_app.and_then(apps::get) {
                    if (app.touch)(&mut display, &mut app_state, apps::TouchEvent::Up(end)) == apps::AppAction::Close {
                        _app_performance = None;
                        screen = Screen::Apps;
                        active_app = None;
                        apps::draw_menu(&mut display, menu_page);
                    }
                },
                Screen::Watch if !long_press_opened && !touch_moved => {
                    screen = Screen::Apps;
                    menu_page = 0;
                    apps::draw_menu(&mut display, menu_page);
                }
                Screen::Apps if !long_press_opened => {
                    let dx = end.x - start.x;
                    if touch_moved && dx.abs() >= 30 {
                        let max_page = apps::page_count().saturating_sub(1);
                        if dx < 0 { menu_page = (menu_page + 1).min(max_page); }
                        else { menu_page = menu_page.saturating_sub(1); }
                        apps::draw_menu(&mut display, menu_page);
                    } else if !touch_moved {
                        if let Some(index) = apps::hit_test(menu_page, start) {
                            if let Some(app) = apps::get(index) {
                                active_app = Some(index);
                                if app.power == apps::PowerProfile::Performance {
                                    _app_performance = Some(performance_lock.enter().map_err(idf_error)?);
                                }
                                app_state = apps::AppState::new();
                                screen = Screen::RunningApp;
                                (app.open)(&mut display, &mut app_state);
                            }
                        } else {
                            screen = Screen::Watch;
                            let time = Pcf85063a::new(&mut i2c, ADDR_RTC).get().unwrap_or(initial);
                            let battery = Axp2101::new(&mut i2c, ADDR_PMU).battery_percent().ok();
                            draw_watchface(&mut display, time, battery);
                            shown_minute = time.minute;
                        }
                    }
                }
                _ => {}
            }
            touch_start = None;
            last_touch = None;
            touch_moved = false;
            long_press_deadline = None;
            long_press_opened = false;
            if screen == Screen::Watch { idle_since = Instant::now(); }
        }
        if screen == Screen::RunningApp {
            if let Some(app) = active_app.and_then(apps::get) {
                if (app.tick)(&mut display, &mut app_state) == apps::AppAction::Close {
                    _app_performance = None;
                    screen = Screen::Apps;
                    active_app = None;
                    apps::draw_menu(&mut display, menu_page);
                }
            }
        }
        let delay_ms = if screen == Screen::RunningApp { 16 } else if touch_start.is_some() { 25 } else { 50 };
        thread::sleep(Duration::from_millis(delay_ms));
    }
}

fn main() {
    esp_idf_sys::link_patches();
    if let Err(error) = run() {
        eprintln!("Error fatal del reloj: {error}");
        loop { thread::sleep(Duration::from_secs(1)); }
    }
}

fn idf_error(code: esp_err_t) -> std::io::Error {
    std::io::Error::other(format!("ESP-IDF error {code}"))
}
