mod cache;
mod other;
mod store;
mod util;

use crate::cache::Cache;
use crate::store::Store;
use crate::util::{checksum, helper};

#[derive(Debug)]
enum Command { // @def Command
    Find(String), // @def Command::Find
    Quit, // @def Command::Quit
}

fn main() { // @def main
    let mut store = Store::new();
    store.save("a");
    let cache = Cache::new(8);
    cache.save("b");
    let total = store.len() + cache.len();
    let names = vec!["x", "y"];
    let hit = names.iter().find(|n| n.len() == 1);
    helper();
    other::helper();
    run(Command::Find("q".into()));
    println!("{total} {hit:?} {}", checksum("z"));
}

fn run(cmd: Command) { // @def run
    match cmd {
        Command::Find(q) => {
            let r = search(&q, 0);
            println!("{r}");
        }
        Command::Quit => {}
    }
}

fn search(q: &str, depth: u32) -> usize { // @def search
    if depth > 3 {
        return q.len();
    }
    search(q, depth + 1)
}

fn process(store: &mut Store) { // @def process
    let apply = |s: &str| {
        let c = checksum(s);
        c
    };
    store.save(&apply("k").to_string());
}

#[cfg(test)]
mod tests { // @def tests
    use super::*;

    #[test]
    fn test_search() { // @def tests::test_search
        assert_eq!(search("ab", 0), 2);
    }

    #[test]
    fn test_process() { // @def tests::test_process
        let mut s = Store::new();
        process(&mut s);
        assert_eq!(s.len(), 1);
    }
}
