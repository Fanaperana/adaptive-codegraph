package util

import "strings"

func Checksum(s string) int { // @def util::Checksum
	return len(strings.ToUpper(s))
}

func Helper() int { // @def util::Helper
	return Checksum("h")
}
