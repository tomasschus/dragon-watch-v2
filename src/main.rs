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
mod apps;
mod pages;
use drivers::{pmu::Axp2101, rtc::{DateTime, Pcf85063a}, touch::Ft5x06};

const I2C_ADDR_PMU: u8 = 0x34;
const I2C_ADDR_RTC: u8 = 0x51;
const I2C_ADDR_TOUCH: u8 = 0x38;
const BRIGHTNESS_40_PERCENT: u8 = 102;
const BRIGHTNESS_10_PERCENT: u8 = 26;
const CLOCK_DIM_AFTER_MS: u64 = 5_000;
const CLOCK_SLEEP_AFTER_MS: u64 = 65_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum UiScreen { Watch, Formula, Apps, RunningApp }

static PANEL_INIT: &[(u8, &[u8], u16)] = &[
    // Secuencia completa validada para el SH8601 del reloj. 0x29 solo no
    // alcanza: 0x30/0x44/0xc4 configuran la ventana y el modo AMOLED.
    (0x01, &[], 10), (0x11, &[], 120), (0x3a, &[0x77], 5),
    (0x36, &[0x00], 0), (0x44, &[0x01, 0xc5], 0), (0x35, &[0x00], 0),
    (0x29, &[], 120), (0x30, &[0x00, 0x80, 0x00, 0x02], 10),
    (0xc4, &[0x80], 0), (0x44, &[0x01, 0xd1], 0), (0x53, &[0x20], 0),
    (0x63, &[0xff], 0), (0x51, &[BRIGHTNESS_40_PERCENT], 0),
];

type MyI2c = I2c<'static, esp_hal::Blocking>;
pub(crate) type MyDisplay = Sh8601Driver<Panel, GpioReset, Rgb888>;

impl apps::AppDisplay for MyDisplay {
    const PARTIAL_REFRESH: bool = false;
    fn refresh_rect(&mut self, _x: i32, _y: i32, _width: u32, _height: u32) {}
    fn refresh_all(&mut self) { self.flush().ok(); }
}
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
    let mut screen = UiScreen::Watch;
    let mut formula_state = apps::formulas::State::new();
    let mut active_app: Option<usize> = None;
    let mut app_state = apps::AppState::new();
    let mut menu_page = 0usize;
    let mut touch_start: Option<Point> = None;
    let mut last_touch: Option<Point> = None;
    let mut touch_moved = false;
    let mut long_press_deadline: Option<Instant> = None;
    let mut long_press_opened = false;
    let mut clock_idle_since = Instant::now();
    let mut clock_dimmed = false;
    let mut shown_minute = 255;

    let initial_time = Pcf85063a::new(&mut hw.i2c, I2C_ADDR_RTC).get().unwrap_or(DateTime { year: 2026, month: 1, day: 1, weekday: 0, hour: 0, minute: 0, second: 0 });
    draw_power_saver_clock(&mut hw.display, initial_time);
    let mut touch_delay = Delay::new();
    Ft5x06::new(&mut hw.i2c, I2C_ADDR_TOUCH, &mut hw.touch_reset).init(&mut touch_delay).ok();
    let battery = Axp2101::new(&mut hw.i2c, I2C_ADDR_PMU).battery_percent().ok();
    draw_blue_watchface(&mut hw.display, initial_time, battery);
    shown_minute = initial_time.minute;
    clock_idle_since = Instant::now();

    loop {
        if screen != UiScreen::Formula {
            let idle_ms = (Instant::now() - clock_idle_since).as_millis();
            if idle_ms >= CLOCK_SLEEP_AFTER_MS { turn_off(hw); }
            if screen == UiScreen::Watch && !clock_dimmed && idle_ms >= CLOCK_DIM_AFTER_MS {
                hw.display.send_command_with_data(0x51, &[BRIGHTNESS_10_PERCENT]).ok();
                clock_dimmed = true;
                shown_minute = 255;
            }
            if screen == UiScreen::Watch {
                let time = Pcf85063a::new(&mut hw.i2c, I2C_ADDR_RTC).get().unwrap_or(DateTime { year: 2026, month: 1, day: 1, weekday: 0, hour: 0, minute: 0, second: 0 });
                if time.minute != shown_minute {
                    if clock_dimmed { draw_power_saver_clock(&mut hw.display, time); }
                    else {
                        let battery = Axp2101::new(&mut hw.i2c, I2C_ADDR_PMU).battery_percent().ok();
                        draw_blue_watchface(&mut hw.display, time, battery);
                    }
                    shown_minute = time.minute;
                }
            }
        }

        let event = {
            let mut button = Input::new(hw.button.reborrow(), InputConfig::default().with_pull(Pull::Up));
            let mut touch = Input::new(hw.touch_int.reborrow(), InputConfig::default().with_pull(Pull::Up));
            let timeout = if touch_start.is_some() {
                Duration::from_millis(25)
            } else if screen == UiScreen::RunningApp {
                Duration::from_millis(if <MyDisplay as apps::AppDisplay>::PARTIAL_REFRESH { 16 } else { 50 })
            } else if screen == UiScreen::Watch && !clock_dimmed {
                Duration::from_millis(CLOCK_DIM_AFTER_MS.saturating_sub((Instant::now() - clock_idle_since).as_millis()))
            } else if screen != UiScreen::Formula {
                Duration::from_millis(CLOCK_SLEEP_AFTER_MS.saturating_sub((Instant::now() - clock_idle_since).as_millis()))
            } else {
                Duration::from_secs(30)
            };
            select4(button.wait_for_low(), touch.wait_for_falling_edge(), TIME_UPDATE.receive(), Timer::after(timeout)).await
        };

        let mut read_touch = false;
        match event {
            Either4::First(_) => {
                let confirmed_press = {
                    let mut button = Input::new(hw.button.reborrow(), InputConfig::default().with_pull(Pull::Up));
                    Timer::after(Duration::from_millis(40)).await;
                    if button.is_low() { button.wait_for_high().await; true } else { false }
                };
                if confirmed_press {
                    Timer::after(Duration::from_millis(80)).await;
                    if screen == UiScreen::RunningApp {
                        active_app = None;
                        screen = UiScreen::Apps;
                        touch_start = None;
                        last_touch = None;
                        touch_moved = false;
                        long_press_deadline = None;
                        long_press_opened = false;
                        clock_idle_since = Instant::now();
                        apps::draw_menu(&mut hw.display, menu_page);
                    } else {
                        turn_off(hw);
                    }
                }
            }
            Either4::Second(_) => read_touch = true,
            Either4::Third(time) => {
                Pcf85063a::new(&mut hw.i2c, I2C_ADDR_RTC).set(time).ok();
                println!("RTC updated from USB");
                shown_minute = 255;
            }
            Either4::Fourth(_) => read_touch = touch_start.is_some(),
        }

        if screen == UiScreen::RunningApp {
            if let Some(app) = active_app.and_then(apps::get) {
                if (app.tick)(&mut hw.display, &mut app_state) == apps::AppAction::Close {
                    screen = UiScreen::Apps;
                    active_app = None;
                    apps::draw_menu(&mut hw.display, menu_page);
                }
            }
        }
        if !read_touch { continue; }
        let point = Ft5x06::new(&mut hw.i2c, I2C_ADDR_TOUCH, &mut hw.touch_reset).read().ok().flatten()
            .map(|(x, y)| Point::new(x as i32, y as i32));

        if let Some(point) = point {
            clock_idle_since = Instant::now();
            if screen == UiScreen::Watch && clock_dimmed {
                hw.display.send_command_with_data(0x51, &[BRIGHTNESS_40_PERCENT]).ok();
                clock_dimmed = false;
                shown_minute = 255;
            }

            if touch_start.is_none() {
                touch_start = Some(point);
                last_touch = Some(point);
                touch_moved = false;
                long_press_opened = false;
                long_press_deadline = if matches!(screen, UiScreen::Watch | UiScreen::Apps) {
                    Some(Instant::now() + Duration::from_millis(600))
                } else { None };
                match screen {
                    UiScreen::Formula => formula_state.touch(&mut hw.display, apps::TouchEvent::Down(point)),
                    UiScreen::RunningApp => {
                        if let Some(app) = active_app.and_then(apps::get) {
                            if (app.touch)(&mut hw.display, &mut app_state, apps::TouchEvent::Down(point)) == apps::AppAction::Close {
                                screen = UiScreen::Apps;
                                active_app = None;
                                long_press_opened = true;
                                apps::draw_menu(&mut hw.display, menu_page);
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                if let Some(start) = touch_start {
                    if (point.x - start.x).abs() > 20 || (point.y - start.y).abs() > 20 {
                        touch_moved = true;
                        long_press_deadline = None;
                    }
                }
                match screen {
                    UiScreen::Formula => formula_state.touch(&mut hw.display, apps::TouchEvent::Move(point)),
                    UiScreen::RunningApp => {
                        if let Some(app) = active_app.and_then(apps::get) {
                            if (app.touch)(&mut hw.display, &mut app_state, apps::TouchEvent::Move(point)) == apps::AppAction::Close {
                                screen = UiScreen::Apps;
                                active_app = None;
                                long_press_opened = true;
                                apps::draw_menu(&mut hw.display, menu_page);
                            }
                        }
                    }
                    _ => {}
                }
                last_touch = Some(point);
            }

            if !touch_moved && !long_press_opened && long_press_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                long_press_deadline = None;
                long_press_opened = true;
                screen = UiScreen::Formula;
                formula_state.open(&mut hw.display, point);
            }
        } else if let Some(start) = touch_start {
            let end = last_touch.unwrap_or(start);
            match screen {
                UiScreen::Formula => formula_state.touch(&mut hw.display, apps::TouchEvent::Up(end)),
                UiScreen::RunningApp => {
                    if let Some(app) = active_app.and_then(apps::get) {
                        if (app.touch)(&mut hw.display, &mut app_state, apps::TouchEvent::Up(end)) == apps::AppAction::Close {
                            screen = UiScreen::Apps;
                            active_app = None;
                            apps::draw_menu(&mut hw.display, menu_page);
                        }
                    }
                }
                UiScreen::Watch if !long_press_opened && !touch_moved => {
                    screen = UiScreen::Apps;
                    menu_page = 0;
                    apps::draw_menu(&mut hw.display, menu_page);
                }
                UiScreen::Apps if !long_press_opened => {
                    let dx = end.x - start.x;
                    if touch_moved && dx.abs() >= 30 {
                        let max_page = apps::page_count().saturating_sub(1);
                        if dx < 0 { menu_page = (menu_page + 1).min(max_page); }
                        else { menu_page = menu_page.saturating_sub(1); }
                        apps::draw_menu(&mut hw.display, menu_page);
                    } else if !touch_moved {
                        if let Some(index) = apps::hit_test(menu_page, start) {
                            if let Some(app) = apps::get(index) {
                                active_app = Some(index);
                                app_state = apps::AppState::new();
                                screen = UiScreen::RunningApp;
                                (app.open)(&mut hw.display, &mut app_state);
                            }
                        } else {
                            screen = UiScreen::Watch;
                            let time = Pcf85063a::new(&mut hw.i2c, I2C_ADDR_RTC).get().unwrap_or(DateTime { year: 2026, month: 1, day: 1, weekday: 0, hour: 0, minute: 0, second: 0 });
                            let battery = Axp2101::new(&mut hw.i2c, I2C_ADDR_PMU).battery_percent().ok();
                            draw_blue_watchface(&mut hw.display, time, battery);
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
            if screen == UiScreen::Watch { clock_idle_since = Instant::now(); }
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
