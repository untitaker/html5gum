use std::io::Read;

use testcase::run;

fn main() {
    let mut input = String::new();
    std::io::stdin().lock().read_to_string(&mut input).unwrap();
    run(&input);
}
