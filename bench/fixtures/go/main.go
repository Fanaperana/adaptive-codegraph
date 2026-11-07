package main

import (
	"fmt"

	"example.com/app/cache"
	"example.com/app/store"
	"example.com/app/util"
)

func main() { // @def main
	s := store.New()
	s.Save("a")
	c := cache.NewCache(4)
	c.Save("b")
	total := s.Len() + c.Len()
	util.Helper()
	fmt.Println(total, util.Checksum("z"))
	run(s)
}

func run(s *store.Store) { // @def run
	s.Save("again")
	defer cleanup()
}

func cleanup() {} // @def cleanup
