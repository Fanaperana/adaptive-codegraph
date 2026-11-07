use crate::util::checksum;

pub struct Store { // @def Store
    items: Vec<String>,
}

impl Store {
    pub fn new() -> Self { // @def Store::new
        Store { items: Vec::new() }
    }

    pub fn save(&mut self, item: &str) { // @def Store::save
        let sum = checksum(item);
        self.items.push(format!("{item}:{sum}"));
        self.flush();
    }

    pub fn len(&self) -> usize { // @def Store::len
        self.items.len()
    }

    fn flush(&self) { // @def Store::flush
        println!("flushing {}", self.items.len());
    }
}

pub trait Persist { // @def Persist
    fn persist(&self) -> bool; // @def Persist::persist
}

impl Persist for Store {
    fn persist(&self) -> bool { // @def Store::persist
        self.len() > 0
    }
}
