mod store;
mod util;

use store::{Cache, Store};
use util::checksum as cs;

fn build() -> Store { // @def build
    let mut s = Store::new();
    s.save(1).save(2);
    s
}

fn make_cache() -> Cache { // @def make_cache
    Cache::new()
}

fn in_macro(x: u32) { // @def in_macro
    println!("{}", util::compute(x));
}

fn in_closure(words: &[&str]) -> Vec<usize> { // @def in_closure
    words.iter().map(|w| cs(w)).collect()
}

fn mentions() -> &'static str { // @def mentions
    // compute and checksum are called elsewhere
    "compute(), checksum() and Store::save() are documented here"
}

fn main() { // @def main
    let mut s = build();
    s.flush();
    let _ = make_cache();
    in_macro(3);
    let _ = in_closure(&["a"]);
    let _ = mentions();
}
