use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::I2c;

pub struct Ft5x06<I2C, RST> { i2c: I2C, addr: u8, rst: RST }

impl<I2C, RST, E, PinE> Ft5x06<I2C, RST>
where I2C: I2c<Error = E>, RST: OutputPin<Error = PinE> {
    pub fn new(i2c: I2C, addr: u8, rst: RST) -> Self { Self { i2c, addr, rst } }
    pub fn init<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), E> {
        self.rst.set_low().ok(); delay.delay_ms(10); self.rst.set_high().ok(); delay.delay_ms(200);
        self.i2c.write(self.addr, &[0xA4, 0x01])
    }
    pub fn read(&mut self) -> Result<Option<(u16, u16)>, E> {
        let mut count = [0]; self.i2c.write_read(self.addr, &[0x02], &mut count)?;
        if count[0] & 0x0f == 0 { return Ok(None); }
        let mut p = [0; 4]; self.i2c.write_read(self.addr, &[0x03], &mut p)?;
        Ok(Some(((((p[0] & 0x0f) as u16) << 8) | p[1] as u16, (((p[2] & 0x0f) as u16) << 8) | p[3] as u16)))
    }
    pub fn sleep(&mut self) -> Result<(), E> { self.i2c.write(self.addr, &[0xA5, 0x03]) }
}
