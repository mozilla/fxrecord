// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt;
use std::io;

use chrono::Utc;
use slog::{Drain, Key, Logger, OwnedKVList, Record, Serializer, KV};
use slog_term::{Decorator, RecordDecorator, TermDecorator};

// RFC3339 timestamp with millisecond precision.
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3fZ";

/// Create a logger.
pub fn build_logger() -> Logger {
    let decorator = TermDecorator::new().stderr().force_plain().build();
    let drain = MultiLineDrain { decorator }.fuse();

    let drain = slog_async::Async::new(drain).build().fuse();

    Logger::root(drain, slog::o! {})
}

/// A drain that serializes each key-value pair on their own line, indented from
/// the logged message.
struct MultiLineDrain<D> {
    decorator: D,
}

impl<D> Drain for MultiLineDrain<D>
where
    D: Decorator,
{
    type Ok = ();
    type Err = io::Error;

    fn log(&self, record: &Record, values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        self.decorator
            .with_record(record, values, |record_decorator| {
                record_decorator.start_timestamp()?;
                write!(record_decorator, "{}", Utc::now().format(TIMESTAMP_FORMAT))?;

                record_decorator.start_whitespace()?;
                write!(record_decorator, " ")?;

                record_decorator.start_level()?;
                write!(record_decorator, "{}", record.level().as_str())?;

                record_decorator.start_whitespace()?;
                write!(record_decorator, " ")?;

                record_decorator.start_msg()?;
                write!(record_decorator, "{}", record.msg())?;

                record_decorator.start_whitespace()?;
                writeln!(record_decorator)?;

                let mut serializer = MultiLineSerializer { record_decorator };
                record.kv().serialize(record, &mut serializer)?;
                values.serialize(record, &mut serializer)?;

                Ok(())
            })
    }
}

/// A serializer that writes each key-value pair on its own line.
///
/// Mutliline strings are formatted indented from the key.
struct MultiLineSerializer<'a> {
    record_decorator: &'a mut dyn RecordDecorator,
}

impl<'a> MultiLineSerializer<'a> {
    /// Emit the key of a key-value pair.
    fn emit_key(&mut self, key: Key) -> Result<(), slog::Error> {
        self.record_decorator.start_whitespace()?;
        write!(self.record_decorator, "  ")?;
        self.record_decorator.start_key()?;
        write!(self.record_decorator, "{}", key)?;
        self.record_decorator.start_separator()?;
        write!(self.record_decorator, ":")?;

        Ok(())
    }

    /// Emit a displayable value.
    fn emit<D>(&mut self, key: Key, val: D) -> Result<(), slog::Error>
    where
        D: fmt::Display,
    {
        self.emit_key(key)?;
        self.record_decorator.start_whitespace()?;
        write!(self.record_decorator, " ")?;
        self.record_decorator.start_value()?;
        write!(self.record_decorator, "{}", val)?;
        self.record_decorator.start_whitespace()?;
        writeln!(self.record_decorator)?;

        Ok(())
    }

    /// Emit a multi-line string value.
    fn emit_lines(&mut self, key: Key, val: &str) -> Result<(), slog::Error> {
        self.emit_key(key)?;
        self.record_decorator.start_whitespace()?;
        writeln!(self.record_decorator)?;

        for line in val.lines() {
            self.record_decorator.start_whitespace()?;
            write!(self.record_decorator, "    ")?;
            self.record_decorator.start_value()?;
            write!(self.record_decorator, "{}", line)?;
            self.record_decorator.start_whitespace()?;
            writeln!(self.record_decorator)?;
        }

        Ok(())
    }
}

impl<'a> Serializer for MultiLineSerializer<'a> {
    fn emit_arguments(&mut self, key: Key, val: &fmt::Arguments) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_usize(&mut self, key: Key, val: usize) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_isize(&mut self, key: Key, val: isize) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_bool(&mut self, key: Key, val: bool) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_u8(&mut self, key: Key, val: u8) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_i8(&mut self, key: Key, val: i8) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_u16(&mut self, key: Key, val: u16) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_i16(&mut self, key: Key, val: i16) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_u32(&mut self, key: Key, val: u32) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_i32(&mut self, key: Key, val: i32) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_f32(&mut self, key: Key, val: f32) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_u64(&mut self, key: Key, val: u64) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_i64(&mut self, key: Key, val: i64) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_f64(&mut self, key: Key, val: f64) -> Result<(), slog::Error> {
        self.emit(key, val)
    }

    fn emit_str(&mut self, key: Key, val: &str) -> Result<(), slog::Error> {
        if val.contains('\n') {
            self.emit_lines(key, val)
        } else {
            self.emit(key, val)
        }
    }

    fn emit_unit(&mut self, key: Key) -> Result<(), slog::Error> {
        self.emit(key, "()")
    }

    fn emit_none(&mut self, key: Key) -> Result<(), slog::Error> {
        self.emit(key, "None")
    }
}
