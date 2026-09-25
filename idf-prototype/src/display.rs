use core::convert::Infallible;
use embedded_graphics::{
    pixelcolor::{Rgb888, RgbColor},
    prelude::{DrawTarget, OriginDimensions, Pixel, Point, Size},
};
use esp_idf_sys::*;
use std::{ptr, time::Duration};

pub const WIDTH: usize = 410;
pub const HEIGHT: usize = 502;
const PIXELS: usize = WIDTH * HEIGHT;
const COLUMN_OFFSET: u16 = 22;
const DMA_CHUNK: usize = 16_380;
const OPCODE_CONTROL: u16 = 0x02;
const OPCODE_PIXELS: u16 = 0x32;
const RAM_WRITE: u8 = 0x2c;
const RAM_WRITE_CONTINUE: u8 = 0x3c;

const PANEL_INIT: &[(u8, &[u8], u16)] = &[
    (0x01, &[], 10),
    (0x11, &[], 120),
    (0x3a, &[0x77], 5),
    (0x36, &[0x00], 0),
    (0x44, &[0x01, 0xc5], 0),
    (0x35, &[0x00], 0),
    (0x29, &[], 120),
    (0x30, &[0x00, 0x80, 0x00, 0x02], 10),
    (0xc4, &[0x80], 0),
    (0x44, &[0x01, 0xd1], 0),
    (0x53, &[0x20], 0),
    (0x63, &[0xff], 0),
    (0x51, &[102], 0),
];

pub struct Display {
    spi: spi_device_handle_t,
    pixels: Vec<u8>,
    dma: *mut u8,
}

impl Display {
    pub fn new() -> Result<Self, esp_err_t> {
        unsafe {
            gpio_reset_pin(8);
            gpio_set_direction(8, gpio_mode_t_GPIO_MODE_OUTPUT);
            gpio_set_level(8, 0);
        }
        std::thread::sleep(Duration::from_millis(20));
        unsafe { gpio_set_level(8, 1) };
        std::thread::sleep(Duration::from_millis(150));

        let mut bus: spi_bus_config_t = unsafe { core::mem::zeroed() };
        unsafe {
            bus.__bindgen_anon_1 = spi_bus_config_t__bindgen_ty_1 { data0_io_num: 4 };
            bus.__bindgen_anon_2 = spi_bus_config_t__bindgen_ty_2 { data1_io_num: 5 };
            bus.__bindgen_anon_3 = spi_bus_config_t__bindgen_ty_3 { data2_io_num: 6 };
            bus.__bindgen_anon_4 = spi_bus_config_t__bindgen_ty_4 { data3_io_num: 7 };
        }
        bus.sclk_io_num = 11;
        bus.max_transfer_sz = DMA_CHUNK as i32;
        let result = unsafe {
            spi_bus_initialize(
                spi_host_device_t_SPI2_HOST,
                &bus,
                spi_common_dma_t_SPI_DMA_CH_AUTO,
            )
        };
        check(result)?;

        let config = spi_device_interface_config_t {
            command_bits: 8,
            address_bits: 24,
            mode: 0,
            clock_speed_hz: 40_000_000,
            spics_io_num: 12,
            queue_size: 1,
            flags: SPI_DEVICE_HALFDUPLEX,
            ..Default::default()
        };
        let mut spi = ptr::null_mut();
        let result = unsafe {
            spi_bus_add_device(spi_host_device_t_SPI2_HOST, &config, &mut spi)
        };
        if let Err(error) = check(result) {
            unsafe { spi_bus_free(spi_host_device_t_SPI2_HOST) };
            return Err(error);
        }

        let dma = unsafe { heap_caps_malloc(DMA_CHUNK, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL) }
            .cast::<u8>();
        if dma.is_null() {
            unsafe {
                spi_bus_remove_device(spi);
                spi_bus_free(spi_host_device_t_SPI2_HOST);
            }
            return Err(ESP_ERR_NO_MEM as esp_err_t);
        }

        let mut display = Self {
            spi,
            pixels: vec![0; PIXELS * 3],
            dma,
        };
        for &(command, data, delay_ms) in PANEL_INIT {
            display.send_command_with_data(command, data)?;
            if delay_ms != 0 {
                std::thread::sleep(Duration::from_millis(delay_ms as u64));
            }
        }
        Ok(display)
    }

    pub fn clear(&mut self, color: Rgb888) -> Result<(), esp_err_t> {
        if color.r() == 0 && color.g() == 0 && color.b() == 0 {
            self.pixels.fill(0);
            return Ok(());
        }
        for pixel in self.pixels.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[color.r(), color.g(), color.b()]);
        }
        Ok(())
    }

    pub fn send_command_with_data(&mut self, command: u8, data: &[u8]) -> Result<(), esp_err_t> {
        self.transmit(OPCODE_CONTROL, command, data, false)
    }

    pub fn flush(&mut self) -> Result<(), esp_err_t> {
        self.set_column(0, (WIDTH - 1) as u16)?;
        self.set_row(0, (HEIGHT - 1) as u16)?;

        let mut first = true;
        let spi = self.spi;
        let dma = self.dma;
        for pixels in self.pixels.chunks(DMA_CHUNK) {
            let command = if first { RAM_WRITE } else { RAM_WRITE_CONTINUE };
            first = false;
            Self::write_pixel_chunk(spi, dma, command, pixels)?;
        }
        Ok(())
    }

    /// Envía solo una región del framebuffer sin mover ni asignar memoria.
    pub fn flush_rect(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), esp_err_t> {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x.saturating_add(width as i32)).clamp(0, WIDTH as i32) as usize;
        let y1 = (y.saturating_add(height as i32)).clamp(0, HEIGHT as i32) as usize;
        if x0 >= x1 || y0 >= y1 { return Ok(()); }

        let row_bytes = (x1 - x0) * 3;
        let transfer_len = row_bytes * (y1 - y0);
        // Las apps solo mandan rectángulos pequeños. Si alguien pide una región
        // grande, el refresco completo conserva el camino probado del panel.
        if transfer_len > DMA_CHUNK { return self.flush(); }

        self.set_column(x0 as u16, (x1 - 1) as u16)?;
        self.set_row(y0 as u16, (y1 - 1) as u16)?;
        let mut packed = 0;
        for row in y0..y1 {
            let start = (row * WIDTH + x0) * 3;
            unsafe { ptr::copy_nonoverlapping(self.pixels.as_ptr().add(start), self.dma.add(packed), row_bytes) };
            packed += row_bytes;
        }
        Self::write_dma_chunk(self.spi, self.dma, transfer_len, RAM_WRITE)
    }

    fn set_column(&mut self, start: u16, end: u16) -> Result<(), esp_err_t> {
        let start = start + COLUMN_OFFSET;
        let end = end + COLUMN_OFFSET;
        let args = [
            (start >> 8) as u8,
            start as u8,
            (end >> 8) as u8,
            end as u8,
        ];
        self.send_command_with_data(0x2a, &args)
    }

    fn set_row(&mut self, start: u16, end: u16) -> Result<(), esp_err_t> {
        let args = [
            (start >> 8) as u8,
            start as u8,
            (end >> 8) as u8,
            end as u8,
        ];
        self.send_command_with_data(0x2b, &args)
    }

    fn write_pixel_chunk(
        spi: spi_device_handle_t,
        dma: *mut u8,
        command: u8,
        data: &[u8],
    ) -> Result<(), esp_err_t> {
        let len = data.len().min(DMA_CHUNK);
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), dma, len) };
        Self::write_dma_chunk(spi, dma, len, command)
    }

    fn write_dma_chunk(
        spi: spi_device_handle_t,
        dma: *mut u8,
        len: usize,
        command: u8,
    ) -> Result<(), esp_err_t> {
        let mut transaction: spi_transaction_t = unsafe { core::mem::zeroed() };
        transaction.flags = SPI_TRANS_MODE_QIO;
        transaction.cmd = OPCODE_PIXELS;
        transaction.addr = (command as u64) << 8;
        transaction.length = len * 8;
        unsafe {
            transaction.__bindgen_anon_1 = spi_transaction_t__bindgen_ty_1 {
                tx_buffer: dma.cast_const().cast(),
            };
        }
        check(unsafe { spi_device_polling_transmit(spi, &mut transaction) })
    }

    fn transmit(
        &mut self,
        opcode: u16,
        command: u8,
        data: &[u8],
        quad_data: bool,
    ) -> Result<(), esp_err_t> {
        let mut transaction: spi_transaction_t = unsafe { core::mem::zeroed() };
        transaction.flags = if quad_data { SPI_TRANS_MODE_QIO } else { 0 };
        transaction.cmd = opcode;
        transaction.addr = (command as u64) << 8;
        transaction.length = data.len() * 8;
        if !data.is_empty() {
            unsafe {
                transaction.__bindgen_anon_1 = spi_transaction_t__bindgen_ty_1 {
                    tx_buffer: data.as_ptr().cast(),
                };
            }
        }
        check(unsafe { spi_device_polling_transmit(self.spi, &mut transaction) })
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        unsafe {
            heap_caps_free(self.dma.cast());
            spi_bus_remove_device(self.spi);
            spi_bus_free(spi_host_device_t_SPI2_HOST);
        }
    }
}

impl DrawTarget for Display {
    type Color = Rgb888;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(Point { x, y }, color) in pixels {
            if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
                continue;
            }
            let offset = (y as usize * WIDTH + x as usize) * 3;
            self.pixels[offset] = color.r();
            self.pixels[offset + 1] = color.g();
            self.pixels[offset + 2] = color.b();
        }
        Ok(())
    }
}

impl OriginDimensions for Display {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

fn check(error: esp_err_t) -> Result<(), esp_err_t> {
    if error == ESP_OK { Ok(()) } else { Err(error) }
}
