use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;
use volatile::Volatile;

static mut PHYSICAL_MEMORY_OFFSET: u64 = 0;
static mut VGA_INITIALIZED: bool = false;

/// Call this as early as possible in kernel_main, right after memory::init().
pub unsafe fn init_vga_offset(offset: u64) {
    PHYSICAL_MEMORY_OFFSET = offset;
    VGA_INITIALIZED = true;
}

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
    });
}

/// The standard color palette in VGA text mode.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A combination of a foreground and a background color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A screen character in the VGA text buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

/// The VGA text buffer structure.
#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
}

impl Writer {
    fn get_buffer(&mut self) -> &mut Buffer {
        // SAFETY: If VGA is not yet initialized, fall back to raw 0xb8000.
        // This is safe during very early boot before memory::init() runs
        // because the bootloader identity-maps low memory.
        // After init_vga_offset() is called, we use the correct mapped address.
        let vga_addr = 0xb8000 + unsafe { PHYSICAL_MEMORY_OFFSET };
        unsafe { &mut *(vga_addr as *mut Buffer) }
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\x08' => {
                if self.column_position > 0 {
                    self.column_position -= 1;
                }
            }
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;
                let color_code = self.color_code;
                self.get_buffer().chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                self.column_position += 1;
            }
        }
    }

    fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' | b'\x08' => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
    }

    fn new_line(&mut self) {
        let buffer = self.get_buffer();
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = buffer.chars[row][col].read();
                buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let color_code = self.color_code;
        let buffer = self.get_buffer();
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code,
        };
        for col in 0..BUFFER_WIDTH {
            buffer.chars[row][col].write(blank);
        }
    }

    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.column_position = 0;
    }

    pub fn set_color(&mut self, foreground: Color, background: Color) {
        self.color_code = ColorCode::new(foreground, background);
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// Like the `print!` macro in the standard library, but prints to the VGA text buffer.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

/// Like the `println!` macro in the standard library, but prints to the VGA text buffer.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Internal print function — writes to both serial and VGA.
/// Serial always works. VGA is skipped if not yet initialized to avoid
/// a recursive page fault inside the fault handler itself.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    // Always print to serial first — this works even before VGA is mapped
    // and survives inside exception handlers safely.
    crate::serial::_print(args);

    // Only write to VGA if the offset has been set, to avoid a page fault
    // caused by writing to an unmapped address inside an exception handler.
    if unsafe { VGA_INITIALIZED } {
        interrupts::without_interrupts(|| {
            WRITER.lock().write_fmt(args).unwrap();
        });
    }
}

/// Clears the screen (public wrapper).
pub fn clear_screen() {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        WRITER.lock().clear_screen();
    });
}

/// Sets the foreground/background color (public wrapper).
pub fn set_color(foreground: Color, background: Color) {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        WRITER.lock().set_color(foreground, background);
    });
}

// --- TESTS ---

#[test_case]
fn test_println_simple() {
    println!("test_println_simple output");
}

#[test_case]
fn test_println_many() {
    for _ in 0..200 {
        println!("test_println_many output");
    }
}

#[test_case]
fn test_println_output() {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    let s = "Some test string that fits on a single line";
    interrupts::without_interrupts(|| {
        let mut writer = WRITER.lock();
        writeln!(writer, "\n{}", s).expect("writeln failed");

        let vga_addr = 0xb8000 + unsafe { PHYSICAL_MEMORY_OFFSET };
        let buffer = unsafe { &*(vga_addr as *const Buffer) };

        for (i, c) in s.chars().enumerate() {
            let screen_char = buffer.chars[BUFFER_HEIGHT - 2][i].read();
            assert_eq!(char::from(screen_char.ascii_character), c);
        }
    });
}