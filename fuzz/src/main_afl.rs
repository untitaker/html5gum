use testcase::run;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        run(data);
    });
}
