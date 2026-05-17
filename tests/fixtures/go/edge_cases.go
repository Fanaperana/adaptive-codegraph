package go
// GO EDGE CASES — comprehensive test for tricky patterns
package edgecase

import (
	"context"
	"fmt"
	"io"
	"sync"
	"time"
)

// === 1. Embedded structs and promoted methods ===
type Base struct {
	ID        int
	CreatedAt time.Time
}

func (b *Base) GetID() int {
	return b.ID
}

type Timestamped struct {
	Base
	UpdatedAt time.Time
}

type User struct {
	Timestamped
	Name  string
	Email string
}

// === 2. Interface embedding ===
type Reader interface {
	Read(p []byte) (n int, err error)
}

type Writer interface {
	Write(p []byte) (n int, err error)
}

type ReadWriter interface {
	Reader
	Writer
}

type Closer interface {
	Close() error
}

type ReadWriteCloser interface {
	ReadWriter
	Closer
}

// === 3. Struct with tags and unexported fields ===
type Config struct {
	Host     string `json:"host" yaml:"host" validate:"required"`
	Port     int    `json:"port" yaml:"port" validate:"min=1,max=65535"`
	Debug    bool   `json:"debug,omitempty" yaml:"debug"`
	password string // unexported
}

func NewConfig(host string, port int) *Config {
	return &Config{
		Host: host,
		Port: port,
	}
}

func (c *Config) setPassword(p string) {
	c.password = p
}

// === 4. Multiple return values and named returns ===
func divide(a, b float64) (result float64, err error) {
	if b == 0 {
		err = fmt.Errorf("division by zero")
		return
	}
	result = a / b
	return
}

// === 5. Variadic function ===
func sum(nums ...int) int {
	total := 0
	for _, n := range nums {
		total += n
	}
	return total
}

// === 6. Function types and closures ===
type Middleware func(next http_handler) http_handler
type http_handler func(ctx context.Context, req []byte) ([]byte, error)

func loggingMiddleware() Middleware {
	return func(next http_handler) http_handler {
		return func(ctx context.Context, req []byte) ([]byte, error) {
			fmt.Println("Request received")
			resp, err := next(ctx, req)
			fmt.Println("Response sent")
			return resp, err
		}
	}
}

// === 7. Iota constants with complex expressions ===
type ByteSize float64

const (
	_           = iota // ignore first
	KB ByteSize = 1 << (10 * iota)
	MB
	GB
	TB
	PB
)

const (
	StatusPending  = "pending"
	StatusActive   = "active"
	StatusInactive = "inactive"
)

// === 8. Generics (type parameters) ===
type Ordered interface {
	~int | ~float64 | ~string
}

func Max[T Ordered](a, b T) T {
	if a > b {
		return a
	}
	return b
}

type Stack[T any] struct {
	items []T
}

func (s *Stack[T]) Push(item T) {
	s.items = append(s.items, item)
}

func (s *Stack[T]) Pop() (T, bool) {
	var zero T
	if len(s.items) == 0 {
		return zero, false
	}
	item := s.items[len(s.items)-1]
	s.items = s.items[:len(s.items)-1]
	return item, true
}

func (s *Stack[T]) Len() int {
	return len(s.items)
}

// === 9. Channel-based patterns ===
func fanOut(input <-chan int, workers int) []<-chan int {
	outputs := make([]<-chan int, workers)
	for i := 0; i < workers; i++ {
		ch := make(chan int)
		outputs[i] = ch
		go func() {
			defer close(ch)
			for v := range input {
				ch <- v * v
			}
		}()
	}
	return outputs
}

func fanIn(channels ...<-chan int) <-chan int {
	var wg sync.WaitGroup
	merged := make(chan int)

	for _, ch := range channels {
		wg.Add(1)
		go func(c <-chan int) {
			defer wg.Done()
			for v := range c {
				merged <- v
			}
		}(ch)
	}

	go func() {
		wg.Wait()
		close(merged)
	}()

	return merged
}

// === 10. Type assertion and type switch ===
func describe(i interface{}) string {
	switch v := i.(type) {
	case int:
		return fmt.Sprintf("integer: %d", v)
	case string:
		return fmt.Sprintf("string: %q", v)
	case bool:
		return fmt.Sprintf("boolean: %t", v)
	case error:
		return fmt.Sprintf("error: %v", v)
	default:
		return fmt.Sprintf("unknown: %T", v)
	}
}

// === 11. Custom error type ===
type AppError struct {
	Code    int
	Message string
	Cause   error
}

func (e *AppError) Error() string {
	if e.Cause != nil {
		return fmt.Sprintf("[%d] %s: %v", e.Code, e.Message, e.Cause)
	}
	return fmt.Sprintf("[%d] %s", e.Code, e.Message)
}

func (e *AppError) Unwrap() error {
	return e.Cause
}

// === 12. Sync primitives ===
type SafeCounter struct {
	mu sync.RWMutex
	v  map[string]int
}

func NewSafeCounter() *SafeCounter {
	return &SafeCounter{v: make(map[string]int)}
}

func (c *SafeCounter) Inc(key string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.v[key]++
}

func (c *SafeCounter) Get(key string) int {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.v[key]
}

// === 13. Interface satisfaction check (compile-time) ===
var _ io.ReadWriteCloser = (*mockRWC)(nil)

type mockRWC struct{}

func (m *mockRWC) Read(p []byte) (int, error)  { return 0, io.EOF }
func (m *mockRWC) Write(p []byte) (int, error) { return len(p), nil }
func (m *mockRWC) Close() error                { return nil }

// === 14. Init function ===
var defaultTimeout time.Duration

func init() {
	defaultTimeout = 30 * time.Second
}

// === 15. Blank identifier patterns ===
var (
	_ = fmt.Sprintf // reference to keep import
	_ Reader = (*mockRWC)(nil)
)

// === 16. Global vars with complex init ===
var (
	ErrNotFound   = fmt.Errorf("not found")
	ErrForbidden  = fmt.Errorf("forbidden")
	ErrBadRequest = fmt.Errorf("bad request")
)
