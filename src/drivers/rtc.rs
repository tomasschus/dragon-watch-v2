use embedded_hal::i2c::I2c;

#[derive(Clone, Copy)]
pub struct DateTime { pub year: u16, pub month: u8, pub day: u8, pub weekday: u8, pub hour: u8, pub minute: u8, pub second: u8 }
fn from_bcd(v: u8) -> u8 { (v >> 4) * 10 + (v & 15) }
fn to_bcd(v: u8) -> u8 { (v / 10) << 4 | v % 10 }
pub struct Pcf85063a<I2C> { i2c: I2C, addr: u8 }
impl<I2C, E> Pcf85063a<I2C> where I2C: I2c<Error = E> {
    pub fn new(i2c: I2C, addr: u8) -> Self { Self { i2c, addr } }
    pub fn init(&mut self) -> Result<(), E> { self.i2c.write(self.addr, &[0x00, 0x01]) }
    pub fn get(&mut self) -> Result<DateTime, E> {
        let mut r = [0; 7]; self.i2c.write_read(self.addr, &[0x04], &mut r)?;
        Ok(DateTime { second: from_bcd(r[0]&0x7f), minute: from_bcd(r[1]&0x7f), hour: from_bcd(r[2]&0x3f), day: from_bcd(r[3]&0x3f), weekday: from_bcd(r[4]&7), month: from_bcd(r[5]&0x1f), year: 2000 + from_bcd(r[6]) as u16 })
    }
    pub fn set(&mut self, d: DateTime) -> Result<(), E> {
        self.i2c.write(self.addr, &[0x00, 0x21])?;
        self.i2c.write(self.addr, &[0x04, to_bcd(d.second), to_bcd(d.minute), to_bcd(d.hour), to_bcd(d.day), to_bcd(d.weekday), to_bcd(d.month), to_bcd((d.year-2000) as u8)])?;
        self.i2c.write(self.addr, &[0x00, 0x01])
    }
}
