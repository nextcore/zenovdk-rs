pub struct GolombBitReader<'a> {
    data: &'a [u8],
    byte_offset: usize,
    bit_offset: u8,
}

impl<'a> GolombBitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_offset: 0,
            bit_offset: 8,
        }
    }

    pub fn read_bit(&mut self) -> Result<u32, &'static str> {
        if self.byte_offset >= self.data.len() {
            return Err("EOF");
        }
        if self.bit_offset == 0 {
            self.byte_offset += 1;
            self.bit_offset = 8;
            if self.byte_offset >= self.data.len() {
                return Err("EOF");
            }
        }
        self.bit_offset -= 1;
        let res = (self.data[self.byte_offset] >> self.bit_offset) & 1;
        Ok(res as u32)
    }

    pub fn read_bits(&mut self, n: u8) -> Result<u32, &'static str> {
        let mut res = 0;
        for i in 0..n {
            let bit = self.read_bit()?;
            res |= bit << (n - i - 1);
        }
        Ok(res)
    }

    pub fn read_exponential_golomb(&mut self) -> Result<u32, &'static str> {
        let mut zero_bits = 0;
        loop {
            let bit = self.read_bit()?;
            if bit != 0 || zero_bits >= 32 {
                break;
            }
            zero_bits += 1;
        }
        let val = self.read_bits(zero_bits)?;
        Ok(val + (1 << zero_bits) - 1)
    }

    pub fn read_se(&mut self) -> Result<i32, &'static str> {
        let ue = self.read_exponential_golomb()?;
        if ue % 2 != 0 {
            Ok(((ue + 1) / 2) as i32)
        } else {
            Ok(-((ue / 2) as i32))
        }
    }
}
