use std::ops::Deref;
use std::panic::UnwindSafe;

use libtest_mimic::Failed;

use html5gum::testutils::OUTPUT;

pub fn catch_unwind_and_report(f: impl FnOnce() + UnwindSafe) -> Result<(), Failed> {
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
                msg.push_str(s);
            }

            Err(msg.into())
        }
    }
}
