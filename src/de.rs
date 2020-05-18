use std::io::Read;
use byteorder::{ReadBytesExt, BE};
use serde::{de, Deserialize, de::DeserializeOwned};

use crate::types::*;

struct RencodeDeserializer<'de> { data: &'de [u8] }

pub fn from_bytes<'a, T: Deserialize<'a>>(data: &'a [u8]) -> Result<T> {
    let mut deserializer = RencodeDeserializer { data };
    let val = T::deserialize(&mut deserializer)?;
    if deserializer.data.len() == 0 {
        Ok(val)
    } else {
        Err(de::Error::custom("too many bytes"))
    }
}

pub fn from_reader<T: DeserializeOwned>(data: impl Read) -> Result<T> {
    // TODO: not this
    from_bytes(data.bytes().collect::<std::io::Result<Vec<u8>>>().unwrap().as_slice())
}

impl<'de> RencodeDeserializer<'de> {
    fn advance(&mut self, n: usize) {
        self.data = &self.data[n..];
    }

    fn peek_byte(&self) -> u8 {
        self.data[0]
    }

    fn next_byte(&mut self) -> u8 {
        let val = self.peek_byte();
        self.advance(1);
        val
    }

    fn peek_slice(&self, n: usize) -> &'de [u8] {
        &self.data[..n]
    }

    fn next_slice(&mut self, n: usize) -> &'de [u8] {
        let val = self.peek_slice(n);
        self.advance(n);
        val
    }

    fn next_i8(&mut self) -> i8 { self.next_slice(1).read_i8().unwrap() }
    fn next_i16(&mut self) -> i16 { self.next_slice(2).read_i16::<BE>().unwrap() }
    fn next_i32(&mut self) -> i32 { self.next_slice(4).read_i32::<BE>().unwrap() }
    fn next_i64(&mut self) -> i64 { self.next_slice(8).read_i64::<BE>().unwrap() }

    fn next_f32(&mut self) -> f32 { self.next_slice(4).read_f32::<BE>().unwrap() }
    fn next_f64(&mut self) -> f64 { self.next_slice(8).read_f64::<BE>().unwrap() }

    fn next_str_fixed(&mut self, len: usize) -> &'de str {
        std::str::from_utf8(self.next_slice(len)).unwrap()
    }

    fn next_str_terminated(&mut self, first_byte: u8) -> &'de str {
        // this code assumes well-formed input
        let mut splitn = self.data.splitn(2, |&x| x == 58);
        let mut len_bytes: Vec<u8> = splitn.next().unwrap().to_vec();
        len_bytes.insert(0, first_byte); // this is the only time we'd need to peek for deserialize_any
        let len_str: &str = std::str::from_utf8(&len_bytes).unwrap();
        self.advance(len_str.len()); // the missing first byte and the terminating ':' cancel each other out
        let len: usize = len_str.parse().unwrap();
        std::str::from_utf8(self.next_slice(len)).unwrap_or("some non-utf8 nonsense")
    }
}

struct FixedLengthSeq<'a, 'de: 'a>(&'a mut RencodeDeserializer<'de>, usize);

impl<'de, 'a> de::SeqAccess<'de> for FixedLengthSeq<'a, 'de> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.1 == 0 {
            return Ok(None);
        }
        self.1 -= 1;
        seed.deserialize(&mut *self.0).map(Some)
    }
}

struct FixedLengthMap<'a, 'de: 'a>(&'a mut RencodeDeserializer<'de>, usize, bool);

impl<'de, 'a> de::MapAccess<'de> for FixedLengthMap<'a, 'de> {
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

struct TerminatedSeq<'a, 'de: 'a>(&'a mut RencodeDeserializer<'de>);

impl<'de, 'a> de::SeqAccess<'de> for TerminatedSeq<'a, 'de> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.0.peek_byte() == types::TERM {
            self.0.advance(1);
            return Ok(None);
        }
        seed.deserialize(&mut *self.0).map(Some)
    }
}

struct TerminatedMap<'a, 'de: 'a>(&'a mut RencodeDeserializer<'de>, bool);

impl<'de, 'a> de::MapAccess<'de> for TerminatedMap<'a, 'de> {
    type Error = Error;

    fn next_key_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.1 {
            panic!("tried to get a key inappropriately");
        }
        if self.0.peek_byte() == types::TERM {
            self.0.advance(1);
            return Ok(None);
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

impl<'de, 'a> de::Deserializer<'de> for &'a mut RencodeDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.next_byte() {
            types::NONE => visitor.visit_unit(),
            types::TRUE => visitor.visit_bool(true),
            types::FALSE => visitor.visit_bool(false),
            types::INT1 => visitor.visit_i8(self.next_i8()),
            types::INT2 => visitor.visit_i16(self.next_i16()),
            types::INT4 => visitor.visit_i32(self.next_i32()),
            types::INT8 => visitor.visit_i64(self.next_i64()),
            types::INT => unimplemented!(),
            
            types::FLOAT32 => visitor.visit_f32(self.next_f32()),
            types::FLOAT64 => visitor.visit_f64(self.next_f64()),

            x @ 0..=43 => visitor.visit_i8(INT_POS_START + x as i8),
            x @ 70..=101 => visitor.visit_i8(70 - 1 - x as i8),

            x @ STR_START..=STR_END => visitor.visit_borrowed_str(self.next_str_fixed((x - STR_START) as usize)),
            x @ 49..=57 => visitor.visit_borrowed_str(self.next_str_terminated(x)),
            58 => Err(de::Error::custom("unexpected strlen terminator")),

            x @ LIST_START..=LIST_END => visitor.visit_seq(FixedLengthSeq(self, (x - LIST_START) as usize)),
            types::LIST => visitor.visit_seq(TerminatedSeq(self)),

            x @ DICT_START..=DICT_END => visitor.visit_map(FixedLengthMap(self, (x - DICT_START) as usize, false)),
            types::DICT => visitor.visit_map(TerminatedMap(self, false)),

            types::TERM => Err(de::Error::custom("unexpected list/dict terminator")),

            45..=48 => Err(de::Error::custom("I don't know what values 45-48 are supposed to mean")),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}
