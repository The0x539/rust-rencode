pub mod types {
    pub const LIST: u8 = 59;
    pub const DICT: u8 = 60;
    #[allow(dead_code)]
    pub const INT: u8 = 61;
    pub const INT1: u8 = 62;
    pub const INT2: u8 = 63;
    pub const INT4: u8 = 64;
    pub const INT8: u8 = 65;
    pub const FLOAT32: u8 = 66;
    pub const FLOAT64: u8 = 44;
    pub const TRUE: u8 = 67;
    pub const FALSE: u8 = 68;
    pub const NONE: u8 = 69;
    pub const TERM: u8 = 127;
}

pub const INT_POS_START: i8 = 0;
pub const INT_POS_MAX: i8 = 43;

pub const INT_NEG_START: i8 = 70;
pub const INT_NEG_MIN: i8 = -32;

pub const STR_START: u8 = 128;
pub const STR_COUNT: usize = 64;
pub const STR_END: u8 = STR_START - 1 + STR_COUNT as u8;

pub const LIST_START: u8 = STR_START + STR_COUNT as u8;
pub const LIST_COUNT: usize = 64;
pub const LIST_END: u8 = LIST_START - 1 + LIST_COUNT as u8;

pub const DICT_START: u8 = 102;
pub const DICT_COUNT: usize = 25;
pub const DICT_END: u8 = DICT_START - 1 + DICT_COUNT as u8;

// TODO: more meaningful information contents
#[derive(Debug)]
pub struct Error(String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl std::error::Error for Error {}

impl std::convert::From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(format!("{:?}", value))
    }
}

impl serde::de::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}
impl serde::ser::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

pub type Result<T> = std::result::Result<T, self::Error>;
