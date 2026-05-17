/* C EDGE CASES — comprehensive test for tricky patterns */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <stdbool.h>

/* === 1. Function pointers in structs (vtable pattern) === */
typedef struct vtable {
    void (*init)(void *self);
    void (*destroy)(void *self);
    int (*process)(void *self, const char *input);
    const char *(*to_string)(void *self);
} vtable_t;

/* === 2. Nested and anonymous structs/unions === */
typedef struct {
    union {
        struct {
            float x, y, z;
        };
        float v[3];
    };
    enum { POINT, VECTOR, NORMAL } kind;
} vec3_t;

/* === 3. Opaque type (forward declaration) === */
typedef struct context context_t;

/* Implementation hidden in .c file */
struct context {
    int fd;
    char *buffer;
    size_t buf_size;
    void (*on_error)(context_t *ctx, int code);
};

context_t *context_new(size_t buf_size) {
    context_t *ctx = calloc(1, sizeof(context_t));
    if (!ctx) return NULL;
    ctx->buffer = malloc(buf_size);
    if (!ctx->buffer) {
        free(ctx);
        return NULL;
    }
    ctx->buf_size = buf_size;
    ctx->fd = -1;
    return ctx;
}

void context_free(context_t *ctx) {
    if (!ctx) return;
    free(ctx->buffer);
    free(ctx);
}

/* === 4. Complex macros === */
#define ARRAY_SIZE(arr) (sizeof(arr) / sizeof((arr)[0]))

#define MIN(a, b) \
    ({ __typeof__(a) _a = (a); \
       __typeof__(b) _b = (b); \
       _a < _b ? _a : _b; })

#define MAX(a, b) \
    ({ __typeof__(a) _a = (a); \
       __typeof__(b) _b = (b); \
       _a > _b ? _a : _b; })

#define LOG(level, fmt, ...) \
    fprintf(stderr, "[%s] %s:%d: " fmt "\n", \
            level, __FILE__, __LINE__, ##__VA_ARGS__)

#define LOG_INFO(fmt, ...)  LOG("INFO", fmt, ##__VA_ARGS__)
#define LOG_ERROR(fmt, ...) LOG("ERROR", fmt, ##__VA_ARGS__)

/* === 5. Bitfield struct === */
typedef struct {
    unsigned int read    : 1;
    unsigned int write   : 1;
    unsigned int execute : 1;
    unsigned int sticky  : 1;
    unsigned int setuid  : 1;
    unsigned int setgid  : 1;
    unsigned int         : 2; /* padding */
    unsigned int owner   : 4;
    unsigned int group   : 4;
} permissions_t;

/* === 6. Enum with explicit values === */
typedef enum {
    ERR_NONE     = 0,
    ERR_NOMEM    = -1,
    ERR_IO       = -2,
    ERR_OVERFLOW = -3,
    ERR_INVALID  = -4,
    ERR_TIMEOUT  = -5,
    ERR_COUNT    = 6
} error_code_t;

/* === 7. Variadic function === */
int sum_ints(int count, ...) {
    va_list args;
    va_start(args, count);
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += va_arg(args, int);
    }
    va_end(args);
    return total;
}

/* === 8. Inline function === */
static inline int clamp(int value, int lo, int hi) {
    if (value < lo) return lo;
    if (value > hi) return hi;
    return value;
}

/* === 9. Function pointer typedef and callback === */
typedef int (*comparator_t)(const void *, const void *);
typedef void (*callback_t)(void *data, int status);

int int_compare(const void *a, const void *b) {
    return *(const int *)a - *(const int *)b;
}

void sort_ints(int *arr, size_t len) {
    qsort(arr, len, sizeof(int), int_compare);
}

/* === 10. Linked list with flexible array member === */
typedef struct node {
    struct node *next;
    size_t data_len;
    char data[];  /* flexible array member */
} node_t;

node_t *node_create(const char *data, size_t len) {
    node_t *n = malloc(sizeof(node_t) + len);
    if (!n) return NULL;
    n->next = NULL;
    n->data_len = len;
    memcpy(n->data, data, len);
    return n;
}

/* === 11. Static global + extern declaration === */
static int module_initialized = 0;
extern int shared_counter;

static void ensure_init(void) {
    if (!module_initialized) {
        module_initialized = 1;
        LOG_INFO("Module initialized");
    }
}

/* === 12. Union with tag (tagged union / discriminated union) === */
typedef enum {
    VAL_INT,
    VAL_FLOAT,
    VAL_STRING,
    VAL_BOOL,
} value_type_t;

typedef struct {
    value_type_t type;
    union {
        int i;
        double f;
        char *s;
        bool b;
    } as;
} value_t;

value_t value_int(int i) {
    return (value_t){ .type = VAL_INT, .as.i = i };
}

value_t value_float(double f) {
    return (value_t){ .type = VAL_FLOAT, .as.f = f };
}

value_t value_string(const char *s) {
    value_t v = { .type = VAL_STRING };
    v.as.s = strdup(s);
    return v;
}

void value_print(const value_t *v) {
    switch (v->type) {
        case VAL_INT:    printf("%d", v->as.i); break;
        case VAL_FLOAT:  printf("%g", v->as.f); break;
        case VAL_STRING: printf("%s", v->as.s); break;
        case VAL_BOOL:   printf("%s", v->as.b ? "true" : "false"); break;
    }
}

/* === 13. Struct with const and volatile members === */
typedef struct {
    const int id;
    volatile int reference_count;
    const char *name;
} resource_t;

/* === 14. Conditional compilation === */
#ifdef DEBUG
static void debug_print(const char *msg) {
    fprintf(stderr, "DEBUG: %s\n", msg);
}
#else
#define debug_print(msg) ((void)0)
#endif

/* === 15. Complex multi-level pointer === */
typedef struct {
    int **matrix;
    size_t rows;
    size_t cols;
} matrix_t;

matrix_t *matrix_create(size_t rows, size_t cols) {
    matrix_t *m = malloc(sizeof(matrix_t));
    if (!m) return NULL;
    m->rows = rows;
    m->cols = cols;
    m->matrix = calloc(rows, sizeof(int *));
    if (!m->matrix) { free(m); return NULL; }
    for (size_t i = 0; i < rows; i++) {
        m->matrix[i] = calloc(cols, sizeof(int));
        if (!m->matrix[i]) {
            for (size_t j = 0; j < i; j++) free(m->matrix[j]);
            free(m->matrix);
            free(m);
            return NULL;
        }
    }
    return m;
}

void matrix_free(matrix_t *m) {
    if (!m) return;
    for (size_t i = 0; i < m->rows; i++) {
        free(m->matrix[i]);
    }
    free(m->matrix);
    free(m);
}

/* === 16. Generic-like macro (type-safe container) === */
#define DECLARE_VECTOR(T, name) \
    typedef struct { \
        T *data; \
        size_t len; \
        size_t cap; \
    } name##_t; \
    \
    static inline name##_t name##_new(void) { \
        return (name##_t){ NULL, 0, 0 }; \
    } \
    \
    static inline int name##_push(name##_t *v, T item) { \
        if (v->len >= v->cap) { \
            size_t new_cap = v->cap ? v->cap * 2 : 8; \
            T *new_data = realloc(v->data, new_cap * sizeof(T)); \
            if (!new_data) return -1; \
            v->data = new_data; \
            v->cap = new_cap; \
        } \
        v->data[v->len++] = item; \
        return 0; \
    }

DECLARE_VECTOR(int, int_vec)
DECLARE_VECTOR(double, double_vec)
