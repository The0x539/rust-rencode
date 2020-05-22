use std::io::Read;
use byteorder::{ReadBytesExt, BE};
use serde::de::{Error as _, Deserializer, Deserialize, Visitor, DeserializeSeed, SeqAccess, MapAccess};

use crate::types::*;

struct RencodeDeserializer<R: Read> {
    data: R,
    returned_byte: Option<u8>,
}

pub fn from_reader<'de, T: Deserialize<'de>>(data: impl Read) -> Result<T> {
    let mut deserializer = RencodeDeserializer { data: data, returned_byte: None };
    let val = T::deserialize(&mut deserializer)?;
    if deserializer.read(&mut [0u8])? > 0 {
        return Err(Error::custom("too many bytes"))
    }
    Ok(val)
}

pub fn from_bytes<'de, T: Deserialize<'de>>(data: &'de [u8]) -> Result<T> {
    from_reader(data)
}

impl<R: Read> Read for RencodeDeserializer<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // It wouldn't make sense for every call to go_back() not to directly precede a call to next_byte().
        // If self.returned_byte isn't None, one of two things is the case.
        // A: Something is wrong with this deserializer module's logic.
        // B: Something is wrong with my understanding of its logic.
        assert!(self.returned_byte.is_none());
        self.data.read(buf)
    }
}

impl<R: Read> RencodeDeserializer<R> {
    fn next_byte(&mut self) -> Result<u8> {
        if let Some(x) = self.returned_byte.take() {
            return Ok(x);
        }

        let mut buf = [0u8];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn go_back(&mut self, n: u8) {
        match self.returned_byte.replace(n) {
            None => (),
            Some(_) => unreachable!("we should never take more than 2 steps back"),
        }
    }

    fn next_bytes(&mut self, x: u8) -> Result<Vec<u8>> {
        let len: usize = match x {
            n @ 49..=57 => {
                let mut len_bytes = Vec::with_capacity(usize::MAX.to_string().len());
                len_bytes.push(n);
                loop {
                    match self.next_byte()? {
                        // Accept '0' as subsequent digits, but not as the intial digit.
                        n @ 48..=57 => len_bytes.push(n),
                        58 => break,
                        n => return Err(Error::custom(format!("Unexpected byte while parsing string length: {}", n))),
                    }
                }
                // Okay to unwrap because we know the only thing we put in there was ascii decimal digits
                let len_str = std::str::from_utf8(&len_bytes).unwrap();
                match len_str.parse() {
                    Ok(len) => len,
                    Err(e) => return Err(Error::custom(e)),
                }
            },
            n @ STR_START..=STR_END => (n - STR_START) as usize,
            // Okay to panic because this is a private function in a private struct.
            // If this module is correct, this case will never happen.
            _ => unreachable!("RencodeDeserializer::next_bytes should only be called with x in {}..={} or {}..={}.",
                              49, 57,
                              STR_START, STR_END),
        };
        let mut buf = vec![0u8; len];
        self.read_exact(&mut buf)?;
        Ok(buf)
    }
}

impl<'de, 'a, R: Read> Deserializer<'de> for &'a mut RencodeDeserializer<R> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        match self.next_byte()? {
            types::NONE => v.visit_unit(),
            types::TRUE => v.visit_bool(true),
            types::FALSE => v.visit_bool(false),
            types::INT1 => v.visit_i8(self.read_i8()?),
            types::INT2 => v.visit_i16(self.read_i16::<BE>()?),
            types::INT4 => v.visit_i32(self.read_i32::<BE>()?),
            types::INT8 => v.visit_i64(self.read_i64::<BE>()?),
            types::INT => unimplemented!("bigint deserialization is unsupported at the time of writing"),

            types::FLOAT32 => v.visit_f32(self.read_f32::<BE>()?),
            types::FLOAT64 => v.visit_f64(self.read_f64::<BE>()?),

            x @ 0..=43 => v.visit_i8(INT_POS_START + x as i8),
            x @ 70..=101 => v.visit_i8(70 - 1 - x as i8),

            x @ STR_START..=STR_END | x @ 49..=57 => {
                let byte_buf = self.next_bytes(x)?;
                // If the string is valid UTF-8, treat it as a String.
                // Otherwise, treat it as the Vec<u8> it is.
                // Python went to so much trouble to use strongly typed Unicode strings,
                // and rencode just goes and treats `bytes` and `str` the same. Ugh.
                match std::str::from_utf8(&byte_buf) {
                    Ok(s) => v.visit_string(s.to_string()),
                    Err(_) => v.visit_byte_buf(byte_buf),
                }
            }

            x @ LIST_START..=LIST_END => v.visit_seq(FixedSeq(self, (x - LIST_START) as usize)),
            types::LIST => v.visit_seq(TerminatedSeq(self)),

            x @ DICT_START..=DICT_END => v.visit_map(FixedMap(self, (x - DICT_START) as usize, false)),
            types::DICT => v.visit_map(TerminatedMap(self, false)),

            58 => Err(Error::custom("unexpected strlen terminator")),
            types::TERM => Err(Error::custom("unexpected seq/map terminator")),
            x @ 45..=48 => Err(Error::custom(format!("unexpected unrecognized datatype indicator: {}", x))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        match self.next_byte()? {
            types::NONE => v.visit_none(),
            x => {
                self.go_back(x);
                v.visit_some(self)
            }
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct FixedSeq<'a, R: Read>(&'a mut RencodeDeserializer<R>, usize);

impl<'de, 'a, R: Read> SeqAccess<'de> for FixedSeq<'a, R> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.1 == 0 {
            return Ok(None);
        }
        self.1 -= 1;
        seed.deserialize(&mut *self.0).map(Some)
    }
}

struct FixedMap<'a, R: Read>(&'a mut RencodeDeserializer<R>, usize, bool);

impl<'de, 'a, R: Read> MapAccess<'de> for FixedMap<'a, R> {
    type Error = Error;

    fn next_key_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        assert!(!self.2, "inappropriate key access");
        if self.1 == 0 {
            return Ok(None);
        }
        self.2 = true;
        seed.deserialize(&mut *self.0).map(Some)
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value> {
        assert!(self.2, "inappropriate value access");
        self.1 -= 1;
        self.2 = false;
        seed.deserialize(&mut *self.0)
    }
}

struct TerminatedSeq<'a, R: Read>(&'a mut RencodeDeserializer<R>);

impl<'a, 'de: 'a, R: Read> SeqAccess<'de> for TerminatedSeq<'a, R> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        match self.0.next_byte()? {
            types::TERM => return Ok(None),
            n => self.0.go_back(n),
        }
        seed.deserialize(&mut *self.0).map(Some)
    }
}

struct TerminatedMap<'a, R: Read>(&'a mut RencodeDeserializer<R>, bool);

impl<'a, 'de: 'a, R: Read> MapAccess<'de> for TerminatedMap<'a, R> {
    type Error = Error;

    fn next_key_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        assert!(!self.1, "inappropriate key access");
        match self.0.next_byte()? {
            types::TERM => return Ok(None),
            n => self.0.go_back(n),
        }
        self.1 = true;
        seed.deserialize(&mut *self.0).map(Some)
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value> {
        assert!(self.1, "inappropriate value access");
        self.1 = false;
        seed.deserialize(&mut *self.0)
    }
}
