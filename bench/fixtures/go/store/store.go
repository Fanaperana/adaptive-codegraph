package store

import "example.com/app/util"

type Store struct { // @def Store
	items []string
}

func New() *Store { // @def store::New
	return &Store{}
}

func (s *Store) Save(item string) { // @def Store::Save
	sum := util.Checksum(item)
	s.items = append(s.items, item+string(rune(sum)))
	s.flush()
}

func (s *Store) Len() int { // @def Store::Len
	return len(s.items)
}

func (s *Store) flush() { // @def Store::flush
	_ = len(s.items)
}
