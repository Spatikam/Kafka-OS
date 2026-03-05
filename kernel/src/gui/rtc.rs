use x86_64::instructions::port::Port;

pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl DateTime {
    /// Safely applies a timezone offset, handling all calendar rollovers
    pub fn apply_timezone_offset(&mut self, offset_hours: i32, offset_minutes: i32) {
        // 1. Add Minutes
        let mut total_minutes = self.minute as i32 + offset_minutes;
        let mut extra_hours = 0;
        
        if total_minutes >= 60 {
            total_minutes -= 60;
            extra_hours = 1;
        } else if total_minutes < 0 {
            total_minutes += 60;
            extra_hours = -1;
        }
        self.minute = total_minutes as u8;

        // 2. Add Hours
        let mut total_hours = self.hour as i32 + offset_hours + extra_hours;
        let mut extra_days = 0;

        if total_hours >= 24 {
            total_hours -= 24;
            extra_days = 1;
        } else if total_hours < 0 {
            total_hours += 24;
            extra_days = -1;
        }
        self.hour = total_hours as u8;

        // 3. Add Days and Handle Month/Year Rollover
        if extra_days > 0 {
            self.day += 1;
            let days_this_month = days_in_month(self.year, self.month);
            
            if self.day > days_this_month {
                self.day = 1;
                self.month += 1;
                if self.month > 12 {
                    self.month = 1;
                    self.year += 1;
                }
            }
        } else if extra_days < 0 {
            self.day -= 1;
            
            if self.day == 0 {
                self.month -= 1;
                if self.month == 0 {
                    self.month = 12;
                    self.year -= 1;
                }
                self.day = days_in_month(self.year, self.month);
            }
        }
    }
}

pub struct RTC {
    addr_port: Port<u8>,
    data_port: Port<u8>,
}

impl RTC {
    pub fn new() -> Self {
        Self {
            addr_port: Port::new(0x70),
            data_port: Port::new(0x71),
        }
    }

    unsafe fn read_register(&mut self, reg: u8) -> u8 {
        self.addr_port.write(reg);
        self.data_port.read()
    }

    /// The CMOS updates internally. If we read while it's updating, we get garbage data.
    fn is_updating(&mut self) -> bool {
        unsafe { (self.read_register(0x0A) & 0x80) != 0 }
    }

    pub fn read_datetime(&mut self) -> DateTime {
        while self.is_updating() {
            core::hint::spin_loop();
        }

        let mut second = unsafe { self.read_register(0x00) };
        let mut minute = unsafe { self.read_register(0x02) };
        let mut hour = unsafe { self.read_register(0x04) };
        let mut day = unsafe { self.read_register(0x07) };
        let mut month = unsafe { self.read_register(0x08) };
        let mut year = unsafe { self.read_register(0x09) };

        let register_b = unsafe { self.read_register(0x0B) };

        // If the 3rd bit of Register B is 0, the data is in BCD format and needs decoding
        if (register_b & 0x04) == 0 {
            second = (second & 0x0F) + ((second / 16) * 10);
            minute = (minute & 0x0F) + ((minute / 16) * 10);
            // Handle the 12-hour AM/PM bit for hours
            hour = ((hour & 0x0F) + (((hour & 0x70) / 16) * 10)) | (hour & 0x80);
            day = (day & 0x0F) + ((day / 16) * 10);
            month = (month & 0x0F) + ((month / 16) * 10);
            year = (year & 0x0F) + ((year / 16) * 10);
        }

        DateTime {
            year: year as u16 + 2000, // Assuming 21st century
            month, day, hour, minute, second
        }
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            // Leap year calculation
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}
