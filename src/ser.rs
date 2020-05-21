use std::io::Write;
use byteorder::{WriteBytesExt, BE};
use serde::{ser, Serialize};

use crate::types::*;

struct RencodeSerializer<W: Write>(W, Vec<usize>);

impl<W: Write> RencodeSerializer<W> {
    // This is a Vec, so if these writes fail, we have bigger problems.
    fn write_all(&mut self, buf: &[u8]) { self.0.write_all(buf).unwrap(); }
    fn write_u8(&mut self, n: u8) { self.0.write_u8(n).unwrap(); }
    fn write_i8(&mut self, n: i8) { self.0.write_i8(n).unwrap(); }
    fn write_i16(&mut self, n: i16) { self.0.write_i16::<BE>(n).unwrap(); }
    fn write_i32(&mut self, n: i32) { self.0.write_i32::<BE>(n).unwrap(); }
    fn write_i64(&mut self, n: i64) { self.0.write_i64::<BE>(n).unwrap(); }
    fn write_f32(&mut self, n: f32) { self.0.write_f32::<BE>(n).unwrap(); }
    fn write_f64(&mut self, n: f64) { self.0.write_f64::<BE>(n).unwrap(); }
}

pub fn to_writer(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    let mut serializer = RencodeSerializer(writer, vec![]);
    value.serialize(&mut serializer)?;
    serializer.0.flush().map_err(|e| ser::Error::custom(e))
}

pub fn to_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    to_writer(&mut buf, value)?;
    Ok(buf)
}

impl<'a, W: Write> ser::SerializeSeq for &'a mut RencodeSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> Result<()> {
        if self.1.pop().unwrap() >= LIST_COUNT {
            self.write_u8(types::TERM);
        }
        Ok(())
    }
}

impl<'a, W: Write> ser::SerializeTuple for &'a mut RencodeSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> Result<()> {
        if self.1.pop().unwrap() >= LIST_COUNT {
            self.write_u8(types::TERM);
        }
        Ok(())
    }
}

impl<'a, W: Write> ser::SerializeMap for &'a mut RencodeSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<()> {
        v.serialize(&mut **self)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> Result<()> {
        if self.1.pop().unwrap() >= DICT_COUNT {
            self.write_u8(types::TERM);
        }
        Ok(())
    }
}

impl<'a, W: Write> ser::SerializeStruct for &'a mut RencodeSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<()> {
        key.serialize(&mut **self)?;
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<()> {
        if self.1.pop().unwrap() >= DICT_COUNT {
            self.write_u8(types::TERM);
        }
        Ok(())
    }
}

type Impossible = ser::Impossible<(), Error>;
type Nope = Result<Impossible>;

impl<'a, W: Write> ser::Serializer for &'a mut RencodeSerializer<W> {
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;

    type SerializeTupleStruct = Impossible;
    type SerializeTupleVariant = Impossible;
    type SerializeStructVariant = Impossible;

    fn serialize_unit(self) -> Result<()> {
        self.write_u8(types::NONE);
        Ok(())
    }

    fn serialize_none(self) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<()> {
        v.serialize(self)
    }

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.write_u8(if v { types::TRUE } else { types::FALSE });
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<()> {
        match v {
            0..=INT_POS_MAX => {
                self.write_i8(INT_POS_START + v);
            }
            INT_NEG_MIN..=-1 => {
                self.write_i8(INT_NEG_START - 1 - v);
            }
            _ => {
                self.write_u8(types::INT1);
                self.write_i8(v);
            }
        }
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<()> {
        self.write_u8(types::INT2);
        self.write_i16(v);
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<()> {
        self.write_u8(types::INT4);
        self.write_i32(v);
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<()> {
        self.write_u8(types::INT8);
        self.write_i64(v);
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<()> {
        if v > std::i64::MAX as u64 {
            return Err(ser::Error::custom("unsigned integers are unsupported"));
        }
        self.serialize_i64(v as i64)
    }
    
    fn serialize_f32(self, v: f32) -> Result<()> {
        self.write_u8(types::FLOAT32);
        self.write_f32(v);
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<()> {
        self.write_u8(types::FLOAT64);
        self.write_f64(v);
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        let len = v.len();
        if len < STR_COUNT {
            self.write_u8(STR_START + len as u8);
        } else {
            self.write_all(format!("{}:", len).as_bytes());
        }
        self.write_all(v);
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        self.serialize_bytes(v.as_bytes())
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        self.serialize_tuple(len.ok_or(ser::Error::custom("try .collect()"))?)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        if len < LIST_COUNT {
            self.write_u8(LIST_START + len as u8);
        } else {
            self.write_u8(types::LIST);
        }
        self.1.push(len);
        Ok(self)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        let len = len.ok_or(ser::Error::custom("need to know map size ahead of time"))?;
        if len < DICT_COUNT {
            self.write_u8(DICT_START + len as u8);
        } else {
            self.write_u8(types::DICT);
        }
        self.1.push(len);
        Ok(self)
    }

    // Just treat structs as dicts.
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        if len < DICT_COUNT {
            self.write_u8(DICT_START + len as u8);
        } else {
            self.write_u8(types::DICT);
        }
        self.1.push(len);
        Ok(self)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &str, value: &T) -> Result<()> {
        value.serialize(self)
    }

    // Data types not supported by the real rencode
    fn serialize_char(self, _: char) -> Result<()> { unimplemented!() }
    fn serialize_u8(self, _: u8) -> Result<()> { unimplemented!() }
    fn serialize_u16(self, _: u16) -> Result<()> { unimplemented!() }
    fn serialize_u32(self, _: u32) -> Result<()> { unimplemented!() }
    fn serialize_struct_variant(self, _: &str, _: u32, _: &str, _: usize) -> Nope { unimplemented!() }
    fn serialize_unit_struct(self, _: &str) -> Result<()> { unimplemented!() }
    fn serialize_unit_variant(self, _: &str, _: u32, _: &str) -> Result<()> { unimplemented!() }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _: &str, _: u32, _: &str, _: &T) -> Result<()> { unimplemented!() }
    fn serialize_tuple_struct(self, _: &str, _: usize) -> Nope { unimplemented!() }
    fn serialize_tuple_variant(self, _: &str, _: u32, _: &str, _: usize) -> Nope { unimplemented!() }
}
