use std::io::Read;
use byteorder::{ReadBytesExt, BE};
use serde::de::{self, Error as _, Deserializer, Deserialize, Visitor};

use crate::types::*;

struct RencodeDeserializer<'de, R> {
    data: R,
    returned_byte: Option<u8>,
    // TODO: try and get rid of this nightmare thing
    whatever: std::marker::PhantomData<&'de ()>,
}

pub fn from_reader<'de, T: Deserialize<'de>, R: Read>(data: R) -> Result<T> {
    let mut deserializer = RencodeDeserializer { data: data, returned_byte: None, whatever: Default::default() };
    let val = T::deserialize(&mut deserializer)?;
    if deserializer.read(&mut [0u8])? == 0 {
        return Err(Error::custom("too many bytes"))
    }
    Ok(val)
}

pub fn from_bytes<'de, T: Deserialize<'de>>(data: &'de [u8]) -> Result<T> {
    from_reader(data)
}

impl<'de, R: Read> Read for RencodeDeserializer<'de, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.len() == 0 { return Ok(0); }

        match self.returned_byte.take() {
            Some(x) => {
                buf[0] = x;
                self.data.read(&mut buf[1..])
            },
            None => self.data.read(buf),
        }
    }
}

impl<'de, R: Read> RencodeDeserializer<'de, R> {
    fn next_byte(&mut self) -> Result<u8> {
        let mut buf = [0u8];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn go_back<T>(&mut self, n: u8) -> Option<T> {
        match self.returned_byte.replace(n) {
            None => (),
            Some(_) => unreachable!("we should never take more than 2 steps back"),
        }
        None
    }

    fn next_unit(&mut self) -> Result<Option<()>> {
        let x = match self.next_byte()? {
            types::NONE => Some(()),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_bool(&mut self) -> Result<Option<bool>> {
        let x = match self.next_byte()? {
            types::TRUE => Some(true),
            types::FALSE => Some(false),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_i8(&mut self) -> Result<Option<i8>> {
        let x = match self.next_byte()? {
            types::INT1 => Some(self.read_i8()?),
            n @ 0..=43 => Some(INT_POS_START + n as i8),
            n @ 70..=101 => Some(70 - 1 - n as i8),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_i16(&mut self) -> Result<Option<i16>> {
        let x = match self.next_byte()? {
            types::INT2 => Some(self.read_i16::<BE>()?),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_i32(&mut self) -> Result<Option<i32>> {
        let x = match self.next_byte()? {
            types::INT4 => Some(self.read_i32::<BE>()?),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_i64(&mut self) -> Result<Option<i64>> {
        let x = match self.next_byte()? {
            types::INT8 => Some(self.read_i64::<BE>()?),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_f32(&mut self) -> Result<Option<f32>> {
        let x = match self.next_byte()? {
            types::FLOAT32 => Some(self.read_f32::<BE>()?),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_f64(&mut self) -> Result<Option<f64>> {
        let x = match self.next_byte()? {
            types::FLOAT64 => Some(self.read_f64::<BE>()?),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_bytes(&mut self) -> Result<Option<Vec<u8>>> {
        let x = match self.next_byte()? {
            n @ 49..=57 => {
                let mut len_bytes = vec![n];
                loop {
                    match self.next_byte()? {
                        n @ 48..=57 => len_bytes.push(n),
                        58 => break,
                        n => return Err(Error::custom(format!("Unexpected byte while parsing string length: {}", n))),
                    }
                }
                // Okay to unwrap because we know the only thing we put in there was ascii decimal digits
                let len_str = std::str::from_utf8(&len_bytes).unwrap();
                // Okay to unwrap because we know it's a decimal, and it's probably reasonably sized.
                // TODO: return Err when it's unreasonably large.
                let len: usize = len_str.parse().unwrap();
                let mut buf = Vec::with_capacity(len);
                self.read_exact(&mut buf)?;
                Some(buf)
            },
            n @ STR_START..=STR_END => {
                let len = (n - STR_START) as usize;
                let mut buf = Vec::with_capacity(len);
                self.read_exact(&mut buf)?;
                Some(buf)
            },
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_fixed_seq(&mut self) -> Result<Option<usize>> {
        let x = match self.next_byte()? {
            n @ LIST_START..=LIST_END => Some((n - LIST_START) as usize),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_fixed_map(&mut self) -> Result<Option<usize>> {
        let x = match self.next_byte()? {
            n @ DICT_START..=DICT_END => Some((n - LIST_START) as usize),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_terminated_seq(&mut self) -> Result<Option<()>> {
        let x = match self.next_byte()? {
            types::LIST => Some(()),
            n => self.go_back(n),
        }; Ok(x)
    }

    fn next_terminated_map(&mut self) -> Result<Option<()>> {
        let x = match self.next_byte()? {
            types::DICT => Some(()),
            n => self.go_back(n),
        }; Ok(x)
    }
}

impl<'de, 'a, R: Read> Deserializer<'de> for &'a mut RencodeDeserializer<'de, R> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(mut self, v: V) -> Result<V::Value> {
        if let Some(()) = self.next_unit()? {
            v.visit_unit()
        } else if let Some(x) = self.next_bool()? {
            v.visit_bool(x)
        } else if let Some(x) = self.next_i8()? {
            v.visit_i8(x)
        } else if let Some(x) = self.next_i16()? {
            v.visit_i16(x)
        } else if let Some(x) = self.next_i32()? {
            v.visit_i32(x)
        } else if let Some(x) = self.next_i64()? {
            v.visit_i64(x)
        } else if let Some(x) = self.next_f32()? {
            v.visit_f32(x)
        } else if let Some(x) = self.next_f64()? {
            v.visit_f64(x)
        } else if let Some(x) = self.next_bytes()? {
            match std::str::from_utf8(&x) {
                Ok(s) => v.visit_string(s.to_string()),
                Err(_) => v.visit_byte_buf(x),
            }
        } else if let Some(x) = self.next_fixed_seq()? {
            v.visit_seq(FixedSeq(&mut self, x))
        } else if let Some(x) = self.next_fixed_map()? {
            v.visit_map(FixedMap(&mut self, x, false))
        } else if let Some(()) = self.next_terminated_seq()? {
            v.visit_seq(TerminatedSeq(&mut self))
        } else if let Some(()) = self.next_terminated_map()? {
            v.visit_map(TerminatedMap(&mut self, false))
        } else {
            let e = match self.next_byte()? {
                types::INT => Error::custom("deserialization of bigints is unsupported at the time of writing"),
                58 => Error::custom("unexpected strlen terminator"),
                types::TERM => Error::custom("unexpected seq/map terminator"),
                n @ 45..=48 => Error::custom(format!("unexpected unknown type indicator: {}", n)),
                n => unreachable!("unexpectedly unhandled type indicator: {}", n),
            };
            Err(Error::custom(e))
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, _v: V) -> Result<V::Value> {
        todo!("gotta impl this...")
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct FixedSeq<'a, 'de: 'a, R: Read>(&'a mut RencodeDeserializer<'de, R>, usize);

impl<'de, 'a, R: Read> de::SeqAccess<'de> for FixedSeq<'a, 'de, R> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.1 == 0 {
            return Ok(None);
        }
        self.1 -= 1;
        seed.deserialize(&mut *self.0).map(Some)
    }
}

struct FixedMap<'a, 'de: 'a, R: Read>(&'a mut RencodeDeserializer<'de, R>, usize, bool);

impl<'de, 'a, R: Read> de::MapAccess<'de> for FixedMap<'a, 'de, R> {
    type Error = Error;

    fn next_key_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.2 {
            panic!("tried to get a key inappropriately");
        }
        if self.1 == 0 {
            return Ok(None);
        }
        self.2 = true;
        seed.deserialize(&mut *self.0).map(Some)
    }

    fn next_value_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value> {
        if !self.2 {
            panic!("tried to get a value inappropriately");
        }
        self.1 -= 1;
        self.2 = false;
        seed.deserialize(&mut *self.0)
    }
}

struct TerminatedSeq<'a, 'de: 'a, R: Read>(&'a mut RencodeDeserializer<'de, R>);

impl<'a, 'de: 'a, R: Read> de::SeqAccess<'de> for TerminatedSeq<'a, 'de, R> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        match self.0.next_byte()? {
            types::TERM => { return Ok(None); },
            n => { self.0.go_back::<()>(n); },
        }
        seed.deserialize(&mut *self.0).map(Some)
    }
}

struct TerminatedMap<'a, 'de: 'a, R: Read>(&'a mut RencodeDeserializer<'de, R>, bool);

impl<'a, 'de: 'a, R: Read> de::MapAccess<'de> for TerminatedMap<'a, 'de, R> {
    type Error = Error;

    fn next_key_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.1 {
            panic!("tried to get a key inappropriately");
        }
        match self.0.next_byte()? {
            types::TERM => { return Ok(None); },
            n => { self.0.go_back::<()>(n); },
        }
        self.1 = true;
        seed.deserialize(&mut *self.0).map(Some)
    }

    fn next_value_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value> {
        if !self.1 {
            panic!("tried to get a value inappropriately");
        }
        self.1 = false;
        seed.deserialize(&mut *self.0)
    }
}
