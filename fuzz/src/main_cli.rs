use std::io::Read;

use testcase::run;

fn main() {
    let mut input = Vec::new();
    std::io::stdin().lock().read_to_end(&mut input).unwrap();
    run(&input);
}
