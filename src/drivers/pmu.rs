use embedded_hal::i2c::I2c;

/// Solo administra los cuatro rails necesarios para panel/touch; no activa ADC ni sensores.
pub struct Axp2101<I2C> { i2c: I2C, addr: u8 }
impl<I2C, E> Axp2101<I2C> where I2C: I2c<Error = E> {
    pub fn new(i2c: I2C, addr: u8) -> Self { Self { i2c, addr } }
    pub fn screen_power(&mut self, on: bool) -> Result<(), E> {
        let mut a = [0]; let mut b = [0];
        self.i2c.write_read(self.addr, &[0x90], &mut a)?; self.i2c.write_read(self.addr, &[0x91], &mut b)?;
        let a = if on { a[0] | 0xc0 } else { a[0] & !0xc0 };
        let b = if on { b[0] | 0x30 } else { b[0] & !0x30 };
        self.i2c.write(self.addr, &[0x90, a])?; self.i2c.write(self.addr, &[0x91, b])
    }

    /// Porcentaje calculado por el fuel gauge del PMU. Sólo se consulta al
    /// redibujar el reloj, no mientras se lee el resumen.
    pub fn battery_percent(&mut self) -> Result<u8, E> {
        let mut value = [0];
        self.i2c.write_read(self.addr, &[0xa4], &mut value)?;
        Ok((value[0] & 0x7f).min(100))
    }
}
