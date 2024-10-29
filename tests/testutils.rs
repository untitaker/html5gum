use std::backtrace::BacktraceStatus;
use std::ops::Deref;
use std::sync::Once;
use std::panic::{self, UnwindSafe};

use libtest_mimic::Failed;

use html5gum::testutils::OUTPUT;

/// Improved panic messages for the html5lib-tests test suite.
///
/// This function catches panics, prepends the log message to the panic message, and captures
/// stacktraces.
///
/// libtest_mimic already catches panics but doesn't provide stacktraces for some reason.
///
/// Because custom test harnesses in Rust do not support capturing of stdout, we have to implement
/// our own "log buffer", and append it to the error message in case of test failure. OUTPUT is the
/// log buffer -- it is bound in size and compiled out in release mode. Code can use the
/// `crate::trace_log` macro to write lines to it.
pub fn catch_unwind_and_report(f: impl FnOnce() + UnwindSafe) -> Result<(), Failed> {
    static PANIC_HOOK: Once = Once::new();
    PANIC_HOOK.call_once(|| {
        panic::set_hook(Box::new(|_info| {
            let backtrace = std::backtrace::Backtrace::capture();
            if backtrace.status() != BacktraceStatus::Captured {
                html5gum::testutils::trace_log("PANIC BACKTRACE: did not capture, use RUST_BACKTRACE=1");
            } else {
                // clean up noisy frames from backtrace
                let mut backtrace_str = String::new();
                let mut seen_begin_unwind = false;
                for line in format!("{:#?}", backtrace).lines() {
                    if line.contains("\"std::panicking::try::do_call\"") {
                        break;
                    } else if seen_begin_unwind {
                        backtrace_str.push_str(line);
                        backtrace_str.push('\n');
                    } else if line.contains("\"core::panicking::panic_fmt\"") {
                        seen_begin_unwind = true;
                    }
                }
                html5gum::testutils::trace_log(&format!("\nPANIC BACKTRACE:\n{}", backtrace_str));
            }
        }));
    });

    let result = std::panic::catch_unwind(f);

    let mut msg = String::new();

    OUTPUT.with(|cell| {
        let mut buf = cell.take();
        msg.push_str(&buf);
        buf.clear();
        cell.set(buf);
    });

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            msg.push('\n');
            if let Some(s) = e
                // Try to convert it to a String, then turn that into a str
                .downcast_ref::<String>()
                .map(String::as_str)
                // If that fails, try to turn it into a &'static str
                .or_else(|| e.downcast_ref::<&'static str>().map(Deref::deref))
            {
                msg.push_str("PANIC: ");
                msg.push_str(s);
            }

            Err(msg.into())
        }
    }
}
