pub struct Store { // @def Store
    pub items: Vec<u32>,
}

impl Store {
    pub fn new() -> Self { // @def Store::new
        Self { items: Vec::new() }
    }

    pub fn save(&mut self, x: u32) -> &mut Self { // @def Store::save
        self.items.push(x);
        self
    }

    pub fn flush(&mut self) { // @def Store::flush
        Self::reset(self);
    }

    fn reset(s: &mut Store) { // @def Store::reset
        s.items.clear();
    }
}

pub struct Cache { // @def Cache
    pub hits: u32,
}

impl Cache {
    pub fn new() -> Self { // @def Cache::new
        Cache { hits: 0 }
    }
}
