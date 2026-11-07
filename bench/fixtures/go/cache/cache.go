package cache

type Cache struct { // @def Cache
	data map[string]int
	cap  int
}

func NewCache(cap int) *Cache { // @def cache::NewCache
	return &Cache{data: map[string]int{}, cap: cap}
}

func (c *Cache) Save(key string) { // @def Cache::Save
	if len(c.data) >= c.cap {
		c.evict()
	}
	c.data[key] = 1
}

func (c *Cache) Len() int { // @def Cache::Len
	return len(c.data)
}

func (c *Cache) evict() { // @def Cache::evict
	fresh := NewCache(c.cap)
	_ = fresh.Len()
}
