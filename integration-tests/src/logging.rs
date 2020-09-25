// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt;
use std::io;

use slog::{o, Logger, OwnedKVList, Record};
use slog_term::{Decorator, RecordDecorator};

/// Generate loggers for testing.
///
/// The loggers are prepended with the process type.
pub fn build_test_loggers() -> (Logger, Logger) {
    use slog::Drain;

    let runner_decorator = ProcessDecorator {
        inner: slog_term::PlainSyncDecorator::new(slog_term::TestStdoutWriter),
        process: Process::Runner,
    };

    let recorder_decorator = ProcessDecorator {
        inner: slog_term::PlainSyncDecorator::new(slog_term::TestStdoutWriter),
        process: Process::Recorder,
    };

    let runner_drain = slog_term::FullFormat::new(runner_decorator).build().fuse();
    let recorder_drain = slog_term::FullFormat::new(recorder_decorator)
        .build()
        .fuse();

    (
        Logger::root(runner_drain, o! {}),
        Logger::root(recorder_drain, o! {}),
    )
}

enum Process {
    Runner,
    Recorder,
}
impl fmt::Display for Process {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Process::Runner => "runner".fmt(f),
            Process::Recorder => "recorder".fmt(f),
        }
    }
}

struct ProcessDecorator<D> {
    inner: D,
    process: Process,
}

impl<D> Decorator for ProcessDecorator<D>
where
    D: Decorator,
{
    fn with_record<F>(&self, record: &Record, logger_values: &OwnedKVList, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut dyn RecordDecorator) -> io::Result<()>,
    {
        self.inner
            .with_record(record, logger_values, |record_decorator| {
                record_decorator.reset()?;
                write!(record_decorator, "[{:8}] ", self.process)?;
                f(record_decorator)
            })
    }
}
