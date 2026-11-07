use std::collections::HashMap;

pub struct Cache { // @def Cache
    map: HashMap<String, usize>,
    cap: usize,
}

impl Cache {
    pub fn new(cap: usize) -> Self { // @def Cache::new
        Cache { map: HashMap::new(), cap }
    }

    pub fn save(&self, key: &str) { // @def Cache::save
        if self.map.len() < self.cap {
            self.evict();
        }
        let _ = self.map.get(key);
    }

    pub fn len(&self) -> usize { // @def Cache::len
        self.map.len()
    }

    fn evict(&self) { // @def Cache::evict
        let fresh = Cache::new(self.cap);
        let _ = fresh.len();
    }
}
