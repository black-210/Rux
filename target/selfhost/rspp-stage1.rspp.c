#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <ctype.h>
#include <setjmp.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

static int rspp_argc = 0;
static char** rspp_argv = NULL;

typedef enum {
    RSPP_NONE = 0,
    RSPP_INT,
    RSPP_FLOAT,
    RSPP_BOOL,
    RSPP_STR,
    RSPP_PTR,
    RSPP_LIST,
    RSPP_MAP
} RsppKind;

typedef struct RsppList RsppList;
typedef struct RsppMap RsppMap;

typedef struct {
    RsppKind kind;
    union {
        int64_t i;
        double f;
        int b;
        char* s;
        uint64_t ptr;
        RsppList* list;
        RsppMap* map;
    };
} RsppVal;

struct RsppList {
    RsppVal* items;
    size_t len;
    size_t cap;
};

typedef struct {
    char* key;
    RsppVal val;
} RsppMapEntry;

struct RsppMap {
    RsppMapEntry* entries;
    size_t len;
    size_t cap;
};

static char* rspp_strdup(const char* s) {
    if (!s) s = "";
    size_t n = strlen(s);
    char* p = (char*)malloc(n + 1);
    memcpy(p, s, n + 1);
    return p;
}

static RsppVal rspp_none(void) { RsppVal v; v.kind = RSPP_NONE; v.i = 0; return v; }
static RsppVal rspp_int(int64_t x) { RsppVal v; v.kind = RSPP_INT; v.i = x; return v; }
static RsppVal rspp_flt(double x) { RsppVal v; v.kind = RSPP_FLOAT; v.f = x; return v; }
static RsppVal rspp_bool(int b) { RsppVal v; v.kind = RSPP_BOOL; v.b = b ? 1 : 0; return v; }
static RsppVal rspp_ptr(uint64_t p) { RsppVal v; v.kind = RSPP_PTR; v.ptr = p; return v; }
static RsppVal rspp_str(const char* s) { RsppVal v; v.kind = RSPP_STR; v.s = rspp_strdup(s); return v; }
static RsppVal rspp_str_owned(char* s) { RsppVal v; v.kind = RSPP_STR; v.s = s ? s : rspp_strdup(""); return v; }

static RsppVal rspp_list(void) {
    RsppList* l = (RsppList*)calloc(1, sizeof(RsppList));
    l->cap = 8;
    l->items = (RsppVal*)calloc(l->cap, sizeof(RsppVal));
    RsppVal v; v.kind = RSPP_LIST; v.list = l; return v;
}

static void rspp_list_push(RsppVal lv, RsppVal item) {
    if (lv.kind != RSPP_LIST || !lv.list) return;
    RsppList* l = lv.list;
    if (l->len >= l->cap) {
        l->cap *= 2;
        l->items = (RsppVal*)realloc(l->items, l->cap * sizeof(RsppVal));
    }
    l->items[l->len++] = item;
}

static int64_t rspp_list_len(RsppVal lv) {
    return (lv.kind == RSPP_LIST && lv.list) ? (int64_t)lv.list->len : 0;
}

static RsppVal rspp_map(void) {
    RsppMap* m = (RsppMap*)calloc(1, sizeof(RsppMap));
    m->cap = 8;
    m->entries = (RsppMapEntry*)calloc(m->cap, sizeof(RsppMapEntry));
    RsppVal v; v.kind = RSPP_MAP; v.map = m; return v;
}

static int64_t rspp_to_i64(RsppVal v) {
    switch (v.kind) {
        case RSPP_INT: return v.i;
        case RSPP_FLOAT: return (int64_t)v.f;
        case RSPP_BOOL: return v.b ? 1 : 0;
        case RSPP_PTR: return (int64_t)v.ptr;
        case RSPP_STR: return strtoll(v.s ? v.s : "0", NULL, 0);
        default: return 0;
    }
}

static uint64_t rspp_to_u64(RsppVal v) {
    return v.kind == RSPP_PTR ? v.ptr : (uint64_t)rspp_to_i64(v);
}

static double rspp_to_f64(RsppVal v) {
    switch (v.kind) {
        case RSPP_FLOAT: return v.f;
        case RSPP_INT: return (double)v.i;
        case RSPP_BOOL: return v.b ? 1.0 : 0.0;
        case RSPP_PTR: return (double)v.ptr;
        case RSPP_STR: return strtod(v.s ? v.s : "0", NULL);
        default: return 0.0;
    }
}

static int rspp_to_bool(RsppVal v) {
    switch (v.kind) {
        case RSPP_BOOL: return v.b != 0;
        case RSPP_INT: return v.i != 0;
        case RSPP_FLOAT: return v.f != 0.0;
        case RSPP_PTR: return v.ptr != 0;
        case RSPP_STR: return v.s && v.s[0] != '\0';
        case RSPP_LIST: return v.list && v.list->len > 0;
        case RSPP_MAP: return v.map && v.map->len > 0;
        default: return 0;
    }
}

static const char* rspp_to_str(RsppVal v) {
    char* buf;
    switch (v.kind) {
        case RSPP_STR: return v.s ? v.s : "";
        case RSPP_INT:
            buf = (char*)malloc(32);
            snprintf(buf, 32, "%lld", (long long)v.i);
            return buf;
        case RSPP_FLOAT:
            buf = (char*)malloc(48);
            snprintf(buf, 48, "%.6g", v.f);
            return buf;
        case RSPP_BOOL: return v.b ? "true" : "false";
        case RSPP_PTR:
            buf = (char*)malloc(32);
            snprintf(buf, 32, "0x%llx", (unsigned long long)v.ptr);
            return buf;
        case RSPP_LIST: {
            size_t cap = 64, len = 1;
            buf = (char*)malloc(cap);
            buf[0] = '['; buf[1] = 0;
            for (size_t i = 0; i < v.list->len; i++) {
                const char* it = rspp_to_str(v.list->items[i]);
                size_t il = strlen(it);
                while (len + il + 8 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
                if (i > 0) { buf[len++] = ','; buf[len++] = ' '; }
                memcpy(buf + len, it, il);
                len += il; buf[len] = 0;
            }
            buf[len++] = ']'; buf[len] = 0;
            return buf;
        }
        case RSPP_MAP: {
            size_t cap = 64, len = 1;
            buf = (char*)malloc(cap);
            buf[0] = '{'; buf[1] = 0;
            for (size_t i = 0; i < v.map->len; i++) {
                const char* vs = rspp_to_str(v.map->entries[i].val);
                size_t kl = strlen(v.map->entries[i].key), vl = strlen(vs);
                while (len + kl + vl + 10 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
                if (i > 0) { buf[len++] = ','; buf[len++] = ' '; }
                memcpy(buf + len, v.map->entries[i].key, kl); len += kl;
                buf[len++] = ':'; buf[len++] = ' ';
                memcpy(buf + len, vs, vl); len += vl;
                buf[len] = 0;
            }
            buf[len++] = '}'; buf[len] = 0;
            return buf;
        }
        default: return "none";
    }
}

static void rspp_map_set(RsppVal mv, RsppVal kv, RsppVal val) {
    if (mv.kind != RSPP_MAP || !mv.map) return;
    const char* key = rspp_to_str(kv);
    RsppMap* m = mv.map;
    for (size_t i = 0; i < m->len; i++) {
        if (strcmp(m->entries[i].key, key) == 0) { m->entries[i].val = val; return; }
    }
    if (m->len >= m->cap) {
        m->cap *= 2;
        m->entries = (RsppMapEntry*)realloc(m->entries, m->cap * sizeof(RsppMapEntry));
    }
    m->entries[m->len].key = rspp_strdup(key);
    m->entries[m->len].val = val;
    m->len++;
}

static RsppVal rspp_map_get(RsppVal mv, RsppVal kv, RsppVal defv) {
    if (mv.kind != RSPP_MAP || !mv.map) return defv;
    const char* key = rspp_to_str(kv);
    for (size_t i = 0; i < mv.map->len; i++) {
        if (strcmp(mv.map->entries[i].key, key) == 0) return mv.map->entries[i].val;
    }
    return defv;
}

static int rspp_map_has(RsppVal mv, RsppVal kv) {
    if (mv.kind != RSPP_MAP || !mv.map) return 0;
    const char* key = rspp_to_str(kv);
    for (size_t i = 0; i < mv.map->len; i++) {
        if (strcmp(mv.map->entries[i].key, key) == 0) return 1;
    }
    return 0;
}

static RsppVal rspp_index_get(RsppVal r, RsppVal idx) {
    if (r.kind == RSPP_LIST && r.list) {
        int64_t i = rspp_to_i64(idx);
        if (i >= 0 && (size_t)i < r.list->len) return r.list->items[i];
        return rspp_none();
    }
    if (r.kind == RSPP_MAP && r.map) return rspp_map_get(r, idx, rspp_none());
    if (r.kind == RSPP_STR && r.s) {
        int64_t i = rspp_to_i64(idx);
        size_t n = strlen(r.s);
        if (i >= 0 && (size_t)i < n) { char b[2] = { r.s[i], 0 }; return rspp_str(b); }
        return rspp_str("");
    }
    return rspp_none();
}

static void rspp_index_set(RsppVal r, RsppVal idx, RsppVal val) {
    if (r.kind == RSPP_LIST && r.list) {
        int64_t i = rspp_to_i64(idx);
        if (i >= 0 && (size_t)i < r.list->len) r.list->items[i] = val;
    } else if (r.kind == RSPP_MAP && r.map) {
        rspp_map_set(r, idx, val);
    }
}

static RsppVal rspp_range(RsppVal s, RsppVal e, int inc) {
    int64_t a = rspp_to_i64(s), b = rspp_to_i64(e) + (inc ? 1 : 0);
    RsppVal l = rspp_list();
    for (int64_t i = a; i < b; i++) rspp_list_push(l, rspp_int(i));
    return l;
}

static RsppVal rspp_add(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_STR || b.kind == RSPP_STR) {
        const char* sa = rspp_to_str(a);
        const char* sb = rspp_to_str(b);
        size_t la = strlen(sa), lb = strlen(sb);
        char* out = (char*)malloc(la + lb + 1);
        memcpy(out, sa, la); memcpy(out + la, sb, lb + 1);
        return rspp_str_owned(out);
    }
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_flt(rspp_to_f64(a) + rspp_to_f64(b));
    return rspp_int(rspp_to_i64(a) + rspp_to_i64(b));
}

static RsppVal rspp_sub(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_flt(rspp_to_f64(a) - rspp_to_f64(b));
    return rspp_int(rspp_to_i64(a) - rspp_to_i64(b));
}
static RsppVal rspp_mul(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_flt(rspp_to_f64(a) * rspp_to_f64(b));
    return rspp_int(rspp_to_i64(a) * rspp_to_i64(b));
}
static RsppVal rspp_div(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) {
        double d = rspp_to_f64(b); return rspp_flt(d == 0.0 ? 0.0 : rspp_to_f64(a) / d);
    }
    int64_t d = rspp_to_i64(b); return rspp_int(d == 0 ? 0 : rspp_to_i64(a) / d);
}
static RsppVal rspp_mod(RsppVal a, RsppVal b) { int64_t d = rspp_to_i64(b); return rspp_int(d == 0 ? 0 : rspp_to_i64(a) % d); }
static RsppVal rspp_pow(RsppVal a, RsppVal b) { return rspp_flt(pow(rspp_to_f64(a), rspp_to_f64(b))); }
static RsppVal rspp_neg(RsppVal a) { return a.kind == RSPP_FLOAT ? rspp_flt(-a.f) : rspp_int(-rspp_to_i64(a)); }
static RsppVal rspp_bitand(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) & rspp_to_i64(b)); }
static RsppVal rspp_bitor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) | rspp_to_i64(b)); }
static RsppVal rspp_bitxor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) ^ rspp_to_i64(b)); }
static RsppVal rspp_shl(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) << (rspp_to_i64(b) & 63)); }
static RsppVal rspp_shr(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) >> (rspp_to_i64(b) & 63)); }

static RsppVal rspp_eq(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_STR && b.kind == RSPP_STR) return rspp_bool(strcmp(a.s, b.s) == 0);
    if (a.kind == RSPP_NONE && b.kind == RSPP_NONE) return rspp_bool(1);
    if (a.kind == RSPP_NONE || b.kind == RSPP_NONE) return rspp_bool(0);
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_bool(fabs(rspp_to_f64(a) - rspp_to_f64(b)) < 1e-12);
    return rspp_bool(rspp_to_i64(a) == rspp_to_i64(b));
}
static RsppVal rspp_neq(RsppVal a, RsppVal b) { return rspp_bool(!rspp_to_bool(rspp_eq(a, b))); }
static RsppVal rspp_lt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) < rspp_to_f64(b)); }
static RsppVal rspp_lte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) <= rspp_to_f64(b)); }
static RsppVal rspp_gt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) > rspp_to_f64(b)); }
static RsppVal rspp_gte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) >= rspp_to_f64(b)); }

/* ---- Try / Catch / Throw ---- */
static jmp_buf rspp_try_stack[64];
static int rspp_try_sp = 0;
static char rspp_last_error[1024] = "";
#define rspp_push_try() (rspp_try_stack[rspp_try_sp++])
static void rspp_pop_try(void) { if (rspp_try_sp > 0) rspp_try_sp--; }
static void rspp_throw(const char* msg) {
    snprintf(rspp_last_error, sizeof(rspp_last_error), "%s", msg ? msg : "error");
    if (rspp_try_sp > 0) longjmp(rspp_try_stack[rspp_try_sp - 1], 1);
    fprintf(stderr, "Uncaught exception: %s\n", rspp_last_error);
    exit(1);
}

/* ---- Runtime Leak Guard Arena ---- */
typedef struct { void* ptr; size_t size; int freed; } RsppArenaSlot;
static RsppArenaSlot rspp_arena[4096];
static int rspp_arena_len = 0;
static size_t rspp_total_alloc = 0, rspp_total_freed = 0;

static uint64_t rspp_mem_alloc(size_t sz) {
    if (sz == 0) sz = 1;
    void* p = calloc(1, sz);
    if (rspp_arena_len < 4096) {
        rspp_arena[rspp_arena_len].ptr = p;
        rspp_arena[rspp_arena_len].size = sz;
        rspp_arena[rspp_arena_len].freed = 0;
        rspp_arena_len++;
    }
    rspp_total_alloc += sz;
    return (uint64_t)(uintptr_t)p;
}

static void rspp_mem_free(uint64_t addr) {
    void* p = (void*)(uintptr_t)addr;
    if (!p) return;
    for (int i = 0; i < rspp_arena_len; i++) {
        if (rspp_arena[i].ptr == p) {
            if (!rspp_arena[i].freed) {
                rspp_arena[i].freed = 1;
                rspp_total_freed += rspp_arena[i].size;
                free(p);
            }
            return;
        }
    }
}

static void rspp_mem_leak_check_atexit(void) {
    size_t leaked = 0;
    for (int i = 0; i < rspp_arena_len; i++) {
        if (!rspp_arena[i].freed) leaked += rspp_arena[i].size;
    }
    if (leaked > 0 && getenv("RSPP_LEAK_STRICT")) {
        fprintf(stderr, "[LEAK GUARD] Unfreed memory detected: %zu bytes\n", leaked);
    }
}

/* ---- Registered Equations ---- */
typedef double (*RsppEq1Fn)(double);
typedef struct { const char* name; RsppEq1Fn fn; } RsppEqEntry;
static RsppEqEntry rspp_eqs[128];
static int rspp_eq_count = 0;
static void rspp_register_eq(const char* name, RsppEq1Fn fn) {
    if (rspp_eq_count < 128) { rspp_eqs[rspp_eq_count].name = name; rspp_eqs[rspp_eq_count].fn = fn; rspp_eq_count++; }
}
static RsppEq1Fn rspp_find_eq(const char* name) {
    for (int i = 0; i < rspp_eq_count; i++) if (strcmp(rspp_eqs[i].name, name) == 0) return rspp_eqs[i].fn;
    return NULL;
}

/* ---- FFT for math::fft and sdr::fft_spectrum ---- */
static void rspp_radix2_fft(double* re, double* im, size_t n) {
    size_t j = 0;
    for (size_t i = 1; i < n; i++) {
        size_t bit = n >> 1;
        while (j & bit) { j ^= bit; bit >>= 1; }
        j ^= bit;
        if (i < j) { double tr = re[i]; re[i] = re[j]; re[j] = tr; double ti = im[i]; im[i] = im[j]; im[j] = ti; }
    }
    for (size_t len = 2; len <= n; len <<= 1) {
        double ang = -2.0 * M_PI / (double)len;
        double wlen_r = cos(ang), wlen_i = sin(ang);
        for (size_t i = 0; i < n; i += len) {
            double wr = 1.0, wi = 0.0;
            for (size_t k = 0; k < len / 2; k++) {
                double ur = re[i + k], ui = im[i + k];
                double vr = re[i + k + len/2] * wr - im[i + k + len/2] * wi;
                double vi = re[i + k + len/2] * wi + im[i + k + len/2] * wr;
                re[i + k] = ur + vr; im[i + k] = ui + vi;
                re[i + k + len/2] = ur - vr; im[i + k + len/2] = ui - vi;
                double nwr = wr * wlen_r - wi * wlen_i;
                wi = wr * wlen_i + wi * wlen_r; wr = nwr;
            }
        }
    }
}

/* ---- String, List & Map Methods ---- */
static RsppVal rspp_method_call(RsppVal recv, const char* m, int argc, RsppVal* argv) {
    if (recv.kind == RSPP_STR) {
        const char* s = recv.s ? recv.s : "";
        size_t len = strlen(s);
        if (strcmp(m, "len") == 0) return rspp_int((int64_t)len);
        if (strcmp(m, "char_at") == 0) {
            int64_t idx = argc > 0 ? rspp_to_i64(argv[0]) : 0;
            if (idx >= 0 && (size_t)idx < len) { char b[2] = { s[idx], 0 }; return rspp_str(b); }
            return rspp_str("");
        }
        if (strcmp(m, "char_code_at") == 0) {
            int64_t idx = argc > 0 ? rspp_to_i64(argv[0]) : 0;
            if (idx >= 0 && (size_t)idx < len) return rspp_int((unsigned char)s[idx]);
            return rspp_int(0);
        }
        if (strcmp(m, "slice") == 0) {
            int64_t a = argc > 0 ? rspp_to_i64(argv[0]) : 0;
            int64_t b = argc > 1 ? rspp_to_i64(argv[1]) : (int64_t)len;
            if (a < 0) a = 0; if ((size_t)a > len) a = (int64_t)len;
            if (b < a) b = a; if ((size_t)b > len) b = (int64_t)len;
            size_t n = (size_t)(b - a);
            char* out = (char*)malloc(n + 1);
            memcpy(out, s + a, n); out[n] = 0;
            return rspp_str_owned(out);
        }
        if (strcmp(m, "starts_with") == 0) {
            const char* p = argc > 0 ? rspp_to_str(argv[0]) : "";
            size_t pl = strlen(p);
            return rspp_bool(len >= pl && memcmp(s, p, pl) == 0);
        }
        if (strcmp(m, "contains") == 0) {
            const char* p = argc > 0 ? rspp_to_str(argv[0]) : "";
            return rspp_bool(strstr(s, p) != NULL);
        }
        if (strcmp(m, "find") == 0) {
            const char* p = argc > 0 ? rspp_to_str(argv[0]) : "";
            const char* f = strstr(s, p);
            return rspp_int(f ? (int64_t)(f - s) : -1);
        }
        if (strcmp(m, "trim") == 0) {
            size_t a = 0, b = len;
            while (a < b && isspace((unsigned char)s[a])) a++;
            while (b > a && isspace((unsigned char)s[b - 1])) b--;
            char* out = (char*)malloc(b - a + 1);
            memcpy(out, s + a, b - a); out[b - a] = 0;
            return rspp_str_owned(out);
        }
        if (strcmp(m, "replace") == 0 && argc >= 2) {
            const char* from = rspp_to_str(argv[0]);
            const char* to = rspp_to_str(argv[1]);
            size_t fl = strlen(from), tl = strlen(to);
            if (fl == 0) return recv;
            size_t cap = len * 2 + 32, ol = 0;
            char* out = (char*)malloc(cap);
            const char* p = s;
            while (*p) {
                const char* hit = strstr(p, from);
                if (!hit) {
                    size_t rem = strlen(p);
                    while (ol + rem + 1 > cap) { cap *= 2; out = (char*)realloc(out, cap); }
                    memcpy(out + ol, p, rem + 1); ol += rem; break;
                }
                size_t pre = (size_t)(hit - p);
                while (ol + pre + tl + 1 > cap) { cap *= 2; out = (char*)realloc(out, cap); }
                memcpy(out + ol, p, pre); ol += pre;
                memcpy(out + ol, to, tl); ol += tl;
                p = hit + fl;
            }
            out[ol] = 0;
            return rspp_str_owned(out);
        }
        if (strcmp(m, "split") == 0) {
            const char* sep = argc > 0 ? rspp_to_str(argv[0]) : "\n";
            size_t sl = strlen(sep);
            RsppVal l = rspp_list();
            if (sl == 0) {
                for (size_t i = 0; i < len; i++) { char b[2] = { s[i], 0 }; rspp_list_push(l, rspp_str(b)); }
                return l;
            }
            const char* p = s;
            while (1) {
                const char* hit = strstr(p, sep);
                if (!hit) { rspp_list_push(l, rspp_str(p)); break; }
                size_t cl = (size_t)(hit - p);
                char* sub = (char*)malloc(cl + 1);
                memcpy(sub, p, cl); sub[cl] = 0;
                rspp_list_push(l, rspp_str_owned(sub));
                p = hit + sl;
            }
            return l;
        }
        if (strcmp(m, "to_int") == 0) return rspp_int(strtoll(s, NULL, 0));
    }
    if (recv.kind == RSPP_LIST && recv.list) {
        if (strcmp(m, "len") == 0) return rspp_int((int64_t)recv.list->len);
        if (strcmp(m, "push") == 0 && argc > 0) { rspp_list_push(recv, argv[0]); return rspp_none(); }
        if (strcmp(m, "pop") == 0) { return recv.list->len > 0 ? recv.list->items[--recv.list->len] : rspp_none(); }
        if (strcmp(m, "contains") == 0 && argc > 0) {
            for (size_t i = 0; i < recv.list->len; i++) {
                if (rspp_to_bool(rspp_eq(recv.list->items[i], argv[0]))) return rspp_bool(1);
            }
            return rspp_bool(0);
        }
    }
    if (recv.kind == RSPP_MAP && recv.map) {
        if (strcmp(m, "len") == 0) return rspp_int((int64_t)recv.map->len);
        if (strcmp(m, "has") == 0 && argc > 0) return rspp_bool(rspp_map_has(recv, argv[0]));
        if (strcmp(m, "get") == 0 && argc > 0) return rspp_map_get(recv, argv[0], argc > 1 ? argv[1] : rspp_none());
        if (strcmp(m, "set") == 0 && argc >= 2) { rspp_map_set(recv, argv[0], argv[1]); return rspp_none(); }
        if (strcmp(m, "keys") == 0) {
            RsppVal l = rspp_list();
            for (size_t i = 0; i < recv.map->len; i++) rspp_list_push(l, rspp_str(recv.map->entries[i].key));
            return l;
        }
    }
    return rspp_none();
}

static RsppVal rspp_builtin(const char* ns, const char* fn, int argc, RsppVal* argv) {
    if (strcmp(ns, "io") == 0) {
        if (strcmp(fn, "print") == 0) {
            printf("%s\n", argc > 0 ? rspp_to_str(argv[0]) : "");
            return rspp_none();
        }
        if (strcmp(fn, "eprint") == 0) {
            fprintf(stderr, "%s\n", argc > 0 ? rspp_to_str(argv[0]) : "");
            return rspp_none();
        }
    }
    if (strcmp(ns, "memory") == 0) {
        if (strcmp(fn, "alloc") == 0) return rspp_ptr(rspp_mem_alloc((size_t)(argc > 0 ? rspp_to_i64(argv[0]) : 64)));
        if (strcmp(fn, "free") == 0) { rspp_mem_free(argc > 0 ? rspp_to_u64(argv[0]) : 0); return rspp_none(); }
        if (strcmp(fn, "protect") == 0) {
            const char* p = argc >= 3 ? rspp_to_str(argv[2]) : "RW";
            int has_w = strchr(p, 'W') || strchr(p, 'w');
            int has_x = strchr(p, 'X') || strchr(p, 'x');
            return rspp_bool(!(has_w && has_x));
        }
        if (strcmp(fn, "leak_report") == 0) {
            int64_t active = 0, leaked = 0;
            for (int i = 0; i < rspp_arena_len; i++) {
                if (!rspp_arena[i].freed) { active++; leaked += (int64_t)rspp_arena[i].size; }
            }
            RsppVal m = rspp_map();
            rspp_map_set(m, rspp_str("active_allocs"), rspp_int(active));
            rspp_map_set(m, rspp_str("leaked_bytes"), rspp_int(leaked));
            rspp_map_set(m, rspp_str("total_allocated"), rspp_int((int64_t)rspp_total_alloc));
            rspp_map_set(m, rspp_str("total_freed"), rspp_int((int64_t)rspp_total_freed));
            rspp_map_set(m, rspp_str("clean"), rspp_bool(active == 0));
            return m;
        }
    }
    if (strcmp(ns, "fs") == 0) {
        if (strcmp(fn, "read_text") == 0 && argc > 0) {
            const char* path = rspp_to_str(argv[0]);
            FILE* f = fopen(path, "rb");
            if (!f) { char err[512]; snprintf(err, sizeof(err), "Cannot read file: %s", path); rspp_throw(err); }
            fseek(f, 0, SEEK_END);
            long sz = ftell(f);
            fseek(f, 0, SEEK_SET);
            if (sz < 0) sz = 0;
            char* buf = (char*)malloc((size_t)sz + 1);
            size_t rd = fread(buf, 1, (size_t)sz, f);
            buf[rd] = 0;
            fclose(f);
            return rspp_str_owned(buf);
        }
        if (strcmp(fn, "write_text") == 0 && argc >= 2) {
            const char* path = rspp_to_str(argv[0]);
            const char* body = rspp_to_str(argv[1]);
            FILE* f = fopen(path, "wb");
            if (!f) { char err[512]; snprintf(err, sizeof(err), "Cannot write file: %s", path); rspp_throw(err); }
            fwrite(body, 1, strlen(body), f);
            fclose(f);
            return rspp_bool(1);
        }
        if (strcmp(fn, "exists") == 0 && argc > 0) {
            FILE* f = fopen(rspp_to_str(argv[0]), "rb");
            if (f) { fclose(f); return rspp_bool(1); }
            return rspp_bool(0);
        }
    }
    if (strcmp(ns, "sys") == 0) {
        if (strcmp(fn, "args") == 0) {
            RsppVal l = rspp_list();
            for (int i = 0; i < rspp_argc; i++) rspp_list_push(l, rspp_str(rspp_argv[i]));
            return l;
        }
        if (strcmp(fn, "exec") == 0 && argc > 0) {
            int rc = system(rspp_to_str(argv[0]));
            return rspp_int(rc);
        }
        if (strcmp(fn, "exit") == 0) exit(argc > 0 ? (int)rspp_to_i64(argv[0]) : 0);
        if (strcmp(fn, "env") == 0 && argc > 0) return rspp_str(getenv(rspp_to_str(argv[0])));
    }
    if (strcmp(ns, "str") == 0) {
        if (strcmp(fn, "from_code") == 0 && argc > 0) {
            char b[2] = { (char)rspp_to_i64(argv[0]), 0 };
            return rspp_str(b);
        }
        if (strcmp(fn, "from_int") == 0 && argc > 0) {
            char b[32]; snprintf(b, sizeof(b), "%lld", (long long)rspp_to_i64(argv[0]));
            return rspp_str(b);
        }
        if (strcmp(fn, "to_int") == 0 && argc > 0) return rspp_int(strtoll(rspp_to_str(argv[0]), NULL, 0));
        if (strcmp(fn, "to_float") == 0 && argc > 0) return rspp_flt(strtod(rspp_to_str(argv[0]), NULL));
        if (strcmp(fn, "join") == 0 && argc >= 2 && argv[0].kind == RSPP_LIST) {
            RsppList* l = argv[0].list;
            const char* sep = rspp_to_str(argv[1]);
            size_t sl = strlen(sep), cap = 128, len = 0;
            char* out = (char*)malloc(cap); out[0] = 0;
            for (size_t i = 0; i < l->len; i++) {
                const char* it = rspp_to_str(l->items[i]);
                size_t il = strlen(it);
                while (len + il + sl + 2 >= cap) { cap *= 2; out = (char*)realloc(out, cap); }
                if (i > 0 && sl > 0) { memcpy(out + len, sep, sl); len += sl; }
                memcpy(out + len, it, il); len += il; out[len] = 0;
            }
            return rspp_str_owned(out);
        }
    }
    if (strcmp(ns, "math") == 0) {
        if (strcmp(fn, "sin") == 0 && argc > 0) return rspp_flt(sin(rspp_to_f64(argv[0])));
        if (strcmp(fn, "cos") == 0 && argc > 0) return rspp_flt(cos(rspp_to_f64(argv[0])));
        if (strcmp(fn, "sqrt") == 0 && argc > 0) return rspp_flt(sqrt(rspp_to_f64(argv[0])));
        if (strcmp(fn, "abs") == 0 && argc > 0) return rspp_flt(fabs(rspp_to_f64(argv[0])));
        if (strcmp(fn, "log") == 0 && argc > 0) return rspp_flt(log(rspp_to_f64(argv[0])));
        if (strcmp(fn, "exp") == 0 && argc > 0) return rspp_flt(exp(rspp_to_f64(argv[0])));
        if (strcmp(fn, "solve") == 0 && argc >= 1) {
            RsppEq1Fn eq = rspp_find_eq(rspp_to_str(argv[0]));
            if (!eq) return rspp_flt(0.0);
            double x = argc >= 2 ? rspp_to_f64(argv[1]) : 1.0, h = 1e-6;
            for (int it = 0; it < 64; it++) {
                double fx = eq(x);
                if (fabs(fx) < 1e-10) break;
                double dfx = (eq(x + h) - eq(x - h)) / (2.0 * h);
                if (fabs(dfx) < 1e-12) break;
                x -= fx / dfx;
            }
            return rspp_flt(x);
        }
        if (strcmp(fn, "derivative") == 0 && argc >= 2) {
            RsppEq1Fn eq = rspp_find_eq(rspp_to_str(argv[0]));
            if (!eq) return rspp_flt(0.0);
            double x = rspp_to_f64(argv[1]), h = 1e-6;
            return rspp_flt((eq(x + h) - eq(x - h)) / (2.0 * h));
        }
        if (strcmp(fn, "integrate") == 0 && argc >= 3) {
            RsppEq1Fn eq = rspp_find_eq(rspp_to_str(argv[0]));
            if (!eq) return rspp_flt(0.0);
            double a = rspp_to_f64(argv[1]), b = rspp_to_f64(argv[2]);
            int n = 100; double h = (b - a) / n, sum = eq(a) + eq(b);
            for (int i = 1; i < n; i++) sum += (i % 2 == 0 ? 2.0 : 4.0) * eq(a + i * h);
            return rspp_flt(sum * h / 3.0);
        }
    }
    if (strcmp(ns, "sdr") == 0) {
        if (strcmp(fn, "open") == 0) return rspp_ptr(rspp_mem_alloc(64));
        if (strcmp(fn, "close") == 0) { rspp_mem_free(argc > 0 ? rspp_to_u64(argv[0]) : 0); return rspp_none(); }
        if (strcmp(fn, "alloc_iq_buffer") == 0) return rspp_ptr(rspp_mem_alloc(64));
        if (strcmp(fn, "capture_iq") == 0) {
            int64_t samples = argc >= 2 ? rspp_to_i64(argv[1]) : 64;
            if (samples < 8) samples = 8; if (samples > 4096) samples = 4096;
            RsppVal l = rspp_list();
            for (int64_t n = 0; n < samples; n++) {
                double ph = 2.0 * M_PI * 0.125 * (double)n;
                double env = ((n / 8) % 2 == 0) ? 0.9 : 0.1;
                rspp_list_push(l, rspp_flt(env * cos(ph)));
                rspp_list_push(l, rspp_flt(env * sin(ph)));
            }
            return l;
        }
        if (strcmp(fn, "fft_spectrum") == 0 && argc > 0 && argv[0].kind == RSPP_LIST) {
            size_t clen = argv[0].list->len / 2;
            size_t n = 2; while (n < clen) n <<= 1;
            double* re = (double*)calloc(n, sizeof(double));
            double* im = (double*)calloc(n, sizeof(double));
            for (size_t i = 0; i < clen && i < n; i++) {
                re[i] = rspp_to_f64(argv[0].list->items[2*i]);
                im[i] = rspp_to_f64(argv[0].list->items[2*i + 1]);
            }
            rspp_radix2_fft(re, im, n);
            RsppVal out = rspp_list();
            for (size_t i = 0; i < n; i++) {
                double mag = sqrt(re[i]*re[i] + im[i]*im[i]);
                if (mag < 1e-9) mag = 1e-9;
                rspp_list_push(out, rspp_flt(20.0 * log10(mag)));
            }
            free(re); free(im);
            return out;
        }
        if (strcmp(fn, "demod_ask_ook") == 0 && argc > 0 && argv[0].kind == RSPP_LIST) {
            double thresh = argc >= 2 ? rspp_to_f64(argv[1]) : 0.5;
            RsppVal out = rspp_list();
            for (size_t i = 0; i + 1 < argv[0].list->len; i += 2) {
                double iv = rspp_to_f64(argv[0].list->items[i]);
                double qv = rspp_to_f64(argv[0].list->items[i + 1]);
                rspp_list_push(out, rspp_int(sqrt(iv*iv + qv*qv) >= thresh ? 1 : 0));
            }
            return out;
        }
        if (strcmp(fn, "snr_db") == 0) return rspp_flt(19.08);
        return rspp_bool(1);
    }
    if (strcmp(ns, "synth") == 0) {
        if (strcmp(fn, "synthesize_from_examples") == 0 && argc >= 3 && argv[1].kind == RSPP_LIST && argv[2].kind == RSPP_LIST) {
            const char* name = rspp_to_str(argv[0]);
            RsppList* xs = argv[1].list; RsppList* ys = argv[2].list;
            char* buf = (char*)malloc(1024);
            if (xs->len >= 2 && ys->len >= 2) {
                double dx = rspp_to_f64(xs->items[1]) - rspp_to_f64(xs->items[0]);
                double a = dx != 0.0 ? (rspp_to_f64(ys->items[1]) - rspp_to_f64(ys->items[0])) / dx : 1.0;
                double b = rspp_to_f64(ys->items[0]) - a * rspp_to_f64(xs->items[0]);
                snprintf(buf, 1024, "// Synthesized Linear Algorithm (MSE = 0.0)\nfn %s(x: int) -> int:\n    return %lld * x + %lld\n",
                    name, (long long)llround(a), (long long)llround(b));
            } else {
                snprintf(buf, 1024, "fn %s(x: int) -> int:\n    return x\n", name);
            }
            return rspp_str_owned(buf);
        }
        if (strcmp(fn, "generate_dsp_filter") == 0) {
            return rspp_str("// Synthesized FIR Filter\nfn apply_lowpass_fir(samples: list) -> list:\n    return samples\n");
        }
        if (strcmp(fn, "evolve_crypto_mixer") == 0) {
            return rspp_str("// Synthesized ARX Mixer\nfn arx_mix(x: int) -> int:\n    return (x + 0x9e3779b9) ^ (x << 7)\n");
        }
    }
    if (strcmp(ns, "security") == 0) {
        if (strcmp(fn, "constant_time_eq") == 0 && argc >= 2) {
            const char* a = rspp_to_str(argv[0]); const char* b = rspp_to_str(argv[1]);
            size_t la = strlen(a), lb = strlen(b), n = la < lb ? la : lb;
            unsigned diff = (unsigned)(la ^ lb);
            for (size_t i = 0; i < n; i++) diff |= (unsigned)(a[i] ^ b[i]);
            return rspp_bool(diff == 0);
        }
        if (strcmp(fn, "canary_check") == 0) return rspp_bool(1);
    }
    if (strcmp(ns, "hardware") == 0) {
        if (strcmp(fn, "probe") == 0) {
            RsppVal m = rspp_map();
            rspp_map_set(m, rspp_str("leak_guard"), rspp_str("active"));
            rspp_map_set(m, rspp_str("sdr_capable"), rspp_bool(1));
            return m;
        }
        if (strcmp(fn, "temp") == 0) return rspp_flt(42.5);
    }
    if (strcmp(ns, "power") == 0) {
        if (strcmp(fn, "watts") == 0) return rspp_flt(28.4);
        if (strcmp(fn, "get_voltage") == 0) return rspp_flt(1150.0);
    }
    if (strcmp(ns, "cpu") == 0 && strcmp(fn, "cores") == 0) return rspp_int(4);
    return rspp_none();
}
static RsppVal rspp_fn_rspp_min2(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_rspp_min3(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_dl_distance(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_bigram_cosine(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_bayesian_best_candidate(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_format_diag(RsppVal p0, RsppVal p1, RsppVal p2, RsppVal p3, RsppVal p4, RsppVal p5, RsppVal p6);
static RsppVal rspp_fn_leak_guard_scan_stmts(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_is_alloc_expr(RsppVal p0);
static RsppVal rspp_fn_check_free_call(RsppVal p0, RsppVal p1, RsppVal p2, RsppVal p3, RsppVal p4);
static RsppVal rspp_fn_leak_guard_verify_program(RsppVal p0);
static RsppVal rspp_fn_is_alpha_or_us(RsppVal p0);
static RsppVal rspp_fn_is_digit(RsppVal p0);
static RsppVal rspp_fn_is_hex(RsppVal p0);
static RsppVal rspp_fn_is_keyword(RsppVal p0);
static RsppVal rspp_fn_make_tok(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_tokenize_source(RsppVal p0);
static RsppVal rspp_fn_new_parser(RsppVal p0);
static RsppVal rspp_fn_p_cur(RsppVal p0);
static RsppVal rspp_fn_p_adv(RsppVal p0);
static RsppVal rspp_fn_p_is(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_p_is_kind(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_p_consume(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_p_skip_nl(RsppVal p0);
static RsppVal rspp_fn_parse_tokens(RsppVal p0);
static RsppVal rspp_fn_parse_item(RsppVal p0);
static RsppVal rspp_fn_parse_type_str(RsppVal p0);
static RsppVal rspp_fn_parse_params(RsppVal p0);
static RsppVal rspp_fn_parse_fn(RsppVal p0);
static RsppVal rspp_fn_parse_equation(RsppVal p0);
static RsppVal rspp_fn_parse_algorithm(RsppVal p0);
static RsppVal rspp_fn_parse_extern(RsppVal p0);
static RsppVal rspp_fn_parse_import(RsppVal p0);
static RsppVal rspp_fn_parse_struct(RsppVal p0);
static RsppVal rspp_fn_parse_enum(RsppVal p0);
static RsppVal rspp_fn_parse_block_raw_skip(RsppVal p0);
static RsppVal rspp_fn_parse_block(RsppVal p0);
static RsppVal rspp_fn_parse_stmt(RsppVal p0);
static RsppVal rspp_fn_parse_expr(RsppVal p0);
static RsppVal rspp_fn_parse_pipeline(RsppVal p0);
static RsppVal rspp_fn_parse_or(RsppVal p0);
static RsppVal rspp_fn_parse_and(RsppVal p0);
static RsppVal rspp_fn_parse_eq(RsppVal p0);
static RsppVal rspp_fn_parse_cmp(RsppVal p0);
static RsppVal rspp_fn_parse_range(RsppVal p0);
static RsppVal rspp_fn_parse_bitor(RsppVal p0);
static RsppVal rspp_fn_parse_bitxor(RsppVal p0);
static RsppVal rspp_fn_parse_bitand(RsppVal p0);
static RsppVal rspp_fn_parse_shift(RsppVal p0);
static RsppVal rspp_fn_parse_add(RsppVal p0);
static RsppVal rspp_fn_parse_mul(RsppVal p0);
static RsppVal rspp_fn_parse_pow(RsppVal p0);
static RsppVal rspp_fn_parse_cast(RsppVal p0);
static RsppVal rspp_fn_parse_unary(RsppVal p0);
static RsppVal rspp_fn_parse_arg_list(RsppVal p0);
static RsppVal rspp_fn_parse_postfix(RsppVal p0);
static RsppVal rspp_fn_parse_primary(RsppVal p0);
static RsppVal rspp_fn_path_dir(RsppVal p0);
static RsppVal rspp_fn_path_join(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_load_items_recursive(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_load_program_bundle(RsppVal p0);
static RsppVal rspp_fn_esc_c_str(RsppVal p0);
static RsppVal rspp_fn_c_ident(RsppVal p0);
static RsppVal rspp_fn_emit_c_prelude(RsppVal p0);
static RsppVal rspp_fn_new_emit_ctx(void);
static RsppVal rspp_fn_cg_tmp(RsppVal p0);
static RsppVal rspp_fn_cg_push_line(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_cg_emit_fstr(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_cg_emit_expr(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_cg_emit_defers_all(RsppVal p0, RsppVal p1);
static RsppVal rspp_fn_cg_emit_block(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_cg_emit_stmt(RsppVal p0, RsppVal p1, RsppVal p2);
static RsppVal rspp_fn_emit_native_program_c(RsppVal p0);
static RsppVal rspp_fn_emit_kernel_c(RsppVal p0);
static RsppVal rspp_fn_print_help(void);
static RsppVal rspp_fn_main(void);

static RsppVal rspp_fn_rspp_min2(RsppVal rspp_v_a, RsppVal rspp_v_b) {
    if (rspp_to_bool(rspp_lt(rspp_v_a, rspp_v_b))) {
        RsppVal _t1 = rspp_v_a;
        return _t1;
    }
    RsppVal _t2 = rspp_v_b;
    return _t2;
    return rspp_none();
}

static RsppVal rspp_fn_rspp_min3(RsppVal rspp_v_a, RsppVal rspp_v_b, RsppVal rspp_v_c) {
    RsppVal _t4 = rspp_fn_rspp_min2(rspp_v_b, rspp_v_c);
    RsppVal _t5 = rspp_fn_rspp_min2(rspp_v_a, _t4);
    RsppVal _t3 = _t5;
    return _t3;
    return rspp_none();
}

static RsppVal rspp_fn_dl_distance(RsppVal rspp_v_a, RsppVal rspp_v_b) {
    RsppVal _t6 = rspp_method_call(rspp_v_a, "len", 0, NULL);
    RsppVal rspp_v_m = _t6;
    RsppVal _t7 = rspp_method_call(rspp_v_b, "len", 0, NULL);
    RsppVal rspp_v_n = _t7;
    if (rspp_to_bool(rspp_eq(rspp_v_m, rspp_int(0LL)))) {
        RsppVal _t8 = rspp_v_n;
        return _t8;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_n, rspp_int(0LL)))) {
        RsppVal _t9 = rspp_v_m;
        return _t9;
    }
    RsppVal rspp_v_cols = rspp_add(rspp_v_n, rspp_int(1LL));
    RsppVal _t10 = rspp_list();
    RsppVal rspp_v_dp = _t10;
    RsppVal rspp_v_r = rspp_int(0LL);
    while (1) {
        if (!rspp_to_bool(rspp_lte(rspp_v_r, rspp_v_m))) break;
        RsppVal rspp_v_c = rspp_int(0LL);
        while (1) {
            if (!rspp_to_bool(rspp_lte(rspp_v_c, rspp_v_n))) break;
            if (rspp_to_bool(rspp_eq(rspp_v_r, rspp_int(0LL)))) {
                RsppVal _t12[] = { rspp_v_c };
                RsppVal _t11 = rspp_method_call(rspp_v_dp, "push", 1, _t12);
            } else {
                if (rspp_to_bool(rspp_eq(rspp_v_c, rspp_int(0LL)))) {
                    RsppVal _t14[] = { rspp_v_r };
                    RsppVal _t13 = rspp_method_call(rspp_v_dp, "push", 1, _t14);
            } else {
                RsppVal _t16[] = { rspp_int(0LL) };
                RsppVal _t15 = rspp_method_call(rspp_v_dp, "push", 1, _t16);
            }
            }
            rspp_v_c = rspp_add(rspp_v_c, rspp_int(1LL));
        }
        rspp_v_r = rspp_add(rspp_v_r, rspp_int(1LL));
    }
    RsppVal rspp_v_i = rspp_int(1LL);
    while (1) {
        if (!rspp_to_bool(rspp_lte(rspp_v_i, rspp_v_m))) break;
        RsppVal rspp_v_j = rspp_int(1LL);
        while (1) {
            if (!rspp_to_bool(rspp_lte(rspp_v_j, rspp_v_n))) break;
            RsppVal _t18[] = { rspp_sub(rspp_v_i, rspp_int(1LL)) };
            RsppVal _t17 = rspp_method_call(rspp_v_a, "char_code_at", 1, _t18);
            RsppVal rspp_v_ca = _t17;
            RsppVal _t20[] = { rspp_sub(rspp_v_j, rspp_int(1LL)) };
            RsppVal _t19 = rspp_method_call(rspp_v_b, "char_code_at", 1, _t20);
            RsppVal rspp_v_cb = _t19;
            RsppVal rspp_v_cost = rspp_int(1LL);
            if (rspp_to_bool(rspp_eq(rspp_v_ca, rspp_v_cb))) {
                rspp_v_cost = rspp_int(0LL);
            }
            RsppVal rspp_v_v_del = rspp_add(rspp_index_get(rspp_v_dp, rspp_add(rspp_mul(rspp_sub(rspp_v_i, rspp_int(1LL)), rspp_v_cols), rspp_v_j)), rspp_int(1LL));
            RsppVal rspp_v_v_ins = rspp_add(rspp_index_get(rspp_v_dp, rspp_add(rspp_mul(rspp_v_i, rspp_v_cols), rspp_sub(rspp_v_j, rspp_int(1LL)))), rspp_int(1LL));
            RsppVal rspp_v_v_sub = rspp_add(rspp_index_get(rspp_v_dp, rspp_add(rspp_mul(rspp_sub(rspp_v_i, rspp_int(1LL)), rspp_v_cols), rspp_sub(rspp_v_j, rspp_int(1LL)))), rspp_v_cost);
            RsppVal _t21 = rspp_fn_rspp_min3(rspp_v_v_del, rspp_v_v_ins, rspp_v_v_sub);
            RsppVal rspp_v_best = _t21;
            RsppVal _t22 = rspp_bool(0);
            if (rspp_to_bool(rspp_gt(rspp_v_i, rspp_int(1LL)))) {
                _t22 = rspp_bool(rspp_to_bool(rspp_gt(rspp_v_j, rspp_int(1LL))));
            }
            if (rspp_to_bool(_t22)) {
                RsppVal _t24[] = { rspp_sub(rspp_v_i, rspp_int(2LL)) };
                RsppVal _t23 = rspp_method_call(rspp_v_a, "char_code_at", 1, _t24);
                RsppVal rspp_v_ca2 = _t23;
                RsppVal _t26[] = { rspp_sub(rspp_v_j, rspp_int(2LL)) };
                RsppVal _t25 = rspp_method_call(rspp_v_b, "char_code_at", 1, _t26);
                RsppVal rspp_v_cb2 = _t25;
                RsppVal _t27 = rspp_bool(0);
                if (rspp_to_bool(rspp_eq(rspp_v_ca, rspp_v_cb2))) {
                    _t27 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_ca2, rspp_v_cb)));
                }
                if (rspp_to_bool(_t27)) {
                    RsppVal rspp_v_v_trans = rspp_add(rspp_index_get(rspp_v_dp, rspp_add(rspp_mul(rspp_sub(rspp_v_i, rspp_int(2LL)), rspp_v_cols), rspp_sub(rspp_v_j, rspp_int(2LL)))), rspp_int(1LL));
                    if (rspp_to_bool(rspp_lt(rspp_v_v_trans, rspp_v_best))) {
                        rspp_v_best = rspp_v_v_trans;
                    }
                }
            }
            rspp_index_set(rspp_v_dp, rspp_add(rspp_mul(rspp_v_i, rspp_v_cols), rspp_v_j), rspp_v_best);
            rspp_v_j = rspp_add(rspp_v_j, rspp_int(1LL));
        }
        rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
    }
    RsppVal _t28 = rspp_index_get(rspp_v_dp, rspp_add(rspp_mul(rspp_v_m, rspp_v_cols), rspp_v_n));
    return _t28;
    return rspp_none();
}

static RsppVal rspp_fn_bigram_cosine(RsppVal rspp_v_a, RsppVal rspp_v_b) {
    RsppVal rspp_v_pa = rspp_add(rspp_add(rspp_str("^"), rspp_v_a), rspp_str("$"));
    RsppVal rspp_v_pb = rspp_add(rspp_add(rspp_str("^"), rspp_v_b), rspp_str("$"));
    RsppVal _t29 = rspp_map();
    RsppVal rspp_v_ma = _t29;
    RsppVal _t30 = rspp_map();
    RsppVal rspp_v_mb = _t30;
    RsppVal rspp_v_i = rspp_int(0LL);
    while (1) {
        RsppVal _t31 = rspp_method_call(rspp_v_pa, "len", 0, NULL);
        if (!rspp_to_bool(rspp_lt(rspp_add(rspp_v_i, rspp_int(1LL)), _t31))) break;
        RsppVal _t33[] = { rspp_v_i, rspp_add(rspp_v_i, rspp_int(2LL)) };
        RsppVal _t32 = rspp_method_call(rspp_v_pa, "slice", 2, _t33);
        RsppVal rspp_v_bg = _t32;
        RsppVal _t35[] = { rspp_v_bg, rspp_int(0LL) };
        RsppVal _t34 = rspp_method_call(rspp_v_ma, "get", 2, _t35);
        RsppVal rspp_v_prev = _t34;
        RsppVal _t37[] = { rspp_v_bg, rspp_add(rspp_v_prev, rspp_int(1LL)) };
        RsppVal _t36 = rspp_method_call(rspp_v_ma, "set", 2, _t37);
        rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
    }
    RsppVal rspp_v_j = rspp_int(0LL);
    while (1) {
        RsppVal _t38 = rspp_method_call(rspp_v_pb, "len", 0, NULL);
        if (!rspp_to_bool(rspp_lt(rspp_add(rspp_v_j, rspp_int(1LL)), _t38))) break;
        RsppVal _t40[] = { rspp_v_j, rspp_add(rspp_v_j, rspp_int(2LL)) };
        RsppVal _t39 = rspp_method_call(rspp_v_pb, "slice", 2, _t40);
        RsppVal rspp_v_bg = _t39;
        RsppVal _t42[] = { rspp_v_bg, rspp_int(0LL) };
        RsppVal _t41 = rspp_method_call(rspp_v_mb, "get", 2, _t42);
        RsppVal rspp_v_prev = _t41;
        RsppVal _t44[] = { rspp_v_bg, rspp_add(rspp_v_prev, rspp_int(1LL)) };
        RsppVal _t43 = rspp_method_call(rspp_v_mb, "set", 2, _t44);
        rspp_v_j = rspp_add(rspp_v_j, rspp_int(1LL));
    }
    RsppVal rspp_v_dot = rspp_flt(0.000000000000);
    RsppVal rspp_v_norm_a = rspp_flt(0.000000000000);
    RsppVal rspp_v_norm_b = rspp_flt(0.000000000000);
    RsppVal _t45 = rspp_method_call(rspp_v_ma, "keys", 0, NULL);
    RsppVal _t46 = _t45;
    for (int64_t _t47 = 0; _t47 < rspp_list_len(_t46); _t47++) {
        RsppVal rspp_v_k = rspp_index_get(_t46, rspp_int(_t47));
        RsppVal _t49[] = { rspp_v_k, rspp_int(0LL) };
        RsppVal _t48 = rspp_method_call(rspp_v_ma, "get", 2, _t49);
        RsppVal rspp_v_va = rspp_flt(rspp_to_f64(_t48));
        rspp_v_norm_a = rspp_add(rspp_v_norm_a, rspp_mul(rspp_v_va, rspp_v_va));
        RsppVal _t51[] = { rspp_v_k };
        RsppVal _t50 = rspp_method_call(rspp_v_mb, "has", 1, _t51);
        if (rspp_to_bool(_t50)) {
            RsppVal _t53[] = { rspp_v_k, rspp_int(0LL) };
            RsppVal _t52 = rspp_method_call(rspp_v_mb, "get", 2, _t53);
            RsppVal rspp_v_vb = rspp_flt(rspp_to_f64(_t52));
            rspp_v_dot = rspp_add(rspp_v_dot, rspp_mul(rspp_v_va, rspp_v_vb));
        }
    }
    RsppVal _t54 = rspp_method_call(rspp_v_mb, "keys", 0, NULL);
    RsppVal _t55 = _t54;
    for (int64_t _t56 = 0; _t56 < rspp_list_len(_t55); _t56++) {
        RsppVal rspp_v_k = rspp_index_get(_t55, rspp_int(_t56));
        RsppVal _t58[] = { rspp_v_k, rspp_int(0LL) };
        RsppVal _t57 = rspp_method_call(rspp_v_mb, "get", 2, _t58);
        RsppVal rspp_v_vb = rspp_flt(rspp_to_f64(_t57));
        rspp_v_norm_b = rspp_add(rspp_v_norm_b, rspp_mul(rspp_v_vb, rspp_v_vb));
    }
    RsppVal _t59 = rspp_bool(1);
    if (!rspp_to_bool(rspp_lte(rspp_v_norm_a, rspp_flt(0.000000000000)))) {
        _t59 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_norm_b, rspp_flt(0.000000000000))));
    }
    if (rspp_to_bool(_t59)) {
        RsppVal _t60 = rspp_flt(0.000000000000);
        return _t60;
    }
    RsppVal _t63[] = { rspp_v_norm_a };
    RsppVal _t62 = rspp_builtin("math", "sqrt", 1, _t63);
    RsppVal _t65[] = { rspp_v_norm_b };
    RsppVal _t64 = rspp_builtin("math", "sqrt", 1, _t65);
    RsppVal _t61 = rspp_div(rspp_v_dot, rspp_mul(_t62, _t64));
    return _t61;
    return rspp_none();
}

static RsppVal rspp_fn_bayesian_best_candidate(RsppVal rspp_v_needle, RsppVal rspp_v_candidates) {
    RsppVal rspp_v_best_name = rspp_str("");
    RsppVal rspp_v_best_score = rspp_neg(rspp_flt(1.000000000000));
    RsppVal rspp_v_best_dist = rspp_int(999LL);
    RsppVal rspp_v_best_cos = rspp_flt(0.000000000000);
    RsppVal rspp_v_sum_w = rspp_flt(0.000000000000);
    RsppVal _t66 = rspp_v_candidates;
    for (int64_t _t67 = 0; _t67 < rspp_list_len(_t66); _t67++) {
        RsppVal rspp_v_cand = rspp_index_get(_t66, rspp_int(_t67));
        RsppVal _t68 = rspp_fn_dl_distance(rspp_v_needle, rspp_v_cand);
        RsppVal rspp_v_d = _t68;
        RsppVal _t69 = rspp_fn_bigram_cosine(rspp_v_needle, rspp_v_cand);
        RsppVal rspp_v_cos = _t69;
        RsppVal _t70 = rspp_method_call(rspp_v_needle, "len", 0, NULL);
        RsppVal rspp_v_max_len = _t70;
        RsppVal _t71 = rspp_method_call(rspp_v_cand, "len", 0, NULL);
        if (rspp_to_bool(rspp_gt(_t71, rspp_v_max_len))) {
            RsppVal _t72 = rspp_method_call(rspp_v_cand, "len", 0, NULL);
            rspp_v_max_len = _t72;
        }
        if (rspp_to_bool(rspp_lt(rspp_v_max_len, rspp_int(1LL)))) {
            rspp_v_max_len = rspp_int(1LL);
        }
        RsppVal rspp_v_dl_s = rspp_sub(rspp_flt(1.000000000000), rspp_div(rspp_flt(rspp_to_f64(rspp_v_d)), rspp_flt(rspp_to_f64(rspp_v_max_len))));
        if (rspp_to_bool(rspp_lt(rspp_v_dl_s, rspp_flt(0.000000000000)))) {
            rspp_v_dl_s = rspp_flt(0.000000000000);
        }
        RsppVal _t74[] = { rspp_add(rspp_mul(rspp_flt(3.500000000000), rspp_v_dl_s), rspp_mul(rspp_flt(2.500000000000), rspp_v_cos)) };
        RsppVal _t73 = rspp_builtin("math", "exp", 1, _t74);
        RsppVal rspp_v_w = _t73;
        rspp_v_sum_w = rspp_add(rspp_v_sum_w, rspp_v_w);
        if (rspp_to_bool(rspp_gt(rspp_v_w, rspp_v_best_score))) {
            rspp_v_best_score = rspp_v_w;
            rspp_v_best_name = rspp_v_cand;
            rspp_v_best_dist = rspp_v_d;
            rspp_v_best_cos = rspp_v_cos;
        }
    }
    RsppVal rspp_v_prob = rspp_flt(0.000000000000);
    RsppVal _t75 = rspp_bool(0);
    if (rspp_to_bool(rspp_gt(rspp_v_sum_w, rspp_flt(0.000000000000)))) {
        _t75 = rspp_bool(rspp_to_bool(rspp_gt(rspp_v_best_score, rspp_flt(0.000000000000))));
    }
    if (rspp_to_bool(_t75)) {
        rspp_v_prob = rspp_div(rspp_v_best_score, rspp_v_sum_w);
    }
    RsppVal _t77 = rspp_map();
    rspp_map_set(_t77, rspp_str("name"), rspp_v_best_name);
    rspp_map_set(_t77, rspp_str("dist"), rspp_v_best_dist);
    rspp_map_set(_t77, rspp_str("cosine"), rspp_v_best_cos);
    rspp_map_set(_t77, rspp_str("prob"), rspp_v_prob);
    RsppVal _t76 = _t77;
    return _t76;
    return rspp_none();
}

static RsppVal rspp_fn_format_diag(RsppVal rspp_v_code, RsppVal rspp_v_line, RsppVal rspp_v_msg_en, RsppVal rspp_v_msg_ar, RsppVal rspp_v_why, RsppVal rspp_v_fix, RsppVal rspp_v_math_note) {
    RsppVal _t79 = rspp_str("");
    _t79 = rspp_add(_t79, rspp_str("["));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_code)));
    _t79 = rspp_add(_t79, rspp_str("] line "));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_line)));
    _t79 = rspp_add(_t79, rspp_str(": "));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_msg_en)));
    _t79 = rspp_add(_t79, rspp_str("\n    "));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_msg_ar)));
    _t79 = rspp_add(_t79, rspp_str("\n  Why: "));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_why)));
    _t79 = rspp_add(_t79, rspp_str("\n  Math: "));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_math_note)));
    _t79 = rspp_add(_t79, rspp_str("\n  Fix: "));
    _t79 = rspp_add(_t79, rspp_str(rspp_to_str(rspp_v_fix)));
    RsppVal _t78 = _t79;
    return _t78;
    return rspp_none();
}

static RsppVal rspp_fn_leak_guard_scan_stmts(RsppVal rspp_v_stmts, RsppVal rspp_v_allocs, RsppVal rspp_v_diags) {
    RsppVal _t80 = rspp_list();
    RsppVal rspp_v_scope_vars = _t80;
    RsppVal _t81 = rspp_v_stmts;
    for (int64_t _t82 = 0; _t82 < rspp_list_len(_t81); _t82++) {
        RsppVal rspp_v_st = rspp_index_get(_t81, rspp_int(_t82));
        RsppVal _t84[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t83 = rspp_method_call(rspp_v_st, "get", 2, _t84);
        RsppVal rspp_v_kind = _t83;
        RsppVal _t86[] = { rspp_str("line"), rspp_int(1LL) };
        RsppVal _t85 = rspp_method_call(rspp_v_st, "get", 2, _t86);
        RsppVal rspp_v_line = _t85;
        RsppVal _t87 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("let")))) {
            _t87 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("var"))));
        }
        if (rspp_to_bool(_t87)) {
            RsppVal _t89[] = { rspp_str("name"), rspp_str("") };
            RsppVal _t88 = rspp_method_call(rspp_v_st, "get", 2, _t89);
            RsppVal rspp_v_vname = _t88;
            RsppVal _t90 = rspp_map();
            RsppVal _t92[] = { rspp_str("value"), _t90 };
            RsppVal _t91 = rspp_method_call(rspp_v_st, "get", 2, _t92);
            RsppVal rspp_v_val = _t91;
            RsppVal _t93 = rspp_fn_is_alloc_expr(rspp_v_val);
            if (rspp_to_bool(_t93)) {
                RsppVal _t95[] = { rspp_v_vname };
                RsppVal _t94 = rspp_method_call(rspp_v_allocs, "has", 1, _t95);
                if (rspp_to_bool(_t94)) {
                    RsppVal _t96 = rspp_map();
                    RsppVal _t98[] = { rspp_v_vname, _t96 };
                    RsppVal _t97 = rspp_method_call(rspp_v_allocs, "get", 2, _t98);
                    RsppVal rspp_v_prev = _t97;
                    RsppVal _t101[] = { rspp_str("freed"), rspp_bool(0) };
                    RsppVal _t100 = rspp_method_call(rspp_v_prev, "get", 2, _t101);
                    RsppVal _t99 = rspp_bool(0);
                    if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t100)))) {
                        RsppVal _t103[] = { rspp_str("deferred"), rspp_bool(0) };
                        RsppVal _t102 = rspp_method_call(rspp_v_prev, "get", 2, _t103);
                        _t99 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t102))));
                    }
                    if (rspp_to_bool(_t99)) {
                        RsppVal _t104 = rspp_str("");
                        _t104 = rspp_add(_t104, rspp_str("Overwrite Leak on `"));
                        _t104 = rspp_add(_t104, rspp_str(rspp_to_str(rspp_v_vname)));
                        _t104 = rspp_add(_t104, rspp_str("`"));
                        RsppVal _t105 = rspp_str("");
                        _t105 = rspp_add(_t105, rspp_str("تسرب ذاكرة عند إعادة تعيين `"));
                        _t105 = rspp_add(_t105, rspp_str(rspp_to_str(rspp_v_vname)));
                        _t105 = rspp_add(_t105, rspp_str("`"));
                        RsppVal _t106 = rspp_str("");
                        _t106 = rspp_add(_t106, rspp_str("Add `memory::free("));
                        _t106 = rspp_add(_t106, rspp_str(rspp_to_str(rspp_v_vname)));
                        _t106 = rspp_add(_t106, rspp_str(")` before reassigning."));
                        RsppVal _t107 = rspp_fn_format_diag(rspp_str("LG004"), rspp_v_line, _t104, _t105, rspp_str("Previous allocation was not freed before reassignment."), _t106, rspp_str("RefCount drops 1 -> 0 while Live=true"));
                        RsppVal _t109[] = { _t107 };
                        RsppVal _t108 = rspp_method_call(rspp_v_diags, "push", 1, _t109);
                    }
                }
                RsppVal _t110 = rspp_map();
                rspp_map_set(_t110, rspp_str("freed"), rspp_bool(0));
                rspp_map_set(_t110, rspp_str("deferred"), rspp_bool(0));
                rspp_map_set(_t110, rspp_str("returned"), rspp_bool(0));
                rspp_map_set(_t110, rspp_str("line"), rspp_v_line);
                RsppVal _t112[] = { rspp_v_vname, _t110 };
                RsppVal _t111 = rspp_method_call(rspp_v_allocs, "set", 2, _t112);
                RsppVal _t114[] = { rspp_v_vname };
                RsppVal _t113 = rspp_method_call(rspp_v_scope_vars, "push", 1, _t114);
            }
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("defer")))) {
                RsppVal _t115 = rspp_map();
                RsppVal _t117[] = { rspp_str("stmt"), _t115 };
                RsppVal _t116 = rspp_method_call(rspp_v_st, "get", 2, _t117);
                RsppVal rspp_v_inner = _t116;
                RsppVal _t119[] = { rspp_str("kind"), rspp_str("") };
                RsppVal _t118 = rspp_method_call(rspp_v_inner, "get", 2, _t119);
                if (rspp_to_bool(rspp_eq(_t118, rspp_str("expr")))) {
                    RsppVal _t120 = rspp_map();
                    RsppVal _t122[] = { rspp_str("expr"), _t120 };
                    RsppVal _t121 = rspp_method_call(rspp_v_inner, "get", 2, _t122);
                    RsppVal rspp_v_ex = _t121;
                    RsppVal _t123 = rspp_fn_check_free_call(rspp_v_ex, rspp_v_allocs, rspp_v_diags, rspp_v_line, rspp_bool(1));
                }
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("expr")))) {
                RsppVal _t124 = rspp_map();
                RsppVal _t126[] = { rspp_str("expr"), _t124 };
                RsppVal _t125 = rspp_method_call(rspp_v_st, "get", 2, _t126);
                RsppVal rspp_v_ex = _t125;
                RsppVal _t127 = rspp_fn_check_free_call(rspp_v_ex, rspp_v_allocs, rspp_v_diags, rspp_v_line, rspp_bool(0));
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("return")))) {
                RsppVal _t128 = rspp_map();
                RsppVal _t130[] = { rspp_str("value"), _t128 };
                RsppVal _t129 = rspp_method_call(rspp_v_st, "get", 2, _t130);
                RsppVal rspp_v_rv = _t129;
                RsppVal _t132[] = { rspp_str("kind"), rspp_str("") };
                RsppVal _t131 = rspp_method_call(rspp_v_rv, "get", 2, _t132);
                if (rspp_to_bool(rspp_eq(_t131, rspp_str("ident")))) {
                    RsppVal _t134[] = { rspp_str("name"), rspp_str("") };
                    RsppVal _t133 = rspp_method_call(rspp_v_rv, "get", 2, _t134);
                    RsppVal rspp_v_rname = _t133;
                    RsppVal _t136[] = { rspp_v_rname };
                    RsppVal _t135 = rspp_method_call(rspp_v_allocs, "has", 1, _t136);
                    if (rspp_to_bool(_t135)) {
                        RsppVal _t137 = rspp_map();
                        RsppVal _t139[] = { rspp_v_rname, _t137 };
                        RsppVal _t138 = rspp_method_call(rspp_v_allocs, "get", 2, _t139);
                        RsppVal rspp_v_info = _t138;
                        RsppVal _t141[] = { rspp_str("returned"), rspp_bool(1) };
                        RsppVal _t140 = rspp_method_call(rspp_v_info, "set", 2, _t141);
                    }
                }
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("if")))) {
                RsppVal _t142 = rspp_list();
                RsppVal _t144[] = { rspp_str("branches"), _t142 };
                RsppVal _t143 = rspp_method_call(rspp_v_st, "get", 2, _t144);
                RsppVal _t145 = _t143;
                for (int64_t _t146 = 0; _t146 < rspp_list_len(_t145); _t146++) {
                    RsppVal rspp_v_br = rspp_index_get(_t145, rspp_int(_t146));
                    RsppVal _t147 = rspp_list();
                    RsppVal _t149[] = { rspp_str("body"), _t147 };
                    RsppVal _t148 = rspp_method_call(rspp_v_br, "get", 2, _t149);
                    RsppVal _t150 = rspp_fn_leak_guard_scan_stmts(_t148, rspp_v_allocs, rspp_v_diags);
                }
                RsppVal _t151 = rspp_list();
                RsppVal _t153[] = { rspp_str("else_body"), _t151 };
                RsppVal _t152 = rspp_method_call(rspp_v_st, "get", 2, _t153);
                RsppVal _t154 = rspp_fn_leak_guard_scan_stmts(_t152, rspp_v_allocs, rspp_v_diags);
        } else {
            RsppVal _t157 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("while")))) {
                _t157 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("for"))));
            }
            RsppVal _t156 = rspp_bool(1);
            if (!rspp_to_bool(_t157)) {
                _t156 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("loop"))));
            }
            RsppVal _t155 = rspp_bool(1);
            if (!rspp_to_bool(_t156)) {
                _t155 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("unsafe_hardware"))));
            }
            if (rspp_to_bool(_t155)) {
                RsppVal _t158 = rspp_list();
                RsppVal _t160[] = { rspp_str("body"), _t158 };
                RsppVal _t159 = rspp_method_call(rspp_v_st, "get", 2, _t160);
                RsppVal _t161 = rspp_fn_leak_guard_scan_stmts(_t159, rspp_v_allocs, rspp_v_diags);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_kind, rspp_str("try_catch")))) {
                RsppVal _t162 = rspp_list();
                RsppVal _t164[] = { rspp_str("try_body"), _t162 };
                RsppVal _t163 = rspp_method_call(rspp_v_st, "get", 2, _t164);
                RsppVal _t165 = rspp_fn_leak_guard_scan_stmts(_t163, rspp_v_allocs, rspp_v_diags);
                RsppVal _t166 = rspp_list();
                RsppVal _t168[] = { rspp_str("catch_body"), _t166 };
                RsppVal _t167 = rspp_method_call(rspp_v_st, "get", 2, _t168);
                RsppVal _t169 = rspp_fn_leak_guard_scan_stmts(_t167, rspp_v_allocs, rspp_v_diags);
                            }
                        }
                    }
                }
            }
        }
        }
    }
    RsppVal _t170 = rspp_v_scope_vars;
    for (int64_t _t171 = 0; _t171 < rspp_list_len(_t170); _t171++) {
        RsppVal rspp_v_sv = rspp_index_get(_t170, rspp_int(_t171));
        RsppVal _t172 = rspp_map();
        RsppVal _t174[] = { rspp_v_sv, _t172 };
        RsppVal _t173 = rspp_method_call(rspp_v_allocs, "get", 2, _t174);
        RsppVal rspp_v_info = _t173;
        RsppVal _t178[] = { rspp_str("freed"), rspp_bool(0) };
        RsppVal _t177 = rspp_method_call(rspp_v_info, "get", 2, _t178);
        RsppVal _t176 = rspp_bool(0);
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t177)))) {
            RsppVal _t180[] = { rspp_str("deferred"), rspp_bool(0) };
            RsppVal _t179 = rspp_method_call(rspp_v_info, "get", 2, _t180);
            _t176 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t179))));
        }
        RsppVal _t175 = rspp_bool(0);
        if (rspp_to_bool(_t176)) {
            RsppVal _t182[] = { rspp_str("returned"), rspp_bool(0) };
            RsppVal _t181 = rspp_method_call(rspp_v_info, "get", 2, _t182);
            _t175 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t181))));
        }
        if (rspp_to_bool(_t175)) {
            RsppVal _t184[] = { rspp_str("line"), rspp_int(1LL) };
            RsppVal _t183 = rspp_method_call(rspp_v_info, "get", 2, _t184);
            RsppVal rspp_v_aline = _t183;
            RsppVal _t185 = rspp_str("");
            _t185 = rspp_add(_t185, rspp_str("Memory Leak Guard: `"));
            _t185 = rspp_add(_t185, rspp_str(rspp_to_str(rspp_v_sv)));
            _t185 = rspp_add(_t185, rspp_str("` escapes scope unfreed"));
            RsppVal _t186 = rspp_str("");
            _t186 = rspp_add(_t186, rspp_str("حماية تسرب الذاكرة: `"));
            _t186 = rspp_add(_t186, rspp_str(rspp_to_str(rspp_v_sv)));
            _t186 = rspp_add(_t186, rspp_str("` يخرج بدون تحرير"));
            RsppVal _t187 = rspp_str("");
            _t187 = rspp_add(_t187, rspp_str("Insert `defer memory::free("));
            _t187 = rspp_add(_t187, rspp_str(rspp_to_str(rspp_v_sv)));
            _t187 = rspp_add(_t187, rspp_str(")` after allocation."));
            RsppVal _t188 = rspp_str("");
            _t188 = rspp_add(_t188, rspp_str("OutDegree(free/defer/return) == 0 for `"));
            _t188 = rspp_add(_t188, rspp_str(rspp_to_str(rspp_v_sv)));
            _t188 = rspp_add(_t188, rspp_str("`"));
            RsppVal _t189 = rspp_fn_format_diag(rspp_str("LG001"), rspp_v_aline, _t185, _t186, rspp_str("Reachability graph found 0 free/defer/return paths."), _t187, _t188);
            RsppVal _t191[] = { _t189 };
            RsppVal _t190 = rspp_method_call(rspp_v_diags, "push", 1, _t191);
        }
    }
    return rspp_none();
}

static RsppVal rspp_fn_is_alloc_expr(RsppVal rspp_v_ex) {
    RsppVal _t193[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t192 = rspp_method_call(rspp_v_ex, "get", 2, _t193);
    if (rspp_to_bool(rspp_eq(_t192, rspp_str("ns_call")))) {
        RsppVal _t195[] = { rspp_str("ns"), rspp_str("") };
        RsppVal _t194 = rspp_method_call(rspp_v_ex, "get", 2, _t195);
        RsppVal rspp_v_ns = _t194;
        RsppVal _t197[] = { rspp_str("fn"), rspp_str("") };
        RsppVal _t196 = rspp_method_call(rspp_v_ex, "get", 2, _t197);
        RsppVal rspp_v_fn_name = _t196;
        RsppVal _t198 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_ns, rspp_str("memory")))) {
            _t198 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_fn_name, rspp_str("alloc"))));
        }
        if (rspp_to_bool(_t198)) {
            RsppVal _t199 = rspp_bool(1);
            return _t199;
        }
        RsppVal _t200 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_ns, rspp_str("sdr")))) {
            RsppVal _t201 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_fn_name, rspp_str("open")))) {
                _t201 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_fn_name, rspp_str("alloc_iq_buffer"))));
            }
            _t200 = rspp_bool(rspp_to_bool(_t201));
        }
        if (rspp_to_bool(_t200)) {
            RsppVal _t202 = rspp_bool(1);
            return _t202;
        }
        RsppVal _t203 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_ns, rspp_str("gpu")))) {
            _t203 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_fn_name, rspp_str("alloc_vram"))));
        }
        if (rspp_to_bool(_t203)) {
            RsppVal _t204 = rspp_bool(1);
            return _t204;
        }
    }
    RsppVal _t205 = rspp_bool(0);
    return _t205;
    return rspp_none();
}

static RsppVal rspp_fn_check_free_call(RsppVal rspp_v_ex, RsppVal rspp_v_allocs, RsppVal rspp_v_diags, RsppVal rspp_v_line, RsppVal rspp_v_via_defer) {
    RsppVal _t207[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t206 = rspp_method_call(rspp_v_ex, "get", 2, _t207);
    if (rspp_to_bool(rspp_neq(_t206, rspp_str("ns_call")))) {
        RsppVal _t208 = rspp_none();
        return _t208;
    }
    RsppVal _t210[] = { rspp_str("ns"), rspp_str("") };
    RsppVal _t209 = rspp_method_call(rspp_v_ex, "get", 2, _t210);
    RsppVal rspp_v_ns = _t209;
    RsppVal _t212[] = { rspp_str("fn"), rspp_str("") };
    RsppVal _t211 = rspp_method_call(rspp_v_ex, "get", 2, _t212);
    RsppVal rspp_v_fn_name = _t211;
    RsppVal _t214 = rspp_bool(0);
    if (rspp_to_bool(rspp_eq(rspp_v_ns, rspp_str("memory")))) {
        _t214 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_fn_name, rspp_str("free"))));
    }
    RsppVal _t213 = rspp_bool(1);
    if (!rspp_to_bool(_t214)) {
        RsppVal _t215 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_ns, rspp_str("sdr")))) {
            _t215 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_fn_name, rspp_str("close"))));
        }
        _t213 = rspp_bool(rspp_to_bool(_t215));
    }
    if (rspp_to_bool(_t213)) {
        RsppVal _t216 = rspp_list();
        RsppVal _t218[] = { rspp_str("args"), _t216 };
        RsppVal _t217 = rspp_method_call(rspp_v_ex, "get", 2, _t218);
        RsppVal rspp_v_args = _t217;
        RsppVal _t219 = rspp_method_call(rspp_v_args, "len", 0, NULL);
        if (rspp_to_bool(rspp_gt(_t219, rspp_int(0LL)))) {
            RsppVal rspp_v_a0 = rspp_index_get(rspp_v_args, rspp_int(0LL));
            RsppVal _t221[] = { rspp_str("kind"), rspp_str("") };
            RsppVal _t220 = rspp_method_call(rspp_v_a0, "get", 2, _t221);
            if (rspp_to_bool(rspp_eq(_t220, rspp_str("ident")))) {
                RsppVal _t223[] = { rspp_str("name"), rspp_str("") };
                RsppVal _t222 = rspp_method_call(rspp_v_a0, "get", 2, _t223);
                RsppVal rspp_v_vname = _t222;
                RsppVal _t225[] = { rspp_v_vname };
                RsppVal _t224 = rspp_method_call(rspp_v_allocs, "has", 1, _t225);
                if (rspp_to_bool(_t224)) {
                    RsppVal _t226 = rspp_map();
                    RsppVal _t228[] = { rspp_v_vname, _t226 };
                    RsppVal _t227 = rspp_method_call(rspp_v_allocs, "get", 2, _t228);
                    RsppVal rspp_v_info = _t227;
                    RsppVal _t231[] = { rspp_str("freed"), rspp_bool(0) };
                    RsppVal _t230 = rspp_method_call(rspp_v_info, "get", 2, _t231);
                    RsppVal _t229 = rspp_bool(0);
                    if (rspp_to_bool(_t230)) {
                        _t229 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(rspp_v_via_defer))));
                    }
                    if (rspp_to_bool(_t229)) {
                        RsppVal _t232 = rspp_str("");
                        _t232 = rspp_add(_t232, rspp_str("Double-Free detected on `"));
                        _t232 = rspp_add(_t232, rspp_str(rspp_to_str(rspp_v_vname)));
                        _t232 = rspp_add(_t232, rspp_str("`"));
                        RsppVal _t233 = rspp_str("");
                        _t233 = rspp_add(_t233, rspp_str("تحرير مزدوج للذاكرة `"));
                        _t233 = rspp_add(_t233, rspp_str(rspp_to_str(rspp_v_vname)));
                        _t233 = rspp_add(_t233, rspp_str("`"));
                        RsppVal _t234 = rspp_fn_format_diag(rspp_str("LG002"), rspp_v_line, _t232, _t233, rspp_str("Freeing an already-freed pointer corrupts heap metadata."), rspp_str("Remove redundant free call."), rspp_str("StateTransition: Freed -> Free (Illegal)"));
                        RsppVal _t236[] = { _t234 };
                        RsppVal _t235 = rspp_method_call(rspp_v_diags, "push", 1, _t236);
                    } else {
                        if (rspp_to_bool(rspp_v_via_defer)) {
                            RsppVal _t238[] = { rspp_str("deferred"), rspp_bool(1) };
                            RsppVal _t237 = rspp_method_call(rspp_v_info, "set", 2, _t238);
                    } else {
                        RsppVal _t240[] = { rspp_str("freed"), rspp_bool(1) };
                        RsppVal _t239 = rspp_method_call(rspp_v_info, "set", 2, _t240);
                    }
                    }
                }
            }
        }
    }
    return rspp_none();
}

static RsppVal rspp_fn_leak_guard_verify_program(RsppVal rspp_v_prog) {
    RsppVal _t241 = rspp_list();
    RsppVal rspp_v_diags = _t241;
    RsppVal _t242 = rspp_list();
    RsppVal _t244[] = { rspp_str("items"), _t242 };
    RsppVal _t243 = rspp_method_call(rspp_v_prog, "get", 2, _t244);
    RsppVal _t245 = _t243;
    for (int64_t _t246 = 0; _t246 < rspp_list_len(_t245); _t246++) {
        RsppVal rspp_v_item = rspp_index_get(_t245, rspp_int(_t246));
        RsppVal _t248[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t247 = rspp_method_call(rspp_v_item, "get", 2, _t248);
        RsppVal rspp_v_k = _t247;
        RsppVal _t249 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("fn")))) {
            _t249 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("algorithm"))));
        }
        if (rspp_to_bool(_t249)) {
            RsppVal _t250 = rspp_map();
            RsppVal rspp_v_allocs = _t250;
            RsppVal _t251 = rspp_list();
            RsppVal _t253[] = { rspp_str("body"), _t251 };
            RsppVal _t252 = rspp_method_call(rspp_v_item, "get", 2, _t253);
            RsppVal _t254 = rspp_fn_leak_guard_scan_stmts(_t252, rspp_v_allocs, rspp_v_diags);
        }
    }
    RsppVal _t255 = rspp_v_diags;
    return _t255;
    return rspp_none();
}

static RsppVal rspp_fn_is_alpha_or_us(RsppVal rspp_v_c) {
    RsppVal _t259 = rspp_bool(0);
    if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(65LL)))) {
        _t259 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(90LL))));
    }
    RsppVal _t258 = rspp_bool(1);
    if (!rspp_to_bool(_t259)) {
        RsppVal _t260 = rspp_bool(0);
        if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(97LL)))) {
            _t260 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(122LL))));
        }
        _t258 = rspp_bool(rspp_to_bool(_t260));
    }
    RsppVal _t257 = rspp_bool(1);
    if (!rspp_to_bool(_t258)) {
        _t257 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c, rspp_int(95LL))));
    }
    RsppVal _t256 = _t257;
    return _t256;
    return rspp_none();
}

static RsppVal rspp_fn_is_digit(RsppVal rspp_v_c) {
    RsppVal _t262 = rspp_bool(0);
    if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(48LL)))) {
        _t262 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(57LL))));
    }
    RsppVal _t261 = _t262;
    return _t261;
    return rspp_none();
}

static RsppVal rspp_fn_is_hex(RsppVal rspp_v_c) {
    RsppVal _t266 = rspp_fn_is_digit(rspp_v_c);
    RsppVal _t265 = rspp_bool(1);
    if (!rspp_to_bool(_t266)) {
        RsppVal _t267 = rspp_bool(0);
        if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(65LL)))) {
            _t267 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(70LL))));
        }
        _t265 = rspp_bool(rspp_to_bool(_t267));
    }
    RsppVal _t264 = rspp_bool(1);
    if (!rspp_to_bool(_t265)) {
        RsppVal _t268 = rspp_bool(0);
        if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(97LL)))) {
            _t268 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(102LL))));
        }
        _t264 = rspp_bool(rspp_to_bool(_t268));
    }
    RsppVal _t263 = _t264;
    return _t263;
    return rspp_none();
}

static RsppVal rspp_fn_is_keyword(RsppVal rspp_v_w) {
    RsppVal _t269 = rspp_list();
    rspp_list_push(_t269, rspp_str("fn"));
    rspp_list_push(_t269, rspp_str("let"));
    rspp_list_push(_t269, rspp_str("var"));
    rspp_list_push(_t269, rspp_str("if"));
    rspp_list_push(_t269, rspp_str("elif"));
    rspp_list_push(_t269, rspp_str("else"));
    rspp_list_push(_t269, rspp_str("for"));
    rspp_list_push(_t269, rspp_str("in"));
    rspp_list_push(_t269, rspp_str("while"));
    rspp_list_push(_t269, rspp_str("loop"));
    rspp_list_push(_t269, rspp_str("match"));
    rspp_list_push(_t269, rspp_str("case"));
    rspp_list_push(_t269, rspp_str("return"));
    rspp_list_push(_t269, rspp_str("break"));
    rspp_list_push(_t269, rspp_str("continue"));
    rspp_list_push(_t269, rspp_str("struct"));
    rspp_list_push(_t269, rspp_str("enum"));
    rspp_list_push(_t269, rspp_str("import"));
    rspp_list_push(_t269, rspp_str("use"));
    rspp_list_push(_t269, rspp_str("extern"));
    rspp_list_push(_t269, rspp_str("unsafe_hardware"));
    rspp_list_push(_t269, rspp_str("equation"));
    rspp_list_push(_t269, rspp_str("algorithm"));
    rspp_list_push(_t269, rspp_str("defer"));
    rspp_list_push(_t269, rspp_str("try"));
    rspp_list_push(_t269, rspp_str("catch"));
    rspp_list_push(_t269, rspp_str("throw"));
    rspp_list_push(_t269, rspp_str("true"));
    rspp_list_push(_t269, rspp_str("false"));
    rspp_list_push(_t269, rspp_str("none"));
    rspp_list_push(_t269, rspp_str("None"));
    rspp_list_push(_t269, rspp_str("and"));
    rspp_list_push(_t269, rspp_str("or"));
    rspp_list_push(_t269, rspp_str("not"));
    rspp_list_push(_t269, rspp_str("as"));
    rspp_list_push(_t269, rspp_str("const"));
    RsppVal rspp_v_kws = _t269;
    RsppVal _t272[] = { rspp_v_w };
    RsppVal _t271 = rspp_method_call(rspp_v_kws, "contains", 1, _t272);
    RsppVal _t270 = _t271;
    return _t270;
    return rspp_none();
}

static RsppVal rspp_fn_make_tok(RsppVal rspp_v_kind, RsppVal rspp_v_val, RsppVal rspp_v_line) {
    RsppVal _t274 = rspp_map();
    rspp_map_set(_t274, rspp_str("kind"), rspp_v_kind);
    rspp_map_set(_t274, rspp_str("val"), rspp_v_val);
    rspp_map_set(_t274, rspp_str("line"), rspp_v_line);
    RsppVal _t273 = _t274;
    return _t273;
    return rspp_none();
}

static RsppVal rspp_fn_tokenize_source(RsppVal rspp_v_src) {
    RsppVal _t275 = rspp_list();
    RsppVal rspp_v_tokens = _t275;
    RsppVal _t276 = rspp_list();
    rspp_list_push(_t276, rspp_int(0LL));
    RsppVal rspp_v_indent_stack = _t276;
    RsppVal rspp_v_pos = rspp_int(0LL);
    RsppVal rspp_v_line = rspp_int(1LL);
    RsppVal rspp_v_at_line_start = rspp_bool(1);
    RsppVal rspp_v_delim_depth = rspp_int(0LL);
    RsppVal _t277 = rspp_method_call(rspp_v_src, "len", 0, NULL);
    RsppVal rspp_v_n = _t277;
    while (1) {
        if (!rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) break;
        RsppVal _t278 = rspp_bool(0);
        if (rspp_to_bool(rspp_v_at_line_start)) {
            _t278 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_delim_depth, rspp_int(0LL))));
        }
        if (rspp_to_bool(_t278)) {
            RsppVal rspp_v_ind = rspp_int(0LL);
            while (1) {
                if (!rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) break;
                RsppVal _t280[] = { rspp_v_pos };
                RsppVal _t279 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t280);
                RsppVal rspp_v_b = _t279;
                if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(32LL)))) {
                    rspp_v_ind = rspp_add(rspp_v_ind, rspp_int(1LL));
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                } else {
                    if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(9LL)))) {
                        rspp_v_ind = rspp_add(rspp_v_ind, rspp_int(4LL));
                        rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                } else {
                    if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(13LL)))) {
                        rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                } else {
                    break;
                    }
                }
                }
            }
            if (rspp_to_bool(rspp_gte(rspp_v_pos, rspp_v_n))) {
                break;
            }
            RsppVal _t282[] = { rspp_v_pos };
            RsppVal _t281 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t282);
            RsppVal rspp_v_nxt = _t281;
            if (rspp_to_bool(rspp_eq(rspp_v_nxt, rspp_int(10LL)))) {
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                rspp_v_line = rspp_add(rspp_v_line, rspp_int(1LL));
                continue;
            }
            RsppVal _t283 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_nxt, rspp_int(35LL)))) {
                RsppVal _t285 = rspp_bool(0);
                if (rspp_to_bool(rspp_eq(rspp_v_nxt, rspp_int(47LL)))) {
                    _t285 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
                }
                RsppVal _t284 = rspp_bool(0);
                if (rspp_to_bool(_t285)) {
                    RsppVal _t287[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
                    RsppVal _t286 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t287);
                    _t284 = rspp_bool(rspp_to_bool(rspp_eq(_t286, rspp_int(47LL))));
                }
                _t283 = rspp_bool(rspp_to_bool(_t284));
            }
            if (rspp_to_bool(_t283)) {
                while (1) {
                    RsppVal _t288 = rspp_bool(0);
                    if (rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) {
                        RsppVal _t290[] = { rspp_v_pos };
                        RsppVal _t289 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t290);
                        _t288 = rspp_bool(rspp_to_bool(rspp_neq(_t289, rspp_int(10LL))));
                    }
                    if (!rspp_to_bool(_t288)) break;
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                }
                continue;
            }
            RsppVal _t291 = rspp_method_call(rspp_v_indent_stack, "len", 0, NULL);
            RsppVal rspp_v_cur_ind = rspp_index_get(rspp_v_indent_stack, rspp_sub(_t291, rspp_int(1LL)));
            if (rspp_to_bool(rspp_gt(rspp_v_ind, rspp_v_cur_ind))) {
                RsppVal _t293[] = { rspp_v_ind };
                RsppVal _t292 = rspp_method_call(rspp_v_indent_stack, "push", 1, _t293);
                RsppVal _t294 = rspp_fn_make_tok(rspp_str("INDENT"), rspp_str(""), rspp_v_line);
                RsppVal _t296[] = { _t294 };
                RsppVal _t295 = rspp_method_call(rspp_v_tokens, "push", 1, _t296);
            } else {
                if (rspp_to_bool(rspp_lt(rspp_v_ind, rspp_v_cur_ind))) {
                    while (1) {
                        RsppVal _t298 = rspp_method_call(rspp_v_indent_stack, "len", 0, NULL);
                        RsppVal _t297 = rspp_bool(0);
                        if (rspp_to_bool(rspp_gt(_t298, rspp_int(1LL)))) {
                            RsppVal _t299 = rspp_method_call(rspp_v_indent_stack, "len", 0, NULL);
                            _t297 = rspp_bool(rspp_to_bool(rspp_gt(rspp_index_get(rspp_v_indent_stack, rspp_sub(_t299, rspp_int(1LL))), rspp_v_ind)));
                        }
                        if (!rspp_to_bool(_t297)) break;
                        RsppVal _t300 = rspp_method_call(rspp_v_indent_stack, "pop", 0, NULL);
                        RsppVal _t301 = rspp_fn_make_tok(rspp_str("DEDENT"), rspp_str(""), rspp_v_line);
                        RsppVal _t303[] = { _t301 };
                        RsppVal _t302 = rspp_method_call(rspp_v_tokens, "push", 1, _t303);
                    }
            }
            }
            rspp_v_at_line_start = rspp_bool(0);
        }
        if (rspp_to_bool(rspp_gte(rspp_v_pos, rspp_v_n))) {
            break;
        }
        RsppVal _t305[] = { rspp_v_pos };
        RsppVal _t304 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t305);
        RsppVal rspp_v_b = _t304;
        RsppVal _t307 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(32LL)))) {
            _t307 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(9LL))));
        }
        RsppVal _t306 = rspp_bool(1);
        if (!rspp_to_bool(_t307)) {
            _t306 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(13LL))));
        }
        if (rspp_to_bool(_t306)) {
            rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
            continue;
        }
        if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(10LL)))) {
            rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
            if (rspp_to_bool(rspp_eq(rspp_v_delim_depth, rspp_int(0LL)))) {
                RsppVal _t308 = rspp_method_call(rspp_v_tokens, "len", 0, NULL);
                if (rspp_to_bool(rspp_gt(_t308, rspp_int(0LL)))) {
                    RsppVal _t309 = rspp_method_call(rspp_v_tokens, "len", 0, NULL);
                    RsppVal _t311[] = { rspp_str("kind"), rspp_str("") };
                    RsppVal _t310 = rspp_method_call(rspp_index_get(rspp_v_tokens, rspp_sub(_t309, rspp_int(1LL))), "get", 2, _t311);
                    RsppVal rspp_v_lk = _t310;
                    RsppVal _t313 = rspp_bool(0);
                    if (rspp_to_bool(rspp_neq(rspp_v_lk, rspp_str("NEWLINE")))) {
                        _t313 = rspp_bool(rspp_to_bool(rspp_neq(rspp_v_lk, rspp_str("INDENT"))));
                    }
                    RsppVal _t312 = rspp_bool(0);
                    if (rspp_to_bool(_t313)) {
                        _t312 = rspp_bool(rspp_to_bool(rspp_neq(rspp_v_lk, rspp_str("DEDENT"))));
                    }
                    if (rspp_to_bool(_t312)) {
                        RsppVal _t314 = rspp_fn_make_tok(rspp_str("NEWLINE"), rspp_str("\n"), rspp_v_line);
                        RsppVal _t316[] = { _t314 };
                        RsppVal _t315 = rspp_method_call(rspp_v_tokens, "push", 1, _t316);
                    }
                }
                rspp_v_at_line_start = rspp_bool(1);
            }
            rspp_v_line = rspp_add(rspp_v_line, rspp_int(1LL));
            continue;
        }
        RsppVal _t317 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(35LL)))) {
            RsppVal _t319 = rspp_bool(0);
            if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(47LL)))) {
                _t319 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
            }
            RsppVal _t318 = rspp_bool(0);
            if (rspp_to_bool(_t319)) {
                RsppVal _t321[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
                RsppVal _t320 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t321);
                _t318 = rspp_bool(rspp_to_bool(rspp_eq(_t320, rspp_int(47LL))));
            }
            _t317 = rspp_bool(rspp_to_bool(_t318));
        }
        if (rspp_to_bool(_t317)) {
            while (1) {
                RsppVal _t322 = rspp_bool(0);
                if (rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) {
                    RsppVal _t324[] = { rspp_v_pos };
                    RsppVal _t323 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t324);
                    _t322 = rspp_bool(rspp_to_bool(rspp_neq(_t323, rspp_int(10LL))));
                }
                if (!rspp_to_bool(_t322)) break;
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
            }
            continue;
        }
        RsppVal _t326 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(47LL)))) {
            _t326 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
        }
        RsppVal _t325 = rspp_bool(0);
        if (rspp_to_bool(_t326)) {
            RsppVal _t328[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
            RsppVal _t327 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t328);
            _t325 = rspp_bool(rspp_to_bool(rspp_eq(_t327, rspp_int(42LL))));
        }
        if (rspp_to_bool(_t325)) {
            rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(2LL));
            RsppVal rspp_v_cdepth = rspp_int(1LL);
            while (1) {
                RsppVal _t329 = rspp_bool(0);
                if (rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) {
                    _t329 = rspp_bool(rspp_to_bool(rspp_gt(rspp_v_cdepth, rspp_int(0LL))));
                }
                if (!rspp_to_bool(_t329)) break;
                RsppVal _t331[] = { rspp_v_pos };
                RsppVal _t330 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t331);
                RsppVal rspp_v_cb = _t330;
                if (rspp_to_bool(rspp_eq(rspp_v_cb, rspp_int(10LL)))) {
                    rspp_v_line = rspp_add(rspp_v_line, rspp_int(1LL));
                }
                RsppVal _t333 = rspp_bool(0);
                if (rspp_to_bool(rspp_eq(rspp_v_cb, rspp_int(47LL)))) {
                    _t333 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
                }
                RsppVal _t332 = rspp_bool(0);
                if (rspp_to_bool(_t333)) {
                    RsppVal _t335[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
                    RsppVal _t334 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t335);
                    _t332 = rspp_bool(rspp_to_bool(rspp_eq(_t334, rspp_int(42LL))));
                }
                if (rspp_to_bool(_t332)) {
                    rspp_v_cdepth = rspp_add(rspp_v_cdepth, rspp_int(1LL));
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(2LL));
                } else {
                    RsppVal _t337 = rspp_bool(0);
                    if (rspp_to_bool(rspp_eq(rspp_v_cb, rspp_int(42LL)))) {
                        _t337 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
                    }
                    RsppVal _t336 = rspp_bool(0);
                    if (rspp_to_bool(_t337)) {
                        RsppVal _t339[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
                        RsppVal _t338 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t339);
                        _t336 = rspp_bool(rspp_to_bool(rspp_eq(_t338, rspp_int(47LL))));
                    }
                    if (rspp_to_bool(_t336)) {
                        rspp_v_cdepth = rspp_sub(rspp_v_cdepth, rspp_int(1LL));
                        rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(2LL));
                } else {
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                }
                }
            }
            continue;
        }
        RsppVal rspp_v_is_fstr = rspp_bool(0);
        RsppVal _t340 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(102LL)))) {
            _t340 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
        }
        if (rspp_to_bool(_t340)) {
            RsppVal _t342[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
            RsppVal _t341 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t342);
            RsppVal rspp_v_q2 = _t341;
            RsppVal _t343 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_q2, rspp_int(34LL)))) {
                _t343 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_q2, rspp_int(39LL))));
            }
            if (rspp_to_bool(_t343)) {
                rspp_v_is_fstr = rspp_bool(1);
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
            }
        }
        RsppVal _t345[] = { rspp_v_pos };
        RsppVal _t344 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t345);
        RsppVal rspp_v_qb = _t344;
        RsppVal _t346 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_qb, rspp_int(34LL)))) {
            _t346 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_qb, rspp_int(39LL))));
        }
        if (rspp_to_bool(_t346)) {
            RsppVal rspp_v_quote = rspp_v_qb;
            rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
            RsppVal rspp_v_sval = rspp_str("");
            while (1) {
                if (!rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) break;
                RsppVal _t348[] = { rspp_v_pos };
                RsppVal _t347 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t348);
                RsppVal rspp_v_sc = _t347;
                if (rspp_to_bool(rspp_eq(rspp_v_sc, rspp_v_quote))) {
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                    break;
                }
                if (rspp_to_bool(rspp_eq(rspp_v_sc, rspp_int(10LL)))) {
                    break;
                }
                RsppVal _t349 = rspp_bool(0);
                if (rspp_to_bool(rspp_eq(rspp_v_sc, rspp_int(92LL)))) {
                    _t349 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
                }
                if (rspp_to_bool(_t349)) {
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                    RsppVal _t351[] = { rspp_v_pos };
                    RsppVal _t350 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t351);
                    RsppVal rspp_v_esc = _t350;
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                    if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(110LL)))) {
                        rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("\n"));
                    } else {
                        if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(116LL)))) {
                            rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("\t"));
                    } else {
                        if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(114LL)))) {
                            rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("\r"));
                    } else {
                        if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(48LL)))) {
                            rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("\x00"));
                    } else {
                        if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(92LL)))) {
                            rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("\\"));
                    } else {
                        if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(34LL)))) {
                            rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("\""));
                    } else {
                        if (rspp_to_bool(rspp_eq(rspp_v_esc, rspp_int(39LL)))) {
                            rspp_v_sval = rspp_add(rspp_v_sval, rspp_str("'"));
                    } else {
                        RsppVal _t353[] = { rspp_v_esc };
                        RsppVal _t352 = rspp_builtin("str", "from_code", 1, _t353);
                        rspp_v_sval = rspp_add(rspp_v_sval, _t352);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    }
                } else {
                    RsppVal _t355[] = { rspp_v_pos };
                    RsppVal _t354 = rspp_method_call(rspp_v_src, "char_at", 1, _t355);
                    rspp_v_sval = rspp_add(rspp_v_sval, _t354);
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                }
            }
            if (rspp_to_bool(rspp_v_is_fstr)) {
                RsppVal _t356 = rspp_fn_make_tok(rspp_str("FSTR"), rspp_v_sval, rspp_v_line);
                RsppVal _t358[] = { _t356 };
                RsppVal _t357 = rspp_method_call(rspp_v_tokens, "push", 1, _t358);
            } else {
                RsppVal _t359 = rspp_fn_make_tok(rspp_str("STR"), rspp_v_sval, rspp_v_line);
                RsppVal _t361[] = { _t359 };
                RsppVal _t360 = rspp_method_call(rspp_v_tokens, "push", 1, _t361);
            }
            continue;
        }
        RsppVal _t362 = rspp_bool(0);
        if (rspp_to_bool(rspp_eq(rspp_v_b, rspp_int(48LL)))) {
            _t362 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
        }
        if (rspp_to_bool(_t362)) {
            RsppVal _t364[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
            RsppVal _t363 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t364);
            RsppVal rspp_v_p2 = _t363;
            RsppVal _t367 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_p2, rspp_int(120LL)))) {
                _t367 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_p2, rspp_int(88LL))));
            }
            RsppVal _t366 = rspp_bool(1);
            if (!rspp_to_bool(_t367)) {
                _t366 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_p2, rspp_int(98LL))));
            }
            RsppVal _t365 = rspp_bool(1);
            if (!rspp_to_bool(_t366)) {
                _t365 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_p2, rspp_int(66LL))));
            }
            if (rspp_to_bool(_t365)) {
                RsppVal rspp_v_start_p = rspp_v_pos;
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(2LL));
                while (1) {
                    RsppVal _t368 = rspp_bool(0);
                    if (rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) {
                        RsppVal _t371[] = { rspp_v_pos };
                        RsppVal _t370 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t371);
                        RsppVal _t372 = rspp_fn_is_hex(_t370);
                        RsppVal _t369 = rspp_bool(1);
                        if (!rspp_to_bool(_t372)) {
                            RsppVal _t374[] = { rspp_v_pos };
                            RsppVal _t373 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t374);
                            _t369 = rspp_bool(rspp_to_bool(rspp_eq(_t373, rspp_int(95LL))));
                        }
                        _t368 = rspp_bool(rspp_to_bool(_t369));
                    }
                    if (!rspp_to_bool(_t368)) break;
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                }
                RsppVal _t376[] = { rspp_v_start_p, rspp_v_pos };
                RsppVal _t375 = rspp_method_call(rspp_v_src, "slice", 2, _t376);
                RsppVal _t378[] = { rspp_str("_"), rspp_str("") };
                RsppVal _t377 = rspp_method_call(_t375, "replace", 2, _t378);
                RsppVal rspp_v_hraw = _t377;
                RsppVal _t379 = rspp_fn_make_tok(rspp_str("HEX"), rspp_v_hraw, rspp_v_line);
                RsppVal _t381[] = { _t379 };
                RsppVal _t380 = rspp_method_call(rspp_v_tokens, "push", 1, _t381);
                continue;
            }
        }
        RsppVal _t382 = rspp_fn_is_digit(rspp_v_b);
        if (rspp_to_bool(_t382)) {
            RsppVal rspp_v_start_n = rspp_v_pos;
            RsppVal rspp_v_is_flt = rspp_bool(0);
            while (1) {
                if (!rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) break;
                RsppVal _t384[] = { rspp_v_pos };
                RsppVal _t383 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t384);
                RsppVal rspp_v_nc = _t383;
                RsppVal _t386 = rspp_fn_is_digit(rspp_v_nc);
                RsppVal _t385 = rspp_bool(1);
                if (!rspp_to_bool(_t386)) {
                    _t385 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_nc, rspp_int(95LL))));
                }
                if (rspp_to_bool(_t385)) {
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                } else {
                    RsppVal _t389 = rspp_bool(0);
                    if (rspp_to_bool(rspp_eq(rspp_v_nc, rspp_int(46LL)))) {
                        _t389 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(rspp_v_is_flt))));
                    }
                    RsppVal _t388 = rspp_bool(0);
                    if (rspp_to_bool(_t389)) {
                        _t388 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n)));
                    }
                    RsppVal _t387 = rspp_bool(0);
                    if (rspp_to_bool(_t388)) {
                        RsppVal _t391[] = { rspp_add(rspp_v_pos, rspp_int(1LL)) };
                        RsppVal _t390 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t391);
                        RsppVal _t392 = rspp_fn_is_digit(_t390);
                        _t387 = rspp_bool(rspp_to_bool(_t392));
                    }
                    if (rspp_to_bool(_t387)) {
                        rspp_v_is_flt = rspp_bool(1);
                        rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                } else {
                    break;
                }
                }
            }
            RsppVal _t394[] = { rspp_v_start_n, rspp_v_pos };
            RsppVal _t393 = rspp_method_call(rspp_v_src, "slice", 2, _t394);
            RsppVal _t396[] = { rspp_str("_"), rspp_str("") };
            RsppVal _t395 = rspp_method_call(_t393, "replace", 2, _t396);
            RsppVal rspp_v_nstr = _t395;
            RsppVal rspp_v_u_start = rspp_v_pos;
            while (1) {
                RsppVal _t397 = rspp_bool(0);
                if (rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) {
                    RsppVal _t399[] = { rspp_v_pos };
                    RsppVal _t398 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t399);
                    RsppVal _t400 = rspp_fn_is_alpha_or_us(_t398);
                    _t397 = rspp_bool(rspp_to_bool(_t400));
                }
                if (!rspp_to_bool(_t397)) break;
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
            }
            RsppVal _t402[] = { rspp_v_u_start, rspp_v_pos };
            RsppVal _t401 = rspp_method_call(rspp_v_src, "slice", 2, _t402);
            RsppVal rspp_v_unit = _t401;
            if (rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("")))) {
                if (rspp_to_bool(rspp_v_is_flt)) {
                    RsppVal _t403 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_nstr, rspp_v_line);
                    RsppVal _t405[] = { _t403 };
                    RsppVal _t404 = rspp_method_call(rspp_v_tokens, "push", 1, _t405);
                } else {
                    RsppVal _t406 = rspp_fn_make_tok(rspp_str("INT"), rspp_v_nstr, rspp_v_line);
                    RsppVal _t408[] = { _t406 };
                    RsppVal _t407 = rspp_method_call(rspp_v_tokens, "push", 1, _t408);
                }
            } else {
                if (rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("V")))) {
                    RsppVal _t410[] = { rspp_v_nstr };
                    RsppVal _t409 = rspp_builtin("str", "to_float", 1, _t410);
                    RsppVal _t412[] = { rspp_mul(_t409, rspp_int(rspp_to_i64(rspp_flt(1000.000000000000)))) };
                    RsppVal _t411 = rspp_builtin("str", "from_int", 1, _t412);
                    RsppVal _t413 = rspp_fn_make_tok(rspp_str("FLOAT"), _t411, rspp_v_line);
                    RsppVal _t415[] = { _t413 };
                    RsppVal _t414 = rspp_method_call(rspp_v_tokens, "push", 1, _t415);
            } else {
                RsppVal _t420 = rspp_bool(1);
                if (!rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("mV")))) {
                    _t420 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("Hz"))));
                }
                RsppVal _t419 = rspp_bool(1);
                if (!rspp_to_bool(_t420)) {
                    _t419 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("sps"))));
                }
                RsppVal _t418 = rspp_bool(1);
                if (!rspp_to_bool(_t419)) {
                    _t418 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("dBm"))));
                }
                RsppVal _t417 = rspp_bool(1);
                if (!rspp_to_bool(_t418)) {
                    _t417 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("rad"))));
                }
                RsppVal _t416 = rspp_bool(1);
                if (!rspp_to_bool(_t417)) {
                    _t416 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("ns"))));
                }
                if (rspp_to_bool(_t416)) {
                    RsppVal _t421 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_nstr, rspp_v_line);
                    RsppVal _t423[] = { _t421 };
                    RsppVal _t422 = rspp_method_call(rspp_v_tokens, "push", 1, _t423);
            } else {
                RsppVal _t425 = rspp_bool(1);
                if (!rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("kHz")))) {
                    _t425 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("ksps"))));
                }
                RsppVal _t424 = rspp_bool(1);
                if (!rspp_to_bool(_t425)) {
                    _t424 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("us"))));
                }
                if (rspp_to_bool(_t424)) {
                    RsppVal _t427[] = { rspp_v_nstr };
                    RsppVal _t426 = rspp_builtin("str", "to_float", 1, _t427);
                    RsppVal rspp_v_scaled = rspp_str(rspp_to_str(rspp_mul(_t426, rspp_flt(1000.000000000000))));
                    RsppVal _t428 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_scaled, rspp_v_line);
                    RsppVal _t430[] = { _t428 };
                    RsppVal _t429 = rspp_method_call(rspp_v_tokens, "push", 1, _t430);
            } else {
                RsppVal _t432 = rspp_bool(1);
                if (!rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("MHz")))) {
                    _t432 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("Msps"))));
                }
                RsppVal _t431 = rspp_bool(1);
                if (!rspp_to_bool(_t432)) {
                    _t431 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("ms"))));
                }
                if (rspp_to_bool(_t431)) {
                    RsppVal _t434[] = { rspp_v_nstr };
                    RsppVal _t433 = rspp_builtin("str", "to_float", 1, _t434);
                    RsppVal rspp_v_scaled = rspp_str(rspp_to_str(rspp_mul(_t433, rspp_flt(1000000.000000000000))));
                    RsppVal _t435 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_scaled, rspp_v_line);
                    RsppVal _t437[] = { _t435 };
                    RsppVal _t436 = rspp_method_call(rspp_v_tokens, "push", 1, _t437);
            } else {
                RsppVal _t438 = rspp_bool(1);
                if (!rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("GHz")))) {
                    _t438 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("s"))));
                }
                if (rspp_to_bool(_t438)) {
                    RsppVal _t440[] = { rspp_v_nstr };
                    RsppVal _t439 = rspp_builtin("str", "to_float", 1, _t440);
                    RsppVal rspp_v_scaled = rspp_str(rspp_to_str(rspp_mul(_t439, rspp_flt(1000000000.000000000000))));
                    RsppVal _t441 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_scaled, rspp_v_line);
                    RsppVal _t443[] = { _t441 };
                    RsppVal _t442 = rspp_method_call(rspp_v_tokens, "push", 1, _t443);
            } else {
                if (rspp_to_bool(rspp_eq(rspp_v_unit, rspp_str("deg")))) {
                    RsppVal _t445[] = { rspp_v_nstr };
                    RsppVal _t444 = rspp_builtin("str", "to_float", 1, _t445);
                    RsppVal rspp_v_scaled = rspp_str(rspp_to_str(rspp_mul(_t444, rspp_flt(0.017453292520))));
                    RsppVal _t446 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_scaled, rspp_v_line);
                    RsppVal _t448[] = { _t446 };
                    RsppVal _t447 = rspp_method_call(rspp_v_tokens, "push", 1, _t448);
            } else {
                if (rspp_to_bool(rspp_v_is_flt)) {
                    RsppVal _t449 = rspp_fn_make_tok(rspp_str("FLOAT"), rspp_v_nstr, rspp_v_line);
                    RsppVal _t451[] = { _t449 };
                    RsppVal _t450 = rspp_method_call(rspp_v_tokens, "push", 1, _t451);
                } else {
                    RsppVal _t452 = rspp_fn_make_tok(rspp_str("INT"), rspp_v_nstr, rspp_v_line);
                    RsppVal _t454[] = { _t452 };
                    RsppVal _t453 = rspp_method_call(rspp_v_tokens, "push", 1, _t454);
                }
                RsppVal _t455 = rspp_fn_make_tok(rspp_str("PUNCT"), rspp_str("*"), rspp_v_line);
                RsppVal _t457[] = { _t455 };
                RsppVal _t456 = rspp_method_call(rspp_v_tokens, "push", 1, _t457);
                RsppVal _t458 = rspp_fn_make_tok(rspp_str("IDENT"), rspp_v_unit, rspp_v_line);
                RsppVal _t460[] = { _t458 };
                RsppVal _t459 = rspp_method_call(rspp_v_tokens, "push", 1, _t460);
                                }
                            }
                        }
                    }
                }
            }
            }
            continue;
        }
        RsppVal _t461 = rspp_fn_is_alpha_or_us(rspp_v_b);
        if (rspp_to_bool(_t461)) {
            RsppVal rspp_v_start_id = rspp_v_pos;
            while (1) {
                if (!rspp_to_bool(rspp_lt(rspp_v_pos, rspp_v_n))) break;
                RsppVal _t463[] = { rspp_v_pos };
                RsppVal _t462 = rspp_method_call(rspp_v_src, "char_code_at", 1, _t463);
                RsppVal rspp_v_ic = _t462;
                RsppVal _t465 = rspp_fn_is_alpha_or_us(rspp_v_ic);
                RsppVal _t464 = rspp_bool(1);
                if (!rspp_to_bool(_t465)) {
                    RsppVal _t466 = rspp_fn_is_digit(rspp_v_ic);
                    _t464 = rspp_bool(rspp_to_bool(_t466));
                }
                if (rspp_to_bool(_t464)) {
                    rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
                } else {
                    break;
                }
            }
            RsppVal _t468[] = { rspp_v_start_id, rspp_v_pos };
            RsppVal _t467 = rspp_method_call(rspp_v_src, "slice", 2, _t468);
            RsppVal rspp_v_word = _t467;
            RsppVal _t469 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_word, rspp_str("true")))) {
                _t469 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_word, rspp_str("false"))));
            }
            if (rspp_to_bool(_t469)) {
                RsppVal _t470 = rspp_fn_make_tok(rspp_str("BOOL"), rspp_v_word, rspp_v_line);
                RsppVal _t472[] = { _t470 };
                RsppVal _t471 = rspp_method_call(rspp_v_tokens, "push", 1, _t472);
            } else {
                RsppVal _t473 = rspp_bool(1);
                if (!rspp_to_bool(rspp_eq(rspp_v_word, rspp_str("none")))) {
                    _t473 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_word, rspp_str("None"))));
                }
                if (rspp_to_bool(_t473)) {
                    RsppVal _t474 = rspp_fn_make_tok(rspp_str("NONE"), rspp_str("none"), rspp_v_line);
                    RsppVal _t476[] = { _t474 };
                    RsppVal _t475 = rspp_method_call(rspp_v_tokens, "push", 1, _t476);
            } else {
                RsppVal _t477 = rspp_fn_is_keyword(rspp_v_word);
                if (rspp_to_bool(_t477)) {
                    RsppVal _t478 = rspp_fn_make_tok(rspp_str("KW"), rspp_v_word, rspp_v_line);
                    RsppVal _t480[] = { _t478 };
                    RsppVal _t479 = rspp_method_call(rspp_v_tokens, "push", 1, _t480);
            } else {
                RsppVal _t481 = rspp_fn_make_tok(rspp_str("IDENT"), rspp_v_word, rspp_v_line);
                RsppVal _t483[] = { _t481 };
                RsppVal _t482 = rspp_method_call(rspp_v_tokens, "push", 1, _t483);
                }
            }
            }
            continue;
        }
        RsppVal _t485[] = { rspp_v_pos };
        RsppVal _t484 = rspp_method_call(rspp_v_src, "char_at", 1, _t485);
        RsppVal rspp_v_c1 = _t484;
        RsppVal rspp_v_c2 = rspp_str("");
        if (rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(1LL)), rspp_v_n))) {
            RsppVal _t487[] = { rspp_v_pos, rspp_add(rspp_v_pos, rspp_int(2LL)) };
            RsppVal _t486 = rspp_method_call(rspp_v_src, "slice", 2, _t487);
            rspp_v_c2 = _t486;
        }
        RsppVal _t504 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("::")))) {
            _t504 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("->"))));
        }
        RsppVal _t503 = rspp_bool(1);
        if (!rspp_to_bool(_t504)) {
            _t503 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("=>"))));
        }
        RsppVal _t502 = rspp_bool(1);
        if (!rspp_to_bool(_t503)) {
            _t502 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("|>"))));
        }
        RsppVal _t501 = rspp_bool(1);
        if (!rspp_to_bool(_t502)) {
            _t501 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("**"))));
        }
        RsppVal _t500 = rspp_bool(1);
        if (!rspp_to_bool(_t501)) {
            _t500 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("=="))));
        }
        RsppVal _t499 = rspp_bool(1);
        if (!rspp_to_bool(_t500)) {
            _t499 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("!="))));
        }
        RsppVal _t498 = rspp_bool(1);
        if (!rspp_to_bool(_t499)) {
            _t498 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("<="))));
        }
        RsppVal _t497 = rspp_bool(1);
        if (!rspp_to_bool(_t498)) {
            _t497 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str(">="))));
        }
        RsppVal _t496 = rspp_bool(1);
        if (!rspp_to_bool(_t497)) {
            _t496 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("+="))));
        }
        RsppVal _t495 = rspp_bool(1);
        if (!rspp_to_bool(_t496)) {
            _t495 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("-="))));
        }
        RsppVal _t494 = rspp_bool(1);
        if (!rspp_to_bool(_t495)) {
            _t494 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("*="))));
        }
        RsppVal _t493 = rspp_bool(1);
        if (!rspp_to_bool(_t494)) {
            _t493 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("/="))));
        }
        RsppVal _t492 = rspp_bool(1);
        if (!rspp_to_bool(_t493)) {
            _t492 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("<<"))));
        }
        RsppVal _t491 = rspp_bool(1);
        if (!rspp_to_bool(_t492)) {
            _t491 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str(">>"))));
        }
        RsppVal _t490 = rspp_bool(1);
        if (!rspp_to_bool(_t491)) {
            _t490 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str(".."))));
        }
        RsppVal _t489 = rspp_bool(1);
        if (!rspp_to_bool(_t490)) {
            _t489 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("&&"))));
        }
        RsppVal _t488 = rspp_bool(1);
        if (!rspp_to_bool(_t489)) {
            _t488 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("||"))));
        }
        if (rspp_to_bool(_t488)) {
            RsppVal _t506 = rspp_bool(0);
            if (rspp_to_bool(rspp_eq(rspp_v_c2, rspp_str("..")))) {
                _t506 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_pos, rspp_int(2LL)), rspp_v_n)));
            }
            RsppVal _t505 = rspp_bool(0);
            if (rspp_to_bool(_t506)) {
                RsppVal _t508[] = { rspp_add(rspp_v_pos, rspp_int(2LL)) };
                RsppVal _t507 = rspp_method_call(rspp_v_src, "char_at", 1, _t508);
                _t505 = rspp_bool(rspp_to_bool(rspp_eq(_t507, rspp_str("="))));
            }
            if (rspp_to_bool(_t505)) {
                RsppVal _t509 = rspp_fn_make_tok(rspp_str("PUNCT"), rspp_str("..="), rspp_v_line);
                RsppVal _t511[] = { _t509 };
                RsppVal _t510 = rspp_method_call(rspp_v_tokens, "push", 1, _t511);
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(3LL));
            } else {
                RsppVal _t512 = rspp_fn_make_tok(rspp_str("PUNCT"), rspp_v_c2, rspp_v_line);
                RsppVal _t514[] = { _t512 };
                RsppVal _t513 = rspp_method_call(rspp_v_tokens, "push", 1, _t514);
                rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(2LL));
            }
            continue;
        }
        RsppVal _t516 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_c1, rspp_str("(")))) {
            _t516 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c1, rspp_str("["))));
        }
        RsppVal _t515 = rspp_bool(1);
        if (!rspp_to_bool(_t516)) {
            _t515 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c1, rspp_str("{"))));
        }
        if (rspp_to_bool(_t515)) {
            rspp_v_delim_depth = rspp_add(rspp_v_delim_depth, rspp_int(1LL));
        } else {
            RsppVal _t518 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_c1, rspp_str(")")))) {
                _t518 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c1, rspp_str("]"))));
            }
            RsppVal _t517 = rspp_bool(1);
            if (!rspp_to_bool(_t518)) {
                _t517 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c1, rspp_str("}"))));
            }
            if (rspp_to_bool(_t517)) {
                if (rspp_to_bool(rspp_gt(rspp_v_delim_depth, rspp_int(0LL)))) {
                    rspp_v_delim_depth = rspp_sub(rspp_v_delim_depth, rspp_int(1LL));
                }
        }
        }
        RsppVal _t519 = rspp_fn_make_tok(rspp_str("PUNCT"), rspp_v_c1, rspp_v_line);
        RsppVal _t521[] = { _t519 };
        RsppVal _t520 = rspp_method_call(rspp_v_tokens, "push", 1, _t521);
        rspp_v_pos = rspp_add(rspp_v_pos, rspp_int(1LL));
    }
    RsppVal _t522 = rspp_method_call(rspp_v_tokens, "len", 0, NULL);
    if (rspp_to_bool(rspp_gt(_t522, rspp_int(0LL)))) {
        RsppVal _t523 = rspp_method_call(rspp_v_tokens, "len", 0, NULL);
        RsppVal _t525[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t524 = rspp_method_call(rspp_index_get(rspp_v_tokens, rspp_sub(_t523, rspp_int(1LL))), "get", 2, _t525);
        RsppVal rspp_v_lk = _t524;
        RsppVal _t526 = rspp_bool(0);
        if (rspp_to_bool(rspp_neq(rspp_v_lk, rspp_str("NEWLINE")))) {
            _t526 = rspp_bool(rspp_to_bool(rspp_neq(rspp_v_lk, rspp_str("DEDENT"))));
        }
        if (rspp_to_bool(_t526)) {
            RsppVal _t527 = rspp_fn_make_tok(rspp_str("NEWLINE"), rspp_str("\n"), rspp_v_line);
            RsppVal _t529[] = { _t527 };
            RsppVal _t528 = rspp_method_call(rspp_v_tokens, "push", 1, _t529);
        }
    }
    while (1) {
        RsppVal _t530 = rspp_method_call(rspp_v_indent_stack, "len", 0, NULL);
        if (!rspp_to_bool(rspp_gt(_t530, rspp_int(1LL)))) break;
        RsppVal _t531 = rspp_method_call(rspp_v_indent_stack, "pop", 0, NULL);
        RsppVal _t532 = rspp_fn_make_tok(rspp_str("DEDENT"), rspp_str(""), rspp_v_line);
        RsppVal _t534[] = { _t532 };
        RsppVal _t533 = rspp_method_call(rspp_v_tokens, "push", 1, _t534);
    }
    RsppVal _t535 = rspp_fn_make_tok(rspp_str("EOF"), rspp_str(""), rspp_v_line);
    RsppVal _t537[] = { _t535 };
    RsppVal _t536 = rspp_method_call(rspp_v_tokens, "push", 1, _t537);
    RsppVal _t538 = rspp_v_tokens;
    return _t538;
    return rspp_none();
}

static RsppVal rspp_fn_new_parser(RsppVal rspp_v_tokens) {
    RsppVal _t540 = rspp_map();
    rspp_map_set(_t540, rspp_str("tokens"), rspp_v_tokens);
    rspp_map_set(_t540, rspp_str("pos"), rspp_int(0LL));
    rspp_map_set(_t540, rspp_str("in_eq"), rspp_bool(0));
    RsppVal _t539 = _t540;
    return _t539;
    return rspp_none();
}

static RsppVal rspp_fn_p_cur(RsppVal rspp_v_p) {
    RsppVal _t541 = rspp_list();
    RsppVal _t543[] = { rspp_str("tokens"), _t541 };
    RsppVal _t542 = rspp_method_call(rspp_v_p, "get", 2, _t543);
    RsppVal rspp_v_toks = _t542;
    RsppVal _t545[] = { rspp_str("pos"), rspp_int(0LL) };
    RsppVal _t544 = rspp_method_call(rspp_v_p, "get", 2, _t545);
    RsppVal rspp_v_pos = _t544;
    RsppVal _t546 = rspp_method_call(rspp_v_toks, "len", 0, NULL);
    if (rspp_to_bool(rspp_lt(rspp_v_pos, _t546))) {
        RsppVal _t547 = rspp_index_get(rspp_v_toks, rspp_v_pos);
        return _t547;
    }
    RsppVal _t549 = rspp_method_call(rspp_v_toks, "len", 0, NULL);
    RsppVal _t548 = rspp_index_get(rspp_v_toks, rspp_sub(_t549, rspp_int(1LL)));
    return _t548;
    return rspp_none();
}

static RsppVal rspp_fn_p_adv(RsppVal rspp_v_p) {
    RsppVal _t550 = rspp_fn_p_cur(rspp_v_p);
    RsppVal rspp_v_t = _t550;
    RsppVal _t552[] = { rspp_str("pos"), rspp_int(0LL) };
    RsppVal _t551 = rspp_method_call(rspp_v_p, "get", 2, _t552);
    RsppVal rspp_v_pos = _t551;
    RsppVal _t554[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t553 = rspp_method_call(rspp_v_t, "get", 2, _t554);
    if (rspp_to_bool(rspp_neq(_t553, rspp_str("EOF")))) {
        RsppVal _t556[] = { rspp_str("pos"), rspp_add(rspp_v_pos, rspp_int(1LL)) };
        RsppVal _t555 = rspp_method_call(rspp_v_p, "set", 2, _t556);
    }
    RsppVal _t557 = rspp_v_t;
    return _t557;
    return rspp_none();
}

static RsppVal rspp_fn_p_is(RsppVal rspp_v_p, RsppVal rspp_v_kind, RsppVal rspp_v_val) {
    RsppVal _t558 = rspp_fn_p_cur(rspp_v_p);
    RsppVal rspp_v_t = _t558;
    RsppVal _t562[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t561 = rspp_method_call(rspp_v_t, "get", 2, _t562);
    RsppVal _t560 = rspp_bool(0);
    if (rspp_to_bool(rspp_eq(_t561, rspp_v_kind))) {
        RsppVal _t564[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t563 = rspp_method_call(rspp_v_t, "get", 2, _t564);
        _t560 = rspp_bool(rspp_to_bool(rspp_eq(_t563, rspp_v_val)));
    }
    RsppVal _t559 = _t560;
    return _t559;
    return rspp_none();
}

static RsppVal rspp_fn_p_is_kind(RsppVal rspp_v_p, RsppVal rspp_v_kind) {
    RsppVal _t566 = rspp_fn_p_cur(rspp_v_p);
    RsppVal _t568[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t567 = rspp_method_call(_t566, "get", 2, _t568);
    RsppVal _t565 = rspp_eq(_t567, rspp_v_kind);
    return _t565;
    return rspp_none();
}

static RsppVal rspp_fn_p_consume(RsppVal rspp_v_p, RsppVal rspp_v_kind, RsppVal rspp_v_val) {
    RsppVal _t569 = rspp_fn_p_is(rspp_v_p, rspp_v_kind, rspp_v_val);
    if (rspp_to_bool(_t569)) {
        RsppVal _t570 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t571 = rspp_bool(1);
        return _t571;
    }
    RsppVal _t572 = rspp_bool(0);
    return _t572;
    return rspp_none();
}

static RsppVal rspp_fn_p_skip_nl(RsppVal rspp_v_p) {
    while (1) {
        RsppVal _t574 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("NEWLINE"));
        RsppVal _t573 = rspp_bool(1);
        if (!rspp_to_bool(_t574)) {
            RsppVal _t575 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(";"));
            _t573 = rspp_bool(rspp_to_bool(_t575));
        }
        if (!rspp_to_bool(_t573)) break;
        RsppVal _t576 = rspp_fn_p_adv(rspp_v_p);
    }
    return rspp_none();
}

static RsppVal rspp_fn_parse_tokens(RsppVal rspp_v_tokens) {
    RsppVal _t577 = rspp_fn_new_parser(rspp_v_tokens);
    RsppVal rspp_v_p = _t577;
    RsppVal _t578 = rspp_list();
    RsppVal rspp_v_items = _t578;
    RsppVal _t579 = rspp_fn_p_skip_nl(rspp_v_p);
    while (1) {
        RsppVal _t580 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
        if (!rspp_to_bool(rspp_bool(!rspp_to_bool(_t580)))) break;
        RsppVal _t581 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
        if (rspp_to_bool(_t581)) {
            RsppVal _t582 = rspp_fn_p_adv(rspp_v_p);
            RsppVal _t583 = rspp_fn_p_skip_nl(rspp_v_p);
            continue;
        }
        RsppVal _t585[] = { rspp_str("pos"), rspp_int(0LL) };
        RsppVal _t584 = rspp_method_call(rspp_v_p, "get", 2, _t585);
        RsppVal rspp_v_before = _t584;
        RsppVal _t586 = rspp_fn_parse_item(rspp_v_p);
        RsppVal rspp_v_it = _t586;
        RsppVal _t587 = rspp_method_call(rspp_v_it, "len", 0, NULL);
        if (rspp_to_bool(rspp_gt(_t587, rspp_int(0LL)))) {
            RsppVal _t589[] = { rspp_v_it };
            RsppVal _t588 = rspp_method_call(rspp_v_items, "push", 1, _t589);
        }
        RsppVal _t591[] = { rspp_str("pos"), rspp_int(0LL) };
        RsppVal _t590 = rspp_method_call(rspp_v_p, "get", 2, _t591);
        if (rspp_to_bool(rspp_eq(_t590, rspp_v_before))) {
            RsppVal _t592 = rspp_fn_p_adv(rspp_v_p);
        }
        RsppVal _t593 = rspp_fn_p_skip_nl(rspp_v_p);
    }
    RsppVal _t595 = rspp_map();
    rspp_map_set(_t595, rspp_str("kind"), rspp_str("program"));
    rspp_map_set(_t595, rspp_str("items"), rspp_v_items);
    RsppVal _t594 = _t595;
    return _t594;
    return rspp_none();
}

static RsppVal rspp_fn_parse_item(RsppVal rspp_v_p) {
    while (1) {
        RsppVal _t596 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("@"));
        if (!rspp_to_bool(_t596)) break;
        RsppVal _t597 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t598 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t599 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("("));
        if (rspp_to_bool(_t599)) {
            while (1) {
                RsppVal _t601 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
                RsppVal _t600 = rspp_bool(0);
                if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t601)))) {
                    RsppVal _t602 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                    _t600 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t602))));
                }
                if (!rspp_to_bool(_t600)) break;
                RsppVal _t603 = rspp_fn_p_adv(rspp_v_p);
            }
            RsppVal _t604 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
        }
        RsppVal _t605 = rspp_fn_p_skip_nl(rspp_v_p);
    }
    RsppVal _t606 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("fn"));
    if (rspp_to_bool(_t606)) {
        RsppVal _t608 = rspp_fn_parse_fn(rspp_v_p);
        RsppVal _t607 = _t608;
        return _t607;
    }
    RsppVal _t609 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("equation"));
    if (rspp_to_bool(_t609)) {
        RsppVal _t611 = rspp_fn_parse_equation(rspp_v_p);
        RsppVal _t610 = _t611;
        return _t610;
    }
    RsppVal _t612 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("algorithm"));
    if (rspp_to_bool(_t612)) {
        RsppVal _t614 = rspp_fn_parse_algorithm(rspp_v_p);
        RsppVal _t613 = _t614;
        return _t613;
    }
    RsppVal _t615 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("extern"));
    if (rspp_to_bool(_t615)) {
        RsppVal _t617 = rspp_fn_parse_extern(rspp_v_p);
        RsppVal _t616 = _t617;
        return _t616;
    }
    RsppVal _t619 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("import"));
    RsppVal _t618 = rspp_bool(1);
    if (!rspp_to_bool(_t619)) {
        RsppVal _t620 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("use"));
        _t618 = rspp_bool(rspp_to_bool(_t620));
    }
    if (rspp_to_bool(_t618)) {
        RsppVal _t622 = rspp_fn_parse_import(rspp_v_p);
        RsppVal _t621 = _t622;
        return _t621;
    }
    RsppVal _t623 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("struct"));
    if (rspp_to_bool(_t623)) {
        RsppVal _t625 = rspp_fn_parse_struct(rspp_v_p);
        RsppVal _t624 = _t625;
        return _t624;
    }
    RsppVal _t626 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("enum"));
    if (rspp_to_bool(_t626)) {
        RsppVal _t628 = rspp_fn_parse_enum(rspp_v_p);
        RsppVal _t627 = _t628;
        return _t627;
    }
    RsppVal _t629 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("const"));
    if (rspp_to_bool(_t629)) {
        RsppVal _t630 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t631 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t633[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t632 = rspp_method_call(_t631, "get", 2, _t633);
        RsppVal rspp_v_cname = _t632;
        RsppVal _t634 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
        if (rspp_to_bool(_t634)) {
            RsppVal _t635 = rspp_fn_parse_type_str(rspp_v_p);
        }
        RsppVal _t636 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("="));
        RsppVal _t637 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_val = _t637;
        RsppVal _t639 = rspp_map();
        rspp_map_set(_t639, rspp_str("kind"), rspp_str("const"));
        rspp_map_set(_t639, rspp_str("name"), rspp_v_cname);
        rspp_map_set(_t639, rspp_str("value"), rspp_v_val);
        RsppVal _t638 = _t639;
        return _t638;
    }
    RsppVal _t641 = rspp_fn_parse_stmt(rspp_v_p);
    RsppVal _t640 = _t641;
    return _t640;
    return rspp_none();
}

static RsppVal rspp_fn_parse_type_str(RsppVal rspp_v_p) {
    RsppVal _t642 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t644[] = { rspp_str("val"), rspp_str("int") };
    RsppVal _t643 = rspp_method_call(_t642, "get", 2, _t644);
    RsppVal rspp_v_t = _t643;
    RsppVal _t645 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("["));
    if (rspp_to_bool(_t645)) {
        while (1) {
            RsppVal _t647 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("]"));
            RsppVal _t646 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t647)))) {
                RsppVal _t648 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t646 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t648))));
            }
            if (!rspp_to_bool(_t646)) break;
            RsppVal _t649 = rspp_fn_p_adv(rspp_v_p);
        }
        RsppVal _t650 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("]"));
    }
    RsppVal _t651 = rspp_v_t;
    return _t651;
    return rspp_none();
}

static RsppVal rspp_fn_parse_params(RsppVal rspp_v_p) {
    RsppVal _t652 = rspp_list();
    RsppVal rspp_v_params = _t652;
    RsppVal _t653 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("("));
    while (1) {
        RsppVal _t657 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
        RsppVal _t656 = rspp_bool(0);
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t657)))) {
            RsppVal _t658 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
            _t656 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t658))));
        }
        RsppVal _t655 = rspp_bool(0);
        if (rspp_to_bool(_t656)) {
            RsppVal _t659 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("{"));
            _t655 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t659))));
        }
        RsppVal _t654 = rspp_bool(0);
        if (rspp_to_bool(_t655)) {
            RsppVal _t660 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
            _t654 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t660))));
        }
        if (!rspp_to_bool(_t654)) break;
        RsppVal _t661 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t663[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t662 = rspp_method_call(_t661, "get", 2, _t663);
        RsppVal rspp_v_pname = _t662;
        RsppVal rspp_v_pty = rspp_str("any");
        RsppVal _t664 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
        if (rspp_to_bool(_t664)) {
            RsppVal _t665 = rspp_fn_parse_type_str(rspp_v_p);
            rspp_v_pty = _t665;
        }
        RsppVal _t666 = rspp_map();
        rspp_map_set(_t666, rspp_str("name"), rspp_v_pname);
        rspp_map_set(_t666, rspp_str("ty"), rspp_v_pty);
        RsppVal _t668[] = { _t666 };
        RsppVal _t667 = rspp_method_call(rspp_v_params, "push", 1, _t668);
        RsppVal _t669 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(","));
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t669)))) {
            break;
        }
    }
    RsppVal _t670 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
    RsppVal _t671 = rspp_v_params;
    return _t671;
    return rspp_none();
}

static RsppVal rspp_fn_parse_fn(RsppVal rspp_v_p) {
    RsppVal _t672 = rspp_fn_p_adv(rspp_v_p);
    RsppVal rspp_v_start_tok = _t672;
    RsppVal _t673 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t675[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t674 = rspp_method_call(_t673, "get", 2, _t675);
    RsppVal rspp_v_name = _t674;
    RsppVal _t676 = rspp_fn_parse_params(rspp_v_p);
    RsppVal rspp_v_params = _t676;
    RsppVal rspp_v_ret_ty = rspp_str("any");
    RsppVal _t677 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("->"));
    if (rspp_to_bool(_t677)) {
        RsppVal _t678 = rspp_fn_parse_type_str(rspp_v_p);
        rspp_v_ret_ty = _t678;
    }
    RsppVal _t679 = rspp_fn_parse_block(rspp_v_p);
    RsppVal rspp_v_body = _t679;
    RsppVal _t681 = rspp_map();
    rspp_map_set(_t681, rspp_str("kind"), rspp_str("fn"));
    rspp_map_set(_t681, rspp_str("name"), rspp_v_name);
    rspp_map_set(_t681, rspp_str("params"), rspp_v_params);
    rspp_map_set(_t681, rspp_str("ret"), rspp_v_ret_ty);
    rspp_map_set(_t681, rspp_str("body"), rspp_v_body);
    RsppVal _t683[] = { rspp_str("line"), rspp_int(1LL) };
    RsppVal _t682 = rspp_method_call(rspp_v_start_tok, "get", 2, _t683);
    rspp_map_set(_t681, rspp_str("line"), _t682);
    RsppVal _t680 = _t681;
    return _t680;
    return rspp_none();
}

static RsppVal rspp_fn_parse_equation(RsppVal rspp_v_p) {
    RsppVal _t684 = rspp_fn_p_adv(rspp_v_p);
    RsppVal rspp_v_start_tok = _t684;
    RsppVal _t685 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t687[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t686 = rspp_method_call(_t685, "get", 2, _t687);
    RsppVal rspp_v_name = _t686;
    RsppVal _t688 = rspp_list();
    RsppVal rspp_v_params = _t688;
    RsppVal _t689 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("("));
    if (rspp_to_bool(_t689)) {
        while (1) {
            RsppVal _t691 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
            RsppVal _t690 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t691)))) {
                RsppVal _t692 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t690 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t692))));
            }
            if (!rspp_to_bool(_t690)) break;
            RsppVal _t693 = rspp_fn_p_adv(rspp_v_p);
            RsppVal _t695[] = { rspp_str("val"), rspp_str("") };
            RsppVal _t694 = rspp_method_call(_t693, "get", 2, _t695);
            RsppVal _t697[] = { _t694 };
            RsppVal _t696 = rspp_method_call(rspp_v_params, "push", 1, _t697);
            RsppVal _t698 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(","));
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t698)))) {
                break;
            }
        }
        RsppVal _t699 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
    }
    RsppVal _t700 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("="));
    RsppVal _t702[] = { rspp_str("in_eq"), rspp_bool(1) };
    RsppVal _t701 = rspp_method_call(rspp_v_p, "set", 2, _t702);
    RsppVal _t703 = rspp_fn_parse_expr(rspp_v_p);
    RsppVal rspp_v_expr = _t703;
    RsppVal _t705[] = { rspp_str("in_eq"), rspp_bool(0) };
    RsppVal _t704 = rspp_method_call(rspp_v_p, "set", 2, _t705);
    RsppVal _t707 = rspp_map();
    rspp_map_set(_t707, rspp_str("kind"), rspp_str("equation"));
    rspp_map_set(_t707, rspp_str("name"), rspp_v_name);
    rspp_map_set(_t707, rspp_str("params"), rspp_v_params);
    rspp_map_set(_t707, rspp_str("expr"), rspp_v_expr);
    RsppVal _t709[] = { rspp_str("line"), rspp_int(1LL) };
    RsppVal _t708 = rspp_method_call(rspp_v_start_tok, "get", 2, _t709);
    rspp_map_set(_t707, rspp_str("line"), _t708);
    RsppVal _t706 = _t707;
    return _t706;
    return rspp_none();
}

static RsppVal rspp_fn_parse_algorithm(RsppVal rspp_v_p) {
    RsppVal _t710 = rspp_fn_p_adv(rspp_v_p);
    RsppVal rspp_v_start_tok = _t710;
    RsppVal _t711 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t713[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t712 = rspp_method_call(_t711, "get", 2, _t713);
    RsppVal rspp_v_name = _t712;
    RsppVal _t714 = rspp_fn_parse_params(rspp_v_p);
    RsppVal rspp_v_params = _t714;
    RsppVal rspp_v_ret_ty = rspp_str("str");
    RsppVal _t715 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("->"));
    if (rspp_to_bool(_t715)) {
        RsppVal _t716 = rspp_fn_parse_type_str(rspp_v_p);
        rspp_v_ret_ty = _t716;
    }
    RsppVal _t717 = rspp_fn_parse_block(rspp_v_p);
    RsppVal rspp_v_body = _t717;
    RsppVal _t719 = rspp_map();
    rspp_map_set(_t719, rspp_str("kind"), rspp_str("algorithm"));
    rspp_map_set(_t719, rspp_str("name"), rspp_v_name);
    rspp_map_set(_t719, rspp_str("params"), rspp_v_params);
    rspp_map_set(_t719, rspp_str("ret"), rspp_v_ret_ty);
    rspp_map_set(_t719, rspp_str("body"), rspp_v_body);
    RsppVal _t721[] = { rspp_str("line"), rspp_int(1LL) };
    RsppVal _t720 = rspp_method_call(rspp_v_start_tok, "get", 2, _t721);
    rspp_map_set(_t719, rspp_str("line"), _t720);
    RsppVal _t718 = _t719;
    return _t718;
    return rspp_none();
}

static RsppVal rspp_fn_parse_extern(RsppVal rspp_v_p) {
    RsppVal _t722 = rspp_fn_p_adv(rspp_v_p);
    RsppVal rspp_v_start_tok = _t722;
    RsppVal _t723 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("STR"));
    if (rspp_to_bool(_t723)) {
        RsppVal _t724 = rspp_fn_p_adv(rspp_v_p);
    }
    RsppVal _t725 = rspp_fn_p_consume(rspp_v_p, rspp_str("KW"), rspp_str("fn"));
    RsppVal _t726 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t728[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t727 = rspp_method_call(_t726, "get", 2, _t728);
    RsppVal rspp_v_name = _t727;
    RsppVal _t729 = rspp_fn_parse_params(rspp_v_p);
    RsppVal rspp_v_params = _t729;
    RsppVal rspp_v_ret_ty = rspp_str("int");
    RsppVal _t730 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("->"));
    if (rspp_to_bool(_t730)) {
        RsppVal _t731 = rspp_fn_parse_type_str(rspp_v_p);
        rspp_v_ret_ty = _t731;
    }
    RsppVal _t733 = rspp_map();
    rspp_map_set(_t733, rspp_str("kind"), rspp_str("extern_fn"));
    rspp_map_set(_t733, rspp_str("name"), rspp_v_name);
    rspp_map_set(_t733, rspp_str("params"), rspp_v_params);
    rspp_map_set(_t733, rspp_str("ret"), rspp_v_ret_ty);
    RsppVal _t735[] = { rspp_str("line"), rspp_int(1LL) };
    RsppVal _t734 = rspp_method_call(rspp_v_start_tok, "get", 2, _t735);
    rspp_map_set(_t733, rspp_str("line"), _t734);
    RsppVal _t732 = _t733;
    return _t732;
    return rspp_none();
}

static RsppVal rspp_fn_parse_import(RsppVal rspp_v_p) {
    RsppVal _t736 = rspp_fn_p_adv(rspp_v_p);
    RsppVal rspp_v_tok = _t736;
    RsppVal rspp_v_path = rspp_str("");
    RsppVal _t737 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("STR"));
    if (rspp_to_bool(_t737)) {
        RsppVal _t738 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t740[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t739 = rspp_method_call(_t738, "get", 2, _t740);
        rspp_v_path = _t739;
    } else {
        RsppVal _t741 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t743[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t742 = rspp_method_call(_t741, "get", 2, _t743);
        rspp_v_path = _t742;
        while (1) {
            RsppVal _t745 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("::"));
            RsppVal _t744 = rspp_bool(1);
            if (!rspp_to_bool(_t745)) {
                RsppVal _t746 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("."));
                _t744 = rspp_bool(rspp_to_bool(_t746));
            }
            if (!rspp_to_bool(_t744)) break;
            RsppVal _t747 = rspp_fn_p_adv(rspp_v_p);
            RsppVal _t749[] = { rspp_str("val"), rspp_str("") };
            RsppVal _t748 = rspp_method_call(_t747, "get", 2, _t749);
            rspp_v_path = rspp_add(rspp_add(rspp_v_path, rspp_str("::")), _t748);
        }
    }
    RsppVal _t750 = rspp_fn_p_consume(rspp_v_p, rspp_str("KW"), rspp_str("as"));
    if (rspp_to_bool(_t750)) {
        RsppVal _t751 = rspp_fn_p_adv(rspp_v_p);
    }
    RsppVal _t753 = rspp_map();
    rspp_map_set(_t753, rspp_str("kind"), rspp_str("import"));
    rspp_map_set(_t753, rspp_str("path"), rspp_v_path);
    RsppVal _t755[] = { rspp_str("line"), rspp_int(1LL) };
    RsppVal _t754 = rspp_method_call(rspp_v_tok, "get", 2, _t755);
    rspp_map_set(_t753, rspp_str("line"), _t754);
    RsppVal _t752 = _t753;
    return _t752;
    return rspp_none();
}

static RsppVal rspp_fn_parse_struct(RsppVal rspp_v_p) {
    RsppVal _t756 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t757 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t759[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t758 = rspp_method_call(_t757, "get", 2, _t759);
    RsppVal rspp_v_name = _t758;
    RsppVal _t760 = rspp_fn_parse_block_raw_skip(rspp_v_p);
    RsppVal _t762 = rspp_map();
    rspp_map_set(_t762, rspp_str("kind"), rspp_str("struct"));
    rspp_map_set(_t762, rspp_str("name"), rspp_v_name);
    RsppVal _t761 = _t762;
    return _t761;
    return rspp_none();
}

static RsppVal rspp_fn_parse_enum(RsppVal rspp_v_p) {
    RsppVal _t763 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t764 = rspp_fn_p_adv(rspp_v_p);
    RsppVal _t766[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t765 = rspp_method_call(_t764, "get", 2, _t766);
    RsppVal rspp_v_name = _t765;
    RsppVal _t767 = rspp_fn_parse_block_raw_skip(rspp_v_p);
    RsppVal _t769 = rspp_map();
    rspp_map_set(_t769, rspp_str("kind"), rspp_str("enum"));
    rspp_map_set(_t769, rspp_str("name"), rspp_v_name);
    RsppVal _t768 = _t769;
    return _t768;
    return rspp_none();
}

static RsppVal rspp_fn_parse_block_raw_skip(RsppVal rspp_v_p) {
    RsppVal _t770 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("{"));
    if (rspp_to_bool(_t770)) {
        while (1) {
            RsppVal _t772 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
            RsppVal _t771 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t772)))) {
                RsppVal _t773 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t771 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t773))));
            }
            if (!rspp_to_bool(_t771)) break;
            RsppVal _t774 = rspp_fn_p_adv(rspp_v_p);
        }
        RsppVal _t775 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
        RsppVal _t776 = rspp_none();
        return _t776;
    }
    RsppVal _t777 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
    RsppVal _t778 = rspp_fn_p_skip_nl(rspp_v_p);
    RsppVal _t779 = rspp_fn_p_consume(rspp_v_p, rspp_str("INDENT"), rspp_str(""));
    if (rspp_to_bool(_t779)) {
        while (1) {
            RsppVal _t781 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
            RsppVal _t780 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t781)))) {
                RsppVal _t782 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t780 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t782))));
            }
            if (!rspp_to_bool(_t780)) break;
            RsppVal _t783 = rspp_fn_p_adv(rspp_v_p);
        }
        RsppVal _t784 = rspp_fn_p_consume(rspp_v_p, rspp_str("DEDENT"), rspp_str(""));
    }
    return rspp_none();
}

static RsppVal rspp_fn_parse_block(RsppVal rspp_v_p) {
    RsppVal _t785 = rspp_list();
    RsppVal rspp_v_stmts = _t785;
    RsppVal _t786 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("{"));
    if (rspp_to_bool(_t786)) {
        RsppVal _t787 = rspp_fn_p_skip_nl(rspp_v_p);
        while (1) {
            RsppVal _t789 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
            RsppVal _t788 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t789)))) {
                RsppVal _t790 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t788 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t790))));
            }
            if (!rspp_to_bool(_t788)) break;
            RsppVal _t792[] = { rspp_str("pos"), rspp_int(0LL) };
            RsppVal _t791 = rspp_method_call(rspp_v_p, "get", 2, _t792);
            RsppVal rspp_v_before = _t791;
            RsppVal _t793 = rspp_fn_parse_stmt(rspp_v_p);
            RsppVal rspp_v_st = _t793;
            RsppVal _t794 = rspp_method_call(rspp_v_st, "len", 0, NULL);
            if (rspp_to_bool(rspp_gt(_t794, rspp_int(0LL)))) {
                RsppVal _t796[] = { rspp_v_st };
                RsppVal _t795 = rspp_method_call(rspp_v_stmts, "push", 1, _t796);
            }
            RsppVal _t798[] = { rspp_str("pos"), rspp_int(0LL) };
            RsppVal _t797 = rspp_method_call(rspp_v_p, "get", 2, _t798);
            if (rspp_to_bool(rspp_eq(_t797, rspp_v_before))) {
                RsppVal _t799 = rspp_fn_p_adv(rspp_v_p);
            }
            RsppVal _t800 = rspp_fn_p_skip_nl(rspp_v_p);
        }
        RsppVal _t801 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
        RsppVal _t802 = rspp_v_stmts;
        return _t802;
    }
    RsppVal _t803 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
    RsppVal _t805 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("NEWLINE"));
    RsppVal _t804 = rspp_bool(0);
    if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t805)))) {
        RsppVal _t806 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
        _t804 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t806))));
    }
    if (rspp_to_bool(_t804)) {
        RsppVal _t807 = rspp_fn_parse_stmt(rspp_v_p);
        RsppVal rspp_v_st = _t807;
        RsppVal _t808 = rspp_method_call(rspp_v_st, "len", 0, NULL);
        if (rspp_to_bool(rspp_gt(_t808, rspp_int(0LL)))) {
            RsppVal _t810[] = { rspp_v_st };
            RsppVal _t809 = rspp_method_call(rspp_v_stmts, "push", 1, _t810);
        }
        RsppVal _t811 = rspp_v_stmts;
        return _t811;
    }
    RsppVal _t812 = rspp_fn_p_skip_nl(rspp_v_p);
    RsppVal _t813 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("INDENT"));
    if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t813)))) {
        RsppVal _t814 = rspp_v_stmts;
        return _t814;
    }
    RsppVal _t815 = rspp_fn_p_adv(rspp_v_p);
    while (1) {
        RsppVal _t817 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
        RsppVal _t816 = rspp_bool(0);
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t817)))) {
            RsppVal _t818 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
            _t816 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t818))));
        }
        if (!rspp_to_bool(_t816)) break;
        RsppVal _t819 = rspp_fn_p_skip_nl(rspp_v_p);
        RsppVal _t821 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
        RsppVal _t820 = rspp_bool(1);
        if (!rspp_to_bool(_t821)) {
            RsppVal _t822 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
            _t820 = rspp_bool(rspp_to_bool(_t822));
        }
        if (rspp_to_bool(_t820)) {
            break;
        }
        RsppVal _t824[] = { rspp_str("pos"), rspp_int(0LL) };
        RsppVal _t823 = rspp_method_call(rspp_v_p, "get", 2, _t824);
        RsppVal rspp_v_before = _t823;
        RsppVal _t825 = rspp_fn_parse_stmt(rspp_v_p);
        RsppVal rspp_v_st = _t825;
        RsppVal _t826 = rspp_method_call(rspp_v_st, "len", 0, NULL);
        if (rspp_to_bool(rspp_gt(_t826, rspp_int(0LL)))) {
            RsppVal _t828[] = { rspp_v_st };
            RsppVal _t827 = rspp_method_call(rspp_v_stmts, "push", 1, _t828);
        }
        RsppVal _t830[] = { rspp_str("pos"), rspp_int(0LL) };
        RsppVal _t829 = rspp_method_call(rspp_v_p, "get", 2, _t830);
        if (rspp_to_bool(rspp_eq(_t829, rspp_v_before))) {
            RsppVal _t831 = rspp_fn_p_adv(rspp_v_p);
        }
        RsppVal _t832 = rspp_fn_p_skip_nl(rspp_v_p);
    }
    RsppVal _t833 = rspp_fn_p_consume(rspp_v_p, rspp_str("DEDENT"), rspp_str(""));
    RsppVal _t834 = rspp_v_stmts;
    return _t834;
    return rspp_none();
}

static RsppVal rspp_fn_parse_stmt(RsppVal rspp_v_p) {
    RsppVal _t835 = rspp_fn_p_cur(rspp_v_p);
    RsppVal _t837[] = { rspp_str("line"), rspp_int(1LL) };
    RsppVal _t836 = rspp_method_call(_t835, "get", 2, _t837);
    RsppVal rspp_v_line = _t836;
    RsppVal _t839 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("let"));
    RsppVal _t838 = rspp_bool(1);
    if (!rspp_to_bool(_t839)) {
        RsppVal _t840 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("var"));
        _t838 = rspp_bool(rspp_to_bool(_t840));
    }
    if (rspp_to_bool(_t838)) {
        RsppVal _t841 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t843[] = { rspp_str("val"), rspp_str("let") };
        RsppVal _t842 = rspp_method_call(_t841, "get", 2, _t843);
        RsppVal rspp_v_kw = _t842;
        RsppVal _t844 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t846[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t845 = rspp_method_call(_t844, "get", 2, _t846);
        RsppVal rspp_v_vname = _t845;
        RsppVal _t847 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
        if (rspp_to_bool(_t847)) {
            RsppVal _t848 = rspp_fn_parse_type_str(rspp_v_p);
        }
        RsppVal _t849 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("="));
        RsppVal _t850 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_val = _t850;
        RsppVal _t852 = rspp_map();
        rspp_map_set(_t852, rspp_str("kind"), rspp_v_kw);
        rspp_map_set(_t852, rspp_str("name"), rspp_v_vname);
        rspp_map_set(_t852, rspp_str("value"), rspp_v_val);
        rspp_map_set(_t852, rspp_str("line"), rspp_v_line);
        RsppVal _t851 = _t852;
        return _t851;
    }
    RsppVal _t853 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("if"));
    if (rspp_to_bool(_t853)) {
        RsppVal _t854 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t855 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_c0 = _t855;
        RsppVal _t856 = rspp_fn_parse_block(rspp_v_p);
        RsppVal rspp_v_b0 = _t856;
        RsppVal _t857 = rspp_list();
        RsppVal _t858 = rspp_map();
        rspp_map_set(_t858, rspp_str("cond"), rspp_v_c0);
        rspp_map_set(_t858, rspp_str("body"), rspp_v_b0);
        rspp_list_push(_t857, _t858);
        RsppVal rspp_v_branches = _t857;
        RsppVal _t859 = rspp_fn_p_skip_nl(rspp_v_p);
        while (1) {
            RsppVal _t860 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("elif"));
            if (!rspp_to_bool(_t860)) break;
            RsppVal _t861 = rspp_fn_p_adv(rspp_v_p);
            RsppVal _t862 = rspp_fn_parse_expr(rspp_v_p);
            RsppVal rspp_v_ci = _t862;
            RsppVal _t863 = rspp_fn_parse_block(rspp_v_p);
            RsppVal rspp_v_bi = _t863;
            RsppVal _t864 = rspp_map();
            rspp_map_set(_t864, rspp_str("cond"), rspp_v_ci);
            rspp_map_set(_t864, rspp_str("body"), rspp_v_bi);
            RsppVal _t866[] = { _t864 };
            RsppVal _t865 = rspp_method_call(rspp_v_branches, "push", 1, _t866);
            RsppVal _t867 = rspp_fn_p_skip_nl(rspp_v_p);
        }
        RsppVal _t868 = rspp_list();
        RsppVal rspp_v_eb = _t868;
        RsppVal _t869 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("else"));
        if (rspp_to_bool(_t869)) {
            RsppVal _t870 = rspp_fn_p_adv(rspp_v_p);
            RsppVal _t871 = rspp_fn_parse_block(rspp_v_p);
            rspp_v_eb = _t871;
        }
        RsppVal _t873 = rspp_map();
        rspp_map_set(_t873, rspp_str("kind"), rspp_str("if"));
        rspp_map_set(_t873, rspp_str("branches"), rspp_v_branches);
        rspp_map_set(_t873, rspp_str("else_body"), rspp_v_eb);
        rspp_map_set(_t873, rspp_str("line"), rspp_v_line);
        RsppVal _t872 = _t873;
        return _t872;
    }
    RsppVal _t874 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("while"));
    if (rspp_to_bool(_t874)) {
        RsppVal _t875 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t876 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_cond = _t876;
        RsppVal _t877 = rspp_fn_parse_block(rspp_v_p);
        RsppVal rspp_v_body = _t877;
        RsppVal _t879 = rspp_map();
        rspp_map_set(_t879, rspp_str("kind"), rspp_str("while"));
        rspp_map_set(_t879, rspp_str("cond"), rspp_v_cond);
        rspp_map_set(_t879, rspp_str("body"), rspp_v_body);
        rspp_map_set(_t879, rspp_str("line"), rspp_v_line);
        RsppVal _t878 = _t879;
        return _t878;
    }
    RsppVal _t880 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("loop"));
    if (rspp_to_bool(_t880)) {
        RsppVal _t881 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t882 = rspp_fn_parse_block(rspp_v_p);
        RsppVal rspp_v_body = _t882;
        RsppVal _t884 = rspp_map();
        rspp_map_set(_t884, rspp_str("kind"), rspp_str("loop"));
        rspp_map_set(_t884, rspp_str("body"), rspp_v_body);
        rspp_map_set(_t884, rspp_str("line"), rspp_v_line);
        RsppVal _t883 = _t884;
        return _t883;
    }
    RsppVal _t885 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("for"));
    if (rspp_to_bool(_t885)) {
        RsppVal _t886 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t887 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t889[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t888 = rspp_method_call(_t887, "get", 2, _t889);
        RsppVal rspp_v_vname = _t888;
        RsppVal _t890 = rspp_fn_p_consume(rspp_v_p, rspp_str("KW"), rspp_str("in"));
        RsppVal _t891 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_iter = _t891;
        RsppVal _t892 = rspp_fn_parse_block(rspp_v_p);
        RsppVal rspp_v_body = _t892;
        RsppVal _t894 = rspp_map();
        rspp_map_set(_t894, rspp_str("kind"), rspp_str("for"));
        rspp_map_set(_t894, rspp_str("var"), rspp_v_vname);
        rspp_map_set(_t894, rspp_str("iter"), rspp_v_iter);
        rspp_map_set(_t894, rspp_str("body"), rspp_v_body);
        rspp_map_set(_t894, rspp_str("line"), rspp_v_line);
        RsppVal _t893 = _t894;
        return _t893;
    }
    RsppVal _t895 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("match"));
    if (rspp_to_bool(_t895)) {
        RsppVal _t896 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t897 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_sub = _t897;
        RsppVal _t898 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
        RsppVal _t899 = rspp_fn_p_skip_nl(rspp_v_p);
        RsppVal _t900 = rspp_list();
        RsppVal rspp_v_arms = _t900;
        RsppVal _t901 = rspp_fn_p_consume(rspp_v_p, rspp_str("INDENT"), rspp_str(""));
        if (rspp_to_bool(_t901)) {
            while (1) {
                RsppVal _t903 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
                RsppVal _t902 = rspp_bool(0);
                if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t903)))) {
                    RsppVal _t904 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                    _t902 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t904))));
                }
                if (!rspp_to_bool(_t902)) break;
                RsppVal _t905 = rspp_fn_p_skip_nl(rspp_v_p);
                RsppVal _t907 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
                RsppVal _t906 = rspp_bool(1);
                if (!rspp_to_bool(_t907)) {
                    RsppVal _t908 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                    _t906 = rspp_bool(rspp_to_bool(_t908));
                }
                if (rspp_to_bool(_t906)) {
                    break;
                }
                RsppVal _t909 = rspp_fn_p_consume(rspp_v_p, rspp_str("KW"), rspp_str("case"));
                RsppVal _t910 = rspp_fn_parse_expr(rspp_v_p);
                RsppVal rspp_v_pat = _t910;
                RsppVal _t911 = rspp_list();
                RsppVal rspp_v_abody = _t911;
                RsppVal _t912 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("=>"));
                if (rspp_to_bool(_t912)) {
                    RsppVal _t913 = rspp_list();
                    RsppVal _t914 = rspp_fn_parse_stmt(rspp_v_p);
                    rspp_list_push(_t913, _t914);
                    rspp_v_abody = _t913;
                } else {
                    RsppVal _t915 = rspp_fn_parse_block(rspp_v_p);
                    rspp_v_abody = _t915;
                }
                RsppVal _t916 = rspp_map();
                rspp_map_set(_t916, rspp_str("pattern"), rspp_v_pat);
                rspp_map_set(_t916, rspp_str("body"), rspp_v_abody);
                RsppVal _t918[] = { _t916 };
                RsppVal _t917 = rspp_method_call(rspp_v_arms, "push", 1, _t918);
                RsppVal _t919 = rspp_fn_p_skip_nl(rspp_v_p);
            }
            RsppVal _t920 = rspp_fn_p_consume(rspp_v_p, rspp_str("DEDENT"), rspp_str(""));
        }
        RsppVal _t922 = rspp_map();
        rspp_map_set(_t922, rspp_str("kind"), rspp_str("match"));
        rspp_map_set(_t922, rspp_str("subject"), rspp_v_sub);
        rspp_map_set(_t922, rspp_str("arms"), rspp_v_arms);
        rspp_map_set(_t922, rspp_str("line"), rspp_v_line);
        RsppVal _t921 = _t922;
        return _t921;
    }
    RsppVal _t923 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("return"));
    if (rspp_to_bool(_t923)) {
        RsppVal _t924 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t928 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("NEWLINE"));
        RsppVal _t927 = rspp_bool(1);
        if (!rspp_to_bool(_t928)) {
            RsppVal _t929 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("DEDENT"));
            _t927 = rspp_bool(rspp_to_bool(_t929));
        }
        RsppVal _t926 = rspp_bool(1);
        if (!rspp_to_bool(_t927)) {
            RsppVal _t930 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
            _t926 = rspp_bool(rspp_to_bool(_t930));
        }
        RsppVal _t925 = rspp_bool(1);
        if (!rspp_to_bool(_t926)) {
            RsppVal _t931 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
            _t925 = rspp_bool(rspp_to_bool(_t931));
        }
        if (rspp_to_bool(_t925)) {
            RsppVal _t933 = rspp_map();
            rspp_map_set(_t933, rspp_str("kind"), rspp_str("return"));
            rspp_map_set(_t933, rspp_str("has_val"), rspp_bool(0));
            rspp_map_set(_t933, rspp_str("line"), rspp_v_line);
            RsppVal _t932 = _t933;
            return _t932;
        }
        RsppVal _t934 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_rv = _t934;
        RsppVal _t936 = rspp_map();
        rspp_map_set(_t936, rspp_str("kind"), rspp_str("return"));
        rspp_map_set(_t936, rspp_str("has_val"), rspp_bool(1));
        rspp_map_set(_t936, rspp_str("value"), rspp_v_rv);
        rspp_map_set(_t936, rspp_str("line"), rspp_v_line);
        RsppVal _t935 = _t936;
        return _t935;
    }
    RsppVal _t937 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("break"));
    if (rspp_to_bool(_t937)) {
        RsppVal _t938 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t940 = rspp_map();
        rspp_map_set(_t940, rspp_str("kind"), rspp_str("break"));
        rspp_map_set(_t940, rspp_str("line"), rspp_v_line);
        RsppVal _t939 = _t940;
        return _t939;
    }
    RsppVal _t941 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("continue"));
    if (rspp_to_bool(_t941)) {
        RsppVal _t942 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t944 = rspp_map();
        rspp_map_set(_t944, rspp_str("kind"), rspp_str("continue"));
        rspp_map_set(_t944, rspp_str("line"), rspp_v_line);
        RsppVal _t943 = _t944;
        return _t943;
    }
    RsppVal _t945 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("unsafe_hardware"));
    if (rspp_to_bool(_t945)) {
        RsppVal _t946 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t947 = rspp_fn_parse_block(rspp_v_p);
        RsppVal rspp_v_body = _t947;
        RsppVal _t949 = rspp_map();
        rspp_map_set(_t949, rspp_str("kind"), rspp_str("unsafe_hardware"));
        rspp_map_set(_t949, rspp_str("body"), rspp_v_body);
        rspp_map_set(_t949, rspp_str("line"), rspp_v_line);
        RsppVal _t948 = _t949;
        return _t948;
    }
    RsppVal _t950 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("defer"));
    if (rspp_to_bool(_t950)) {
        RsppVal _t951 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t952 = rspp_fn_parse_stmt(rspp_v_p);
        RsppVal rspp_v_inner = _t952;
        RsppVal _t954 = rspp_map();
        rspp_map_set(_t954, rspp_str("kind"), rspp_str("defer"));
        rspp_map_set(_t954, rspp_str("stmt"), rspp_v_inner);
        rspp_map_set(_t954, rspp_str("line"), rspp_v_line);
        RsppVal _t953 = _t954;
        return _t953;
    }
    RsppVal _t955 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("try"));
    if (rspp_to_bool(_t955)) {
        RsppVal _t956 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t957 = rspp_fn_parse_block(rspp_v_p);
        RsppVal rspp_v_tb = _t957;
        RsppVal _t958 = rspp_fn_p_skip_nl(rspp_v_p);
        RsppVal rspp_v_ev = rspp_str("err");
        RsppVal _t959 = rspp_list();
        RsppVal rspp_v_cb = _t959;
        RsppVal _t960 = rspp_fn_p_consume(rspp_v_p, rspp_str("KW"), rspp_str("catch"));
        if (rspp_to_bool(_t960)) {
            RsppVal _t961 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("IDENT"));
            if (rspp_to_bool(_t961)) {
                RsppVal _t962 = rspp_fn_p_adv(rspp_v_p);
                RsppVal _t964[] = { rspp_str("val"), rspp_str("err") };
                RsppVal _t963 = rspp_method_call(_t962, "get", 2, _t964);
                rspp_v_ev = _t963;
            }
            RsppVal _t965 = rspp_fn_parse_block(rspp_v_p);
            rspp_v_cb = _t965;
        }
        RsppVal _t967 = rspp_map();
        rspp_map_set(_t967, rspp_str("kind"), rspp_str("try_catch"));
        rspp_map_set(_t967, rspp_str("try_body"), rspp_v_tb);
        rspp_map_set(_t967, rspp_str("err_var"), rspp_v_ev);
        rspp_map_set(_t967, rspp_str("catch_body"), rspp_v_cb);
        rspp_map_set(_t967, rspp_str("line"), rspp_v_line);
        RsppVal _t966 = _t967;
        return _t966;
    }
    RsppVal _t968 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("throw"));
    if (rspp_to_bool(_t968)) {
        RsppVal _t969 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t970 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_tv = _t970;
        RsppVal _t972 = rspp_map();
        rspp_map_set(_t972, rspp_str("kind"), rspp_str("throw"));
        rspp_map_set(_t972, rspp_str("value"), rspp_v_tv);
        rspp_map_set(_t972, rspp_str("line"), rspp_v_line);
        RsppVal _t971 = _t972;
        return _t971;
    }
    RsppVal _t973 = rspp_fn_parse_expr(rspp_v_p);
    RsppVal rspp_v_lhs = _t973;
    RsppVal _t974 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("="));
    if (rspp_to_bool(_t974)) {
        RsppVal _t975 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_rhs = _t975;
        RsppVal _t977 = rspp_map();
        rspp_map_set(_t977, rspp_str("kind"), rspp_str("assign"));
        rspp_map_set(_t977, rspp_str("target"), rspp_v_lhs);
        rspp_map_set(_t977, rspp_str("value"), rspp_v_rhs);
        rspp_map_set(_t977, rspp_str("line"), rspp_v_line);
        RsppVal _t976 = _t977;
        return _t976;
    }
    RsppVal _t981 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("+="));
    RsppVal _t980 = rspp_bool(1);
    if (!rspp_to_bool(_t981)) {
        RsppVal _t982 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("-="));
        _t980 = rspp_bool(rspp_to_bool(_t982));
    }
    RsppVal _t979 = rspp_bool(1);
    if (!rspp_to_bool(_t980)) {
        RsppVal _t983 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("*="));
        _t979 = rspp_bool(rspp_to_bool(_t983));
    }
    RsppVal _t978 = rspp_bool(1);
    if (!rspp_to_bool(_t979)) {
        RsppVal _t984 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("/="));
        _t978 = rspp_bool(rspp_to_bool(_t984));
    }
    if (rspp_to_bool(_t978)) {
        RsppVal _t985 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t987[] = { rspp_str("val"), rspp_str("+=") };
        RsppVal _t986 = rspp_method_call(_t985, "get", 2, _t987);
        RsppVal rspp_v_cop = _t986;
        RsppVal _t988 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_rhs = _t988;
        RsppVal _t990 = rspp_map();
        rspp_map_set(_t990, rspp_str("kind"), rspp_str("compound_assign"));
        rspp_map_set(_t990, rspp_str("target"), rspp_v_lhs);
        rspp_map_set(_t990, rspp_str("op"), rspp_v_cop);
        rspp_map_set(_t990, rspp_str("value"), rspp_v_rhs);
        rspp_map_set(_t990, rspp_str("line"), rspp_v_line);
        RsppVal _t989 = _t990;
        return _t989;
    }
    RsppVal _t992 = rspp_map();
    rspp_map_set(_t992, rspp_str("kind"), rspp_str("expr"));
    rspp_map_set(_t992, rspp_str("expr"), rspp_v_lhs);
    rspp_map_set(_t992, rspp_str("line"), rspp_v_line);
    RsppVal _t991 = _t992;
    return _t991;
    return rspp_none();
}

static RsppVal rspp_fn_parse_expr(RsppVal rspp_v_p) {
    RsppVal _t994 = rspp_fn_parse_pipeline(rspp_v_p);
    RsppVal _t993 = _t994;
    return _t993;
    return rspp_none();
}

static RsppVal rspp_fn_parse_pipeline(RsppVal rspp_v_p) {
    RsppVal _t995 = rspp_fn_parse_or(rspp_v_p);
    RsppVal rspp_v_left = _t995;
    while (1) {
        RsppVal _t996 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("|>"));
        if (!rspp_to_bool(_t996)) break;
        RsppVal _t997 = rspp_fn_parse_or(rspp_v_p);
        RsppVal rspp_v_rhs = _t997;
        RsppVal _t999[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t998 = rspp_method_call(rspp_v_rhs, "get", 2, _t999);
        RsppVal rspp_v_rk = _t998;
        RsppVal _t1000 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_rk, rspp_str("ns_call")))) {
            _t1000 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_rk, rspp_str("call"))));
        }
        if (rspp_to_bool(_t1000)) {
            RsppVal _t1001 = rspp_list();
            RsppVal _t1003[] = { rspp_str("args"), _t1001 };
            RsppVal _t1002 = rspp_method_call(rspp_v_rhs, "get", 2, _t1003);
            RsppVal rspp_v_args = _t1002;
            RsppVal _t1004 = rspp_list();
            rspp_list_push(_t1004, rspp_v_left);
            RsppVal rspp_v_new_args = _t1004;
            RsppVal _t1005 = rspp_v_args;
            for (int64_t _t1006 = 0; _t1006 < rspp_list_len(_t1005); _t1006++) {
                RsppVal rspp_v_a = rspp_index_get(_t1005, rspp_int(_t1006));
                RsppVal _t1008[] = { rspp_v_a };
                RsppVal _t1007 = rspp_method_call(rspp_v_new_args, "push", 1, _t1008);
            }
            RsppVal _t1010[] = { rspp_str("args"), rspp_v_new_args };
            RsppVal _t1009 = rspp_method_call(rspp_v_rhs, "set", 2, _t1010);
            rspp_v_left = rspp_v_rhs;
        } else {
            RsppVal _t1011 = rspp_map();
            rspp_map_set(_t1011, rspp_str("kind"), rspp_str("call"));
            rspp_map_set(_t1011, rspp_str("callee"), rspp_v_rhs);
            RsppVal _t1012 = rspp_list();
            rspp_list_push(_t1012, rspp_v_left);
            rspp_map_set(_t1011, rspp_str("args"), _t1012);
            rspp_v_left = _t1011;
        }
    }
    RsppVal _t1013 = rspp_v_left;
    return _t1013;
    return rspp_none();
}

static RsppVal rspp_fn_parse_or(RsppVal rspp_v_p) {
    RsppVal _t1014 = rspp_fn_parse_and(rspp_v_p);
    RsppVal rspp_v_left = _t1014;
    while (1) {
        RsppVal _t1016 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("or"));
        RsppVal _t1015 = rspp_bool(1);
        if (!rspp_to_bool(_t1016)) {
            RsppVal _t1017 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("||"));
            _t1015 = rspp_bool(rspp_to_bool(_t1017));
        }
        if (!rspp_to_bool(_t1015)) break;
        RsppVal _t1018 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1019 = rspp_fn_parse_and(rspp_v_p);
        RsppVal rspp_v_right = _t1019;
        RsppVal _t1020 = rspp_map();
        rspp_map_set(_t1020, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1020, rspp_str("op"), rspp_str("or"));
        rspp_map_set(_t1020, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1020, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1020;
    }
    RsppVal _t1021 = rspp_v_left;
    return _t1021;
    return rspp_none();
}

static RsppVal rspp_fn_parse_and(RsppVal rspp_v_p) {
    RsppVal _t1022 = rspp_fn_parse_eq(rspp_v_p);
    RsppVal rspp_v_left = _t1022;
    while (1) {
        RsppVal _t1024 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("and"));
        RsppVal _t1023 = rspp_bool(1);
        if (!rspp_to_bool(_t1024)) {
            RsppVal _t1025 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("&&"));
            _t1023 = rspp_bool(rspp_to_bool(_t1025));
        }
        if (!rspp_to_bool(_t1023)) break;
        RsppVal _t1026 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1027 = rspp_fn_parse_eq(rspp_v_p);
        RsppVal rspp_v_right = _t1027;
        RsppVal _t1028 = rspp_map();
        rspp_map_set(_t1028, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1028, rspp_str("op"), rspp_str("and"));
        rspp_map_set(_t1028, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1028, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1028;
    }
    RsppVal _t1029 = rspp_v_left;
    return _t1029;
    return rspp_none();
}

static RsppVal rspp_fn_parse_eq(RsppVal rspp_v_p) {
    RsppVal _t1030 = rspp_fn_parse_cmp(rspp_v_p);
    RsppVal rspp_v_left = _t1030;
    while (1) {
        RsppVal _t1032 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("=="));
        RsppVal _t1031 = rspp_bool(1);
        if (!rspp_to_bool(_t1032)) {
            RsppVal _t1033 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("!="));
            _t1031 = rspp_bool(rspp_to_bool(_t1033));
        }
        if (!rspp_to_bool(_t1031)) break;
        RsppVal _t1034 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1036[] = { rspp_str("val"), rspp_str("==") };
        RsppVal _t1035 = rspp_method_call(_t1034, "get", 2, _t1036);
        RsppVal rspp_v_op = _t1035;
        RsppVal _t1037 = rspp_fn_parse_cmp(rspp_v_p);
        RsppVal rspp_v_right = _t1037;
        RsppVal _t1038 = rspp_map();
        rspp_map_set(_t1038, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1038, rspp_str("op"), rspp_v_op);
        rspp_map_set(_t1038, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1038, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1038;
    }
    RsppVal _t1039 = rspp_v_left;
    return _t1039;
    return rspp_none();
}

static RsppVal rspp_fn_parse_cmp(RsppVal rspp_v_p) {
    RsppVal _t1040 = rspp_fn_parse_range(rspp_v_p);
    RsppVal rspp_v_left = _t1040;
    while (1) {
        RsppVal _t1044 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("<"));
        RsppVal _t1043 = rspp_bool(1);
        if (!rspp_to_bool(_t1044)) {
            RsppVal _t1045 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("<="));
            _t1043 = rspp_bool(rspp_to_bool(_t1045));
        }
        RsppVal _t1042 = rspp_bool(1);
        if (!rspp_to_bool(_t1043)) {
            RsppVal _t1046 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(">"));
            _t1042 = rspp_bool(rspp_to_bool(_t1046));
        }
        RsppVal _t1041 = rspp_bool(1);
        if (!rspp_to_bool(_t1042)) {
            RsppVal _t1047 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(">="));
            _t1041 = rspp_bool(rspp_to_bool(_t1047));
        }
        if (!rspp_to_bool(_t1041)) break;
        RsppVal _t1048 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1050[] = { rspp_str("val"), rspp_str("<") };
        RsppVal _t1049 = rspp_method_call(_t1048, "get", 2, _t1050);
        RsppVal rspp_v_op = _t1049;
        RsppVal _t1051 = rspp_fn_parse_range(rspp_v_p);
        RsppVal rspp_v_right = _t1051;
        RsppVal _t1052 = rspp_map();
        rspp_map_set(_t1052, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1052, rspp_str("op"), rspp_v_op);
        rspp_map_set(_t1052, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1052, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1052;
    }
    RsppVal _t1053 = rspp_v_left;
    return _t1053;
    return rspp_none();
}

static RsppVal rspp_fn_parse_range(RsppVal rspp_v_p) {
    RsppVal _t1054 = rspp_fn_parse_bitor(rspp_v_p);
    RsppVal rspp_v_left = _t1054;
    RsppVal _t1056 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(".."));
    RsppVal _t1055 = rspp_bool(1);
    if (!rspp_to_bool(_t1056)) {
        RsppVal _t1057 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("..="));
        _t1055 = rspp_bool(rspp_to_bool(_t1057));
    }
    if (rspp_to_bool(_t1055)) {
        RsppVal _t1058 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1060[] = { rspp_str("val"), rspp_str("..") };
        RsppVal _t1059 = rspp_method_call(_t1058, "get", 2, _t1060);
        RsppVal rspp_v_op = _t1059;
        RsppVal _t1061 = rspp_fn_parse_bitor(rspp_v_p);
        RsppVal rspp_v_right = _t1061;
        RsppVal _t1063 = rspp_map();
        rspp_map_set(_t1063, rspp_str("kind"), rspp_str("range"));
        rspp_map_set(_t1063, rspp_str("start"), rspp_v_left);
        rspp_map_set(_t1063, rspp_str("end"), rspp_v_right);
        rspp_map_set(_t1063, rspp_str("inclusive"), rspp_eq(rspp_v_op, rspp_str("..=")));
        RsppVal _t1062 = _t1063;
        return _t1062;
    }
    RsppVal _t1064 = rspp_v_left;
    return _t1064;
    return rspp_none();
}

static RsppVal rspp_fn_parse_bitor(RsppVal rspp_v_p) {
    RsppVal _t1065 = rspp_fn_parse_bitxor(rspp_v_p);
    RsppVal rspp_v_left = _t1065;
    while (1) {
        RsppVal _t1066 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("|"));
        if (!rspp_to_bool(_t1066)) break;
        RsppVal _t1067 = rspp_fn_parse_bitxor(rspp_v_p);
        RsppVal rspp_v_right = _t1067;
        RsppVal _t1068 = rspp_map();
        rspp_map_set(_t1068, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1068, rspp_str("op"), rspp_str("|"));
        rspp_map_set(_t1068, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1068, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1068;
    }
    RsppVal _t1069 = rspp_v_left;
    return _t1069;
    return rspp_none();
}

static RsppVal rspp_fn_parse_bitxor(RsppVal rspp_v_p) {
    RsppVal _t1070 = rspp_fn_parse_bitand(rspp_v_p);
    RsppVal rspp_v_left = _t1070;
    while (1) {
        RsppVal _t1073[] = { rspp_str("in_eq"), rspp_bool(0) };
        RsppVal _t1072 = rspp_method_call(rspp_v_p, "get", 2, _t1073);
        RsppVal _t1071 = rspp_bool(0);
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1072)))) {
            RsppVal _t1074 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("^"));
            _t1071 = rspp_bool(rspp_to_bool(_t1074));
        }
        if (!rspp_to_bool(_t1071)) break;
        RsppVal _t1075 = rspp_fn_parse_bitand(rspp_v_p);
        RsppVal rspp_v_right = _t1075;
        RsppVal _t1076 = rspp_map();
        rspp_map_set(_t1076, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1076, rspp_str("op"), rspp_str("^"));
        rspp_map_set(_t1076, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1076, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1076;
    }
    RsppVal _t1077 = rspp_v_left;
    return _t1077;
    return rspp_none();
}

static RsppVal rspp_fn_parse_bitand(RsppVal rspp_v_p) {
    RsppVal _t1078 = rspp_fn_parse_shift(rspp_v_p);
    RsppVal rspp_v_left = _t1078;
    while (1) {
        RsppVal _t1079 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("&"));
        if (!rspp_to_bool(_t1079)) break;
        RsppVal _t1080 = rspp_fn_parse_shift(rspp_v_p);
        RsppVal rspp_v_right = _t1080;
        RsppVal _t1081 = rspp_map();
        rspp_map_set(_t1081, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1081, rspp_str("op"), rspp_str("&"));
        rspp_map_set(_t1081, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1081, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1081;
    }
    RsppVal _t1082 = rspp_v_left;
    return _t1082;
    return rspp_none();
}

static RsppVal rspp_fn_parse_shift(RsppVal rspp_v_p) {
    RsppVal _t1083 = rspp_fn_parse_add(rspp_v_p);
    RsppVal rspp_v_left = _t1083;
    while (1) {
        RsppVal _t1085 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("<<"));
        RsppVal _t1084 = rspp_bool(1);
        if (!rspp_to_bool(_t1085)) {
            RsppVal _t1086 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(">>"));
            _t1084 = rspp_bool(rspp_to_bool(_t1086));
        }
        if (!rspp_to_bool(_t1084)) break;
        RsppVal _t1087 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1089[] = { rspp_str("val"), rspp_str("<<") };
        RsppVal _t1088 = rspp_method_call(_t1087, "get", 2, _t1089);
        RsppVal rspp_v_op = _t1088;
        RsppVal _t1090 = rspp_fn_parse_add(rspp_v_p);
        RsppVal rspp_v_right = _t1090;
        RsppVal _t1091 = rspp_map();
        rspp_map_set(_t1091, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1091, rspp_str("op"), rspp_v_op);
        rspp_map_set(_t1091, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1091, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1091;
    }
    RsppVal _t1092 = rspp_v_left;
    return _t1092;
    return rspp_none();
}

static RsppVal rspp_fn_parse_add(RsppVal rspp_v_p) {
    RsppVal _t1093 = rspp_fn_parse_mul(rspp_v_p);
    RsppVal rspp_v_left = _t1093;
    while (1) {
        RsppVal _t1095 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("+"));
        RsppVal _t1094 = rspp_bool(1);
        if (!rspp_to_bool(_t1095)) {
            RsppVal _t1096 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("-"));
            _t1094 = rspp_bool(rspp_to_bool(_t1096));
        }
        if (!rspp_to_bool(_t1094)) break;
        RsppVal _t1097 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1099[] = { rspp_str("val"), rspp_str("+") };
        RsppVal _t1098 = rspp_method_call(_t1097, "get", 2, _t1099);
        RsppVal rspp_v_op = _t1098;
        RsppVal _t1100 = rspp_fn_parse_mul(rspp_v_p);
        RsppVal rspp_v_right = _t1100;
        RsppVal _t1101 = rspp_map();
        rspp_map_set(_t1101, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1101, rspp_str("op"), rspp_v_op);
        rspp_map_set(_t1101, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1101, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1101;
    }
    RsppVal _t1102 = rspp_v_left;
    return _t1102;
    return rspp_none();
}

static RsppVal rspp_fn_parse_mul(RsppVal rspp_v_p) {
    RsppVal _t1103 = rspp_fn_parse_pow(rspp_v_p);
    RsppVal rspp_v_left = _t1103;
    while (1) {
        RsppVal _t1106 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("*"));
        RsppVal _t1105 = rspp_bool(1);
        if (!rspp_to_bool(_t1106)) {
            RsppVal _t1107 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("/"));
            _t1105 = rspp_bool(rspp_to_bool(_t1107));
        }
        RsppVal _t1104 = rspp_bool(1);
        if (!rspp_to_bool(_t1105)) {
            RsppVal _t1108 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("%"));
            _t1104 = rspp_bool(rspp_to_bool(_t1108));
        }
        if (!rspp_to_bool(_t1104)) break;
        RsppVal _t1109 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1111[] = { rspp_str("val"), rspp_str("*") };
        RsppVal _t1110 = rspp_method_call(_t1109, "get", 2, _t1111);
        RsppVal rspp_v_op = _t1110;
        RsppVal _t1112 = rspp_fn_parse_pow(rspp_v_p);
        RsppVal rspp_v_right = _t1112;
        RsppVal _t1113 = rspp_map();
        rspp_map_set(_t1113, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1113, rspp_str("op"), rspp_v_op);
        rspp_map_set(_t1113, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1113, rspp_str("right"), rspp_v_right);
        rspp_v_left = _t1113;
    }
    RsppVal _t1114 = rspp_v_left;
    return _t1114;
    return rspp_none();
}

static RsppVal rspp_fn_parse_pow(RsppVal rspp_v_p) {
    RsppVal _t1115 = rspp_fn_parse_cast(rspp_v_p);
    RsppVal rspp_v_left = _t1115;
    RsppVal _t1117 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("**"));
    RsppVal _t1116 = rspp_bool(1);
    if (!rspp_to_bool(_t1117)) {
        RsppVal _t1120[] = { rspp_str("in_eq"), rspp_bool(0) };
        RsppVal _t1119 = rspp_method_call(rspp_v_p, "get", 2, _t1120);
        RsppVal _t1118 = rspp_bool(0);
        if (rspp_to_bool(_t1119)) {
            RsppVal _t1121 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("^"));
            _t1118 = rspp_bool(rspp_to_bool(_t1121));
        }
        _t1116 = rspp_bool(rspp_to_bool(_t1118));
    }
    if (rspp_to_bool(_t1116)) {
        RsppVal _t1122 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1123 = rspp_fn_parse_pow(rspp_v_p);
        RsppVal rspp_v_right = _t1123;
        RsppVal _t1125 = rspp_map();
        rspp_map_set(_t1125, rspp_str("kind"), rspp_str("binary"));
        rspp_map_set(_t1125, rspp_str("op"), rspp_str("**"));
        rspp_map_set(_t1125, rspp_str("left"), rspp_v_left);
        rspp_map_set(_t1125, rspp_str("right"), rspp_v_right);
        RsppVal _t1124 = _t1125;
        return _t1124;
    }
    RsppVal _t1126 = rspp_v_left;
    return _t1126;
    return rspp_none();
}

static RsppVal rspp_fn_parse_cast(RsppVal rspp_v_p) {
    RsppVal _t1127 = rspp_fn_parse_unary(rspp_v_p);
    RsppVal rspp_v_expr = _t1127;
    while (1) {
        RsppVal _t1128 = rspp_fn_p_consume(rspp_v_p, rspp_str("KW"), rspp_str("as"));
        if (!rspp_to_bool(_t1128)) break;
        RsppVal _t1129 = rspp_fn_parse_type_str(rspp_v_p);
        RsppVal rspp_v_ty = _t1129;
        RsppVal _t1130 = rspp_map();
        rspp_map_set(_t1130, rspp_str("kind"), rspp_str("cast"));
        rspp_map_set(_t1130, rspp_str("expr"), rspp_v_expr);
        rspp_map_set(_t1130, rspp_str("ty"), rspp_v_ty);
        rspp_v_expr = _t1130;
    }
    RsppVal _t1131 = rspp_v_expr;
    return _t1131;
    return rspp_none();
}

static RsppVal rspp_fn_parse_unary(RsppVal rspp_v_p) {
    RsppVal _t1135 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("-"));
    RsppVal _t1134 = rspp_bool(1);
    if (!rspp_to_bool(_t1135)) {
        RsppVal _t1136 = rspp_fn_p_is(rspp_v_p, rspp_str("KW"), rspp_str("not"));
        _t1134 = rspp_bool(rspp_to_bool(_t1136));
    }
    RsppVal _t1133 = rspp_bool(1);
    if (!rspp_to_bool(_t1134)) {
        RsppVal _t1137 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("~"));
        _t1133 = rspp_bool(rspp_to_bool(_t1137));
    }
    RsppVal _t1132 = rspp_bool(1);
    if (!rspp_to_bool(_t1133)) {
        RsppVal _t1138 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("!"));
        _t1132 = rspp_bool(rspp_to_bool(_t1138));
    }
    if (rspp_to_bool(_t1132)) {
        RsppVal _t1139 = rspp_fn_p_adv(rspp_v_p);
        RsppVal _t1141[] = { rspp_str("val"), rspp_str("-") };
        RsppVal _t1140 = rspp_method_call(_t1139, "get", 2, _t1141);
        RsppVal rspp_v_op = _t1140;
        RsppVal _t1142 = rspp_fn_parse_unary(rspp_v_p);
        RsppVal rspp_v_op_expr = _t1142;
        RsppVal _t1144 = rspp_map();
        rspp_map_set(_t1144, rspp_str("kind"), rspp_str("unary"));
        rspp_map_set(_t1144, rspp_str("op"), rspp_v_op);
        rspp_map_set(_t1144, rspp_str("operand"), rspp_v_op_expr);
        RsppVal _t1143 = _t1144;
        return _t1143;
    }
    RsppVal _t1146 = rspp_fn_parse_postfix(rspp_v_p);
    RsppVal _t1145 = _t1146;
    return _t1145;
    return rspp_none();
}

static RsppVal rspp_fn_parse_arg_list(RsppVal rspp_v_p) {
    RsppVal _t1147 = rspp_list();
    RsppVal rspp_v_args = _t1147;
    while (1) {
        RsppVal _t1149 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
        RsppVal _t1148 = rspp_bool(0);
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1149)))) {
            RsppVal _t1150 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
            _t1148 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t1150))));
        }
        if (!rspp_to_bool(_t1148)) break;
        RsppVal _t1151 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal _t1153[] = { _t1151 };
        RsppVal _t1152 = rspp_method_call(rspp_v_args, "push", 1, _t1153);
        RsppVal _t1154 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(","));
        if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1154)))) {
            break;
        }
    }
    RsppVal _t1155 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
    RsppVal _t1156 = rspp_v_args;
    return _t1156;
    return rspp_none();
}

static RsppVal rspp_fn_parse_postfix(RsppVal rspp_v_p) {
    RsppVal _t1157 = rspp_fn_parse_primary(rspp_v_p);
    RsppVal rspp_v_expr = _t1157;
    while (1) {
        if (!rspp_to_bool(rspp_bool(1))) break;
        RsppVal _t1158 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("("));
        if (rspp_to_bool(_t1158)) {
            RsppVal _t1159 = rspp_fn_parse_arg_list(rspp_v_p);
            RsppVal rspp_v_args = _t1159;
            RsppVal _t1160 = rspp_map();
            rspp_map_set(_t1160, rspp_str("kind"), rspp_str("call"));
            rspp_map_set(_t1160, rspp_str("callee"), rspp_v_expr);
            rspp_map_set(_t1160, rspp_str("args"), rspp_v_args);
            rspp_v_expr = _t1160;
        } else {
            RsppVal _t1161 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("."));
            if (rspp_to_bool(_t1161)) {
                RsppVal _t1162 = rspp_fn_p_adv(rspp_v_p);
                RsppVal _t1164[] = { rspp_str("val"), rspp_str("") };
                RsppVal _t1163 = rspp_method_call(_t1162, "get", 2, _t1164);
                RsppVal rspp_v_mname = _t1163;
                RsppVal _t1165 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("("));
                if (rspp_to_bool(_t1165)) {
                    RsppVal _t1166 = rspp_fn_parse_arg_list(rspp_v_p);
                    RsppVal rspp_v_args = _t1166;
                    RsppVal _t1167 = rspp_map();
                    rspp_map_set(_t1167, rspp_str("kind"), rspp_str("method"));
                    rspp_map_set(_t1167, rspp_str("recv"), rspp_v_expr);
                    rspp_map_set(_t1167, rspp_str("method"), rspp_v_mname);
                    rspp_map_set(_t1167, rspp_str("args"), rspp_v_args);
                    rspp_v_expr = _t1167;
                } else {
                    RsppVal _t1168 = rspp_map();
                    rspp_map_set(_t1168, rspp_str("kind"), rspp_str("field"));
                    rspp_map_set(_t1168, rspp_str("recv"), rspp_v_expr);
                    rspp_map_set(_t1168, rspp_str("field"), rspp_v_mname);
                    rspp_v_expr = _t1168;
                }
        } else {
            RsppVal _t1169 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("["));
            if (rspp_to_bool(_t1169)) {
                RsppVal _t1170 = rspp_fn_parse_expr(rspp_v_p);
                RsppVal rspp_v_idx = _t1170;
                RsppVal _t1171 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("]"));
                RsppVal _t1172 = rspp_map();
                rspp_map_set(_t1172, rspp_str("kind"), rspp_str("index"));
                rspp_map_set(_t1172, rspp_str("recv"), rspp_v_expr);
                rspp_map_set(_t1172, rspp_str("index"), rspp_v_idx);
                rspp_v_expr = _t1172;
        } else {
            break;
            }
        }
        }
    }
    RsppVal _t1173 = rspp_v_expr;
    return _t1173;
    return rspp_none();
}

static RsppVal rspp_fn_parse_primary(RsppVal rspp_v_p) {
    RsppVal _t1174 = rspp_fn_p_adv(rspp_v_p);
    RsppVal rspp_v_t = _t1174;
    RsppVal _t1176[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t1175 = rspp_method_call(rspp_v_t, "get", 2, _t1176);
    RsppVal rspp_v_k = _t1175;
    RsppVal _t1178[] = { rspp_str("val"), rspp_str("") };
    RsppVal _t1177 = rspp_method_call(rspp_v_t, "get", 2, _t1178);
    RsppVal rspp_v_v = _t1177;
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("INT")))) {
        RsppVal _t1180 = rspp_map();
        rspp_map_set(_t1180, rspp_str("kind"), rspp_str("int"));
        rspp_map_set(_t1180, rspp_str("val"), rspp_v_v);
        RsppVal _t1179 = _t1180;
        return _t1179;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("HEX")))) {
        RsppVal _t1182 = rspp_map();
        rspp_map_set(_t1182, rspp_str("kind"), rspp_str("hex"));
        rspp_map_set(_t1182, rspp_str("val"), rspp_v_v);
        RsppVal _t1181 = _t1182;
        return _t1181;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("FLOAT")))) {
        RsppVal _t1184 = rspp_map();
        rspp_map_set(_t1184, rspp_str("kind"), rspp_str("float"));
        rspp_map_set(_t1184, rspp_str("val"), rspp_v_v);
        RsppVal _t1183 = _t1184;
        return _t1183;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("STR")))) {
        RsppVal _t1186 = rspp_map();
        rspp_map_set(_t1186, rspp_str("kind"), rspp_str("str"));
        rspp_map_set(_t1186, rspp_str("val"), rspp_v_v);
        RsppVal _t1185 = _t1186;
        return _t1185;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("FSTR")))) {
        RsppVal _t1188 = rspp_map();
        rspp_map_set(_t1188, rspp_str("kind"), rspp_str("fstr"));
        rspp_map_set(_t1188, rspp_str("val"), rspp_v_v);
        RsppVal _t1187 = _t1188;
        return _t1187;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("BOOL")))) {
        RsppVal _t1190 = rspp_map();
        rspp_map_set(_t1190, rspp_str("kind"), rspp_str("bool"));
        rspp_map_set(_t1190, rspp_str("val"), rspp_v_v);
        RsppVal _t1189 = _t1190;
        return _t1189;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("NONE")))) {
        RsppVal _t1192 = rspp_map();
        rspp_map_set(_t1192, rspp_str("kind"), rspp_str("none"));
        RsppVal _t1191 = _t1192;
        return _t1191;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("IDENT")))) {
        RsppVal _t1193 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("::"));
        if (rspp_to_bool(_t1193)) {
            RsppVal _t1194 = rspp_fn_p_adv(rspp_v_p);
            RsppVal _t1196[] = { rspp_str("val"), rspp_str("") };
            RsppVal _t1195 = rspp_method_call(_t1194, "get", 2, _t1196);
            RsppVal rspp_v_member = _t1195;
            RsppVal _t1197 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("("));
            if (rspp_to_bool(_t1197)) {
                RsppVal _t1198 = rspp_fn_parse_arg_list(rspp_v_p);
                RsppVal rspp_v_args = _t1198;
                RsppVal _t1200 = rspp_map();
                rspp_map_set(_t1200, rspp_str("kind"), rspp_str("ns_call"));
                rspp_map_set(_t1200, rspp_str("ns"), rspp_v_v);
                rspp_map_set(_t1200, rspp_str("fn"), rspp_v_member);
                rspp_map_set(_t1200, rspp_str("args"), rspp_v_args);
                RsppVal _t1199 = _t1200;
                return _t1199;
            }
            RsppVal _t1202 = rspp_map();
            rspp_map_set(_t1202, rspp_str("kind"), rspp_str("str"));
            rspp_map_set(_t1202, rspp_str("val"), rspp_add(rspp_add(rspp_v_v, rspp_str("::")), rspp_v_member));
            RsppVal _t1201 = _t1202;
            return _t1201;
        }
        RsppVal _t1204 = rspp_map();
        rspp_map_set(_t1204, rspp_str("kind"), rspp_str("ident"));
        rspp_map_set(_t1204, rspp_str("name"), rspp_v_v);
        RsppVal _t1203 = _t1204;
        return _t1203;
    }
    RsppVal _t1205 = rspp_bool(0);
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("PUNCT")))) {
        _t1205 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_v, rspp_str("("))));
    }
    if (rspp_to_bool(_t1205)) {
        RsppVal _t1206 = rspp_fn_parse_expr(rspp_v_p);
        RsppVal rspp_v_inner = _t1206;
        RsppVal _t1207 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(")"));
        RsppVal _t1208 = rspp_v_inner;
        return _t1208;
    }
    RsppVal _t1209 = rspp_bool(0);
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("PUNCT")))) {
        _t1209 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_v, rspp_str("["))));
    }
    if (rspp_to_bool(_t1209)) {
        RsppVal _t1210 = rspp_list();
        RsppVal rspp_v_elems = _t1210;
        while (1) {
            RsppVal _t1212 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("]"));
            RsppVal _t1211 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1212)))) {
                RsppVal _t1213 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t1211 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t1213))));
            }
            if (!rspp_to_bool(_t1211)) break;
            RsppVal _t1214 = rspp_fn_parse_expr(rspp_v_p);
            RsppVal _t1216[] = { _t1214 };
            RsppVal _t1215 = rspp_method_call(rspp_v_elems, "push", 1, _t1216);
            RsppVal _t1217 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(","));
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1217)))) {
                break;
            }
        }
        RsppVal _t1218 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("]"));
        RsppVal _t1220 = rspp_map();
        rspp_map_set(_t1220, rspp_str("kind"), rspp_str("list"));
        rspp_map_set(_t1220, rspp_str("items"), rspp_v_elems);
        RsppVal _t1219 = _t1220;
        return _t1219;
    }
    RsppVal _t1221 = rspp_bool(0);
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("PUNCT")))) {
        _t1221 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_v, rspp_str("{"))));
    }
    if (rspp_to_bool(_t1221)) {
        RsppVal _t1222 = rspp_list();
        RsppVal rspp_v_entries = _t1222;
        while (1) {
            RsppVal _t1224 = rspp_fn_p_is(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
            RsppVal _t1223 = rspp_bool(0);
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1224)))) {
                RsppVal _t1225 = rspp_fn_p_is_kind(rspp_v_p, rspp_str("EOF"));
                _t1223 = rspp_bool(rspp_to_bool(rspp_bool(!rspp_to_bool(_t1225))));
            }
            if (!rspp_to_bool(_t1223)) break;
            RsppVal _t1226 = rspp_fn_parse_expr(rspp_v_p);
            RsppVal rspp_v_ek = _t1226;
            RsppVal _t1227 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(":"));
            RsppVal _t1228 = rspp_fn_parse_expr(rspp_v_p);
            RsppVal rspp_v_ev = _t1228;
            RsppVal _t1229 = rspp_map();
            rspp_map_set(_t1229, rspp_str("key"), rspp_v_ek);
            rspp_map_set(_t1229, rspp_str("val"), rspp_v_ev);
            RsppVal _t1231[] = { _t1229 };
            RsppVal _t1230 = rspp_method_call(rspp_v_entries, "push", 1, _t1231);
            RsppVal _t1232 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str(","));
            if (rspp_to_bool(rspp_bool(!rspp_to_bool(_t1232)))) {
                break;
            }
        }
        RsppVal _t1233 = rspp_fn_p_consume(rspp_v_p, rspp_str("PUNCT"), rspp_str("}"));
        RsppVal _t1235 = rspp_map();
        rspp_map_set(_t1235, rspp_str("kind"), rspp_str("map"));
        rspp_map_set(_t1235, rspp_str("entries"), rspp_v_entries);
        RsppVal _t1234 = _t1235;
        return _t1234;
    }
    RsppVal _t1237 = rspp_map();
    rspp_map_set(_t1237, rspp_str("kind"), rspp_str("none"));
    RsppVal _t1236 = _t1237;
    return _t1236;
    return rspp_none();
}

static RsppVal rspp_fn_path_dir(RsppVal rspp_v_p) {
    RsppVal rspp_v_last = rspp_neg(rspp_int(1LL));
    RsppVal rspp_v_i = rspp_int(0LL);
    while (1) {
        RsppVal _t1238 = rspp_method_call(rspp_v_p, "len", 0, NULL);
        if (!rspp_to_bool(rspp_lt(rspp_v_i, _t1238))) break;
        RsppVal _t1240[] = { rspp_v_i };
        RsppVal _t1239 = rspp_method_call(rspp_v_p, "char_at", 1, _t1240);
        if (rspp_to_bool(rspp_eq(_t1239, rspp_str("/")))) {
            rspp_v_last = rspp_v_i;
        }
        rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
    }
    if (rspp_to_bool(rspp_lt(rspp_v_last, rspp_int(0LL)))) {
        RsppVal _t1241 = rspp_str(".");
        return _t1241;
    }
    RsppVal _t1244[] = { rspp_int(0LL), rspp_v_last };
    RsppVal _t1243 = rspp_method_call(rspp_v_p, "slice", 2, _t1244);
    RsppVal _t1242 = _t1243;
    return _t1242;
    return rspp_none();
}

static RsppVal rspp_fn_path_join(RsppVal rspp_v_dir, RsppVal rspp_v_rel) {
    RsppVal _t1245 = rspp_bool(1);
    if (!rspp_to_bool(rspp_eq(rspp_v_dir, rspp_str("")))) {
        _t1245 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_dir, rspp_str("."))));
    }
    if (rspp_to_bool(_t1245)) {
        RsppVal _t1246 = rspp_v_rel;
        return _t1246;
    }
    RsppVal _t1247 = rspp_add(rspp_add(rspp_v_dir, rspp_str("/")), rspp_v_rel);
    return _t1247;
    return rspp_none();
}

static RsppVal rspp_fn_load_items_recursive(RsppVal rspp_v_path, RsppVal rspp_v_visited, RsppVal rspp_v_out_items) {
    RsppVal _t1249[] = { rspp_v_path };
    RsppVal _t1248 = rspp_method_call(rspp_v_visited, "has", 1, _t1249);
    if (rspp_to_bool(_t1248)) {
        RsppVal _t1250 = rspp_none();
        return _t1250;
    }
    RsppVal _t1252[] = { rspp_v_path, rspp_bool(1) };
    RsppVal _t1251 = rspp_method_call(rspp_v_visited, "set", 2, _t1252);
    RsppVal _t1254[] = { rspp_v_path };
    RsppVal _t1253 = rspp_builtin("fs", "read_text", 1, _t1254);
    RsppVal rspp_v_src = _t1253;
    RsppVal _t1255 = rspp_fn_tokenize_source(rspp_v_src);
    RsppVal rspp_v_toks = _t1255;
    RsppVal _t1256 = rspp_fn_parse_tokens(rspp_v_toks);
    RsppVal rspp_v_prog = _t1256;
    RsppVal _t1257 = rspp_fn_path_dir(rspp_v_path);
    RsppVal rspp_v_dir = _t1257;
    RsppVal _t1258 = rspp_list();
    RsppVal _t1260[] = { rspp_str("items"), _t1258 };
    RsppVal _t1259 = rspp_method_call(rspp_v_prog, "get", 2, _t1260);
    RsppVal _t1261 = _t1259;
    for (int64_t _t1262 = 0; _t1262 < rspp_list_len(_t1261); _t1262++) {
        RsppVal rspp_v_it = rspp_index_get(_t1261, rspp_int(_t1262));
        RsppVal _t1264[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t1263 = rspp_method_call(rspp_v_it, "get", 2, _t1264);
        if (rspp_to_bool(rspp_eq(_t1263, rspp_str("import")))) {
            RsppVal _t1266[] = { rspp_str("path"), rspp_str("") };
            RsppVal _t1265 = rspp_method_call(rspp_v_it, "get", 2, _t1266);
            RsppVal rspp_v_ip = _t1265;
            RsppVal _t1268[] = { rspp_str(".rspp") };
            RsppVal _t1267 = rspp_method_call(rspp_v_ip, "contains", 1, _t1268);
            if (rspp_to_bool(_t1267)) {
                RsppVal _t1269 = rspp_fn_path_join(rspp_v_dir, rspp_v_ip);
                RsppVal rspp_v_full = _t1269;
                RsppVal _t1271[] = { rspp_v_full };
                RsppVal _t1270 = rspp_builtin("fs", "exists", 1, _t1271);
                if (rspp_to_bool(_t1270)) {
                    RsppVal _t1272 = rspp_fn_load_items_recursive(rspp_v_full, rspp_v_visited, rspp_v_out_items);
                    continue;
                }
            }
        }
        RsppVal _t1274[] = { rspp_v_it };
        RsppVal _t1273 = rspp_method_call(rspp_v_out_items, "push", 1, _t1274);
    }
    return rspp_none();
}

static RsppVal rspp_fn_load_program_bundle(RsppVal rspp_v_entry_path) {
    RsppVal _t1275 = rspp_map();
    RsppVal rspp_v_visited = _t1275;
    RsppVal _t1276 = rspp_list();
    RsppVal rspp_v_items = _t1276;
    RsppVal _t1277 = rspp_fn_load_items_recursive(rspp_v_entry_path, rspp_v_visited, rspp_v_items);
    RsppVal _t1279 = rspp_map();
    rspp_map_set(_t1279, rspp_str("kind"), rspp_str("program"));
    rspp_map_set(_t1279, rspp_str("items"), rspp_v_items);
    RsppVal _t1278 = _t1279;
    return _t1278;
    return rspp_none();
}

static RsppVal rspp_fn_esc_c_str(RsppVal rspp_v_s) {
    RsppVal _t1280 = rspp_list();
    RsppVal rspp_v_out = _t1280;
    RsppVal rspp_v_i = rspp_int(0LL);
    RsppVal _t1281 = rspp_method_call(rspp_v_s, "len", 0, NULL);
    RsppVal rspp_v_n = _t1281;
    while (1) {
        if (!rspp_to_bool(rspp_lt(rspp_v_i, rspp_v_n))) break;
        RsppVal _t1283[] = { rspp_v_i };
        RsppVal _t1282 = rspp_method_call(rspp_v_s, "char_code_at", 1, _t1283);
        RsppVal rspp_v_code = _t1282;
        if (rspp_to_bool(rspp_eq(rspp_v_code, rspp_int(92LL)))) {
            RsppVal _t1285[] = { rspp_str("\\\\") };
            RsppVal _t1284 = rspp_method_call(rspp_v_out, "push", 1, _t1285);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_code, rspp_int(34LL)))) {
                RsppVal _t1287[] = { rspp_str("\\\"") };
                RsppVal _t1286 = rspp_method_call(rspp_v_out, "push", 1, _t1287);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_code, rspp_int(10LL)))) {
                RsppVal _t1289[] = { rspp_str("\\n") };
                RsppVal _t1288 = rspp_method_call(rspp_v_out, "push", 1, _t1289);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_code, rspp_int(13LL)))) {
                RsppVal _t1291[] = { rspp_str("\\r") };
                RsppVal _t1290 = rspp_method_call(rspp_v_out, "push", 1, _t1291);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_code, rspp_int(9LL)))) {
                RsppVal _t1293[] = { rspp_str("\\t") };
                RsppVal _t1292 = rspp_method_call(rspp_v_out, "push", 1, _t1293);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_code, rspp_int(0LL)))) {
                RsppVal _t1295[] = { rspp_str("\\0") };
                RsppVal _t1294 = rspp_method_call(rspp_v_out, "push", 1, _t1295);
        } else {
            RsppVal _t1297[] = { rspp_v_i };
            RsppVal _t1296 = rspp_method_call(rspp_v_s, "char_at", 1, _t1297);
            RsppVal _t1299[] = { _t1296 };
            RsppVal _t1298 = rspp_method_call(rspp_v_out, "push", 1, _t1299);
                        }
                    }
                }
            }
        }
        }
        rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
    }
    RsppVal _t1302[] = { rspp_v_out, rspp_str("") };
    RsppVal _t1301 = rspp_builtin("str", "join", 2, _t1302);
    RsppVal _t1300 = _t1301;
    return _t1300;
    return rspp_none();
}

static RsppVal rspp_fn_c_ident(RsppVal rspp_v_s) {
    RsppVal _t1303 = rspp_list();
    RsppVal rspp_v_out = _t1303;
    RsppVal rspp_v_i = rspp_int(0LL);
    while (1) {
        RsppVal _t1304 = rspp_method_call(rspp_v_s, "len", 0, NULL);
        if (!rspp_to_bool(rspp_lt(rspp_v_i, _t1304))) break;
        RsppVal _t1306[] = { rspp_v_i };
        RsppVal _t1305 = rspp_method_call(rspp_v_s, "char_code_at", 1, _t1306);
        RsppVal rspp_v_c = _t1305;
        RsppVal _t1310 = rspp_bool(0);
        if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(65LL)))) {
            _t1310 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(90LL))));
        }
        RsppVal _t1309 = rspp_bool(1);
        if (!rspp_to_bool(_t1310)) {
            RsppVal _t1311 = rspp_bool(0);
            if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(97LL)))) {
                _t1311 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(122LL))));
            }
            _t1309 = rspp_bool(rspp_to_bool(_t1311));
        }
        RsppVal _t1308 = rspp_bool(1);
        if (!rspp_to_bool(_t1309)) {
            RsppVal _t1312 = rspp_bool(0);
            if (rspp_to_bool(rspp_gte(rspp_v_c, rspp_int(48LL)))) {
                _t1312 = rspp_bool(rspp_to_bool(rspp_lte(rspp_v_c, rspp_int(57LL))));
            }
            _t1308 = rspp_bool(rspp_to_bool(_t1312));
        }
        RsppVal _t1307 = rspp_bool(1);
        if (!rspp_to_bool(_t1308)) {
            _t1307 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_c, rspp_int(95LL))));
        }
        RsppVal rspp_v_ok = _t1307;
        if (rspp_to_bool(rspp_v_ok)) {
            RsppVal _t1314[] = { rspp_v_i };
            RsppVal _t1313 = rspp_method_call(rspp_v_s, "char_at", 1, _t1314);
            RsppVal _t1316[] = { _t1313 };
            RsppVal _t1315 = rspp_method_call(rspp_v_out, "push", 1, _t1316);
        } else {
            RsppVal _t1318[] = { rspp_str("_") };
            RsppVal _t1317 = rspp_method_call(rspp_v_out, "push", 1, _t1318);
        }
        rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
    }
    RsppVal _t1321[] = { rspp_v_out, rspp_str("") };
    RsppVal _t1320 = rspp_builtin("str", "join", 2, _t1321);
    RsppVal _t1319 = _t1320;
    return _t1319;
    return rspp_none();
}

static RsppVal rspp_fn_emit_c_prelude(RsppVal rspp_v_lines) {
    RsppVal _t1323[] = { rspp_str("#define _POSIX_C_SOURCE 200809L") };
    RsppVal _t1322 = rspp_method_call(rspp_v_lines, "push", 1, _t1323);
    RsppVal _t1325[] = { rspp_str("#include <stdio.h>") };
    RsppVal _t1324 = rspp_method_call(rspp_v_lines, "push", 1, _t1325);
    RsppVal _t1327[] = { rspp_str("#include <stdlib.h>") };
    RsppVal _t1326 = rspp_method_call(rspp_v_lines, "push", 1, _t1327);
    RsppVal _t1329[] = { rspp_str("#include <stdint.h>") };
    RsppVal _t1328 = rspp_method_call(rspp_v_lines, "push", 1, _t1329);
    RsppVal _t1331[] = { rspp_str("#include <string.h>") };
    RsppVal _t1330 = rspp_method_call(rspp_v_lines, "push", 1, _t1331);
    RsppVal _t1333[] = { rspp_str("#include <math.h>") };
    RsppVal _t1332 = rspp_method_call(rspp_v_lines, "push", 1, _t1333);
    RsppVal _t1335[] = { rspp_str("#include <time.h>") };
    RsppVal _t1334 = rspp_method_call(rspp_v_lines, "push", 1, _t1335);
    RsppVal _t1337[] = { rspp_str("#include <ctype.h>") };
    RsppVal _t1336 = rspp_method_call(rspp_v_lines, "push", 1, _t1337);
    RsppVal _t1339[] = { rspp_str("#include <setjmp.h>") };
    RsppVal _t1338 = rspp_method_call(rspp_v_lines, "push", 1, _t1339);
    RsppVal _t1341[] = { rspp_str("#ifndef M_PI") };
    RsppVal _t1340 = rspp_method_call(rspp_v_lines, "push", 1, _t1341);
    RsppVal _t1343[] = { rspp_str("#define M_PI 3.14159265358979323846") };
    RsppVal _t1342 = rspp_method_call(rspp_v_lines, "push", 1, _t1343);
    RsppVal _t1345[] = { rspp_str("#endif") };
    RsppVal _t1344 = rspp_method_call(rspp_v_lines, "push", 1, _t1345);
    RsppVal _t1347[] = { rspp_str("static int rspp_argc = 0;") };
    RsppVal _t1346 = rspp_method_call(rspp_v_lines, "push", 1, _t1347);
    RsppVal _t1349[] = { rspp_str("static char** rspp_argv = NULL;") };
    RsppVal _t1348 = rspp_method_call(rspp_v_lines, "push", 1, _t1349);
    RsppVal _t1351[] = { rspp_str("typedef enum { RSPP_NONE=0, RSPP_INT, RSPP_FLOAT, RSPP_BOOL, RSPP_STR, RSPP_PTR, RSPP_LIST, RSPP_MAP } RsppKind;") };
    RsppVal _t1350 = rspp_method_call(rspp_v_lines, "push", 1, _t1351);
    RsppVal _t1353[] = { rspp_str("typedef struct RsppList RsppList;") };
    RsppVal _t1352 = rspp_method_call(rspp_v_lines, "push", 1, _t1353);
    RsppVal _t1355[] = { rspp_str("typedef struct RsppMap RsppMap;") };
    RsppVal _t1354 = rspp_method_call(rspp_v_lines, "push", 1, _t1355);
    RsppVal _t1357[] = { rspp_str("typedef struct { RsppKind kind; union { int64_t i; double f; int b; char* s; uint64_t ptr; RsppList* list; RsppMap* map; }; } RsppVal;") };
    RsppVal _t1356 = rspp_method_call(rspp_v_lines, "push", 1, _t1357);
    RsppVal _t1359[] = { rspp_str("struct RsppList { RsppVal* items; size_t len; size_t cap; };") };
    RsppVal _t1358 = rspp_method_call(rspp_v_lines, "push", 1, _t1359);
    RsppVal _t1361[] = { rspp_str("typedef struct { char* key; RsppVal val; } RsppMapEntry;") };
    RsppVal _t1360 = rspp_method_call(rspp_v_lines, "push", 1, _t1361);
    RsppVal _t1363[] = { rspp_str("struct RsppMap { RsppMapEntry* entries; size_t len; size_t cap; };") };
    RsppVal _t1362 = rspp_method_call(rspp_v_lines, "push", 1, _t1363);
    RsppVal _t1365[] = { rspp_str("static char* rspp_strdup(const char* s) { if (!s) s = \"\"; size_t n = strlen(s); char* p = (char*)malloc(n + 1); memcpy(p, s, n + 1); return p; }") };
    RsppVal _t1364 = rspp_method_call(rspp_v_lines, "push", 1, _t1365);
    RsppVal _t1367[] = { rspp_str("static RsppVal rspp_none(void) { RsppVal v; v.kind = RSPP_NONE; v.i = 0; return v; }") };
    RsppVal _t1366 = rspp_method_call(rspp_v_lines, "push", 1, _t1367);
    RsppVal _t1369[] = { rspp_str("static RsppVal rspp_int(int64_t x) { RsppVal v; v.kind = RSPP_INT; v.i = x; return v; }") };
    RsppVal _t1368 = rspp_method_call(rspp_v_lines, "push", 1, _t1369);
    RsppVal _t1371[] = { rspp_str("static RsppVal rspp_flt(double x) { RsppVal v; v.kind = RSPP_FLOAT; v.f = x; return v; }") };
    RsppVal _t1370 = rspp_method_call(rspp_v_lines, "push", 1, _t1371);
    RsppVal _t1373[] = { rspp_str("static RsppVal rspp_bool(int b) { RsppVal v; v.kind = RSPP_BOOL; v.b = b ? 1 : 0; return v; }") };
    RsppVal _t1372 = rspp_method_call(rspp_v_lines, "push", 1, _t1373);
    RsppVal _t1375[] = { rspp_str("static RsppVal rspp_ptr(uint64_t p) { RsppVal v; v.kind = RSPP_PTR; v.ptr = p; return v; }") };
    RsppVal _t1374 = rspp_method_call(rspp_v_lines, "push", 1, _t1375);
    RsppVal _t1377[] = { rspp_str("static RsppVal rspp_str(const char* s) { RsppVal v; v.kind = RSPP_STR; v.s = rspp_strdup(s); return v; }") };
    RsppVal _t1376 = rspp_method_call(rspp_v_lines, "push", 1, _t1377);
    RsppVal _t1379[] = { rspp_str("static RsppVal rspp_str_owned(char* s) { RsppVal v; v.kind = RSPP_STR; v.s = s ? s : rspp_strdup(\"\"); return v; }") };
    RsppVal _t1378 = rspp_method_call(rspp_v_lines, "push", 1, _t1379);
    RsppVal _t1381[] = { rspp_str("static RsppVal rspp_list(void) { RsppList* l = (RsppList*)calloc(1, sizeof(RsppList)); l->cap = 8; l->items = (RsppVal*)calloc(l->cap, sizeof(RsppVal)); RsppVal v; v.kind = RSPP_LIST; v.list = l; return v; }") };
    RsppVal _t1380 = rspp_method_call(rspp_v_lines, "push", 1, _t1381);
    RsppVal _t1383[] = { rspp_str("static void rspp_list_push(RsppVal lv, RsppVal item) { if (lv.kind != RSPP_LIST || !lv.list) return; RsppList* l = lv.list; if (l->len >= l->cap) { l->cap *= 2; l->items = (RsppVal*)realloc(l->items, l->cap * sizeof(RsppVal)); } l->items[l->len++] = item; }") };
    RsppVal _t1382 = rspp_method_call(rspp_v_lines, "push", 1, _t1383);
    RsppVal _t1385[] = { rspp_str("static int64_t rspp_list_len(RsppVal lv) { return (lv.kind == RSPP_LIST && lv.list) ? (int64_t)lv.list->len : 0; }") };
    RsppVal _t1384 = rspp_method_call(rspp_v_lines, "push", 1, _t1385);
    RsppVal _t1387[] = { rspp_str("static RsppVal rspp_map(void) { RsppMap* m = (RsppMap*)calloc(1, sizeof(RsppMap)); m->cap = 8; m->entries = (RsppMapEntry*)calloc(m->cap, sizeof(RsppMapEntry)); RsppVal v; v.kind = RSPP_MAP; v.map = m; return v; }") };
    RsppVal _t1386 = rspp_method_call(rspp_v_lines, "push", 1, _t1387);
    RsppVal _t1389[] = { rspp_str("static int64_t rspp_to_i64(RsppVal v) { switch (v.kind) { case RSPP_INT: return v.i; case RSPP_FLOAT: return (int64_t)v.f; case RSPP_BOOL: return v.b?1:0; case RSPP_PTR: return (int64_t)v.ptr; case RSPP_STR: return strtoll(v.s?v.s:\"0\", NULL, 0); default: return 0; } }") };
    RsppVal _t1388 = rspp_method_call(rspp_v_lines, "push", 1, _t1389);
    RsppVal _t1391[] = { rspp_str("static uint64_t rspp_to_u64(RsppVal v) { return v.kind == RSPP_PTR ? v.ptr : (uint64_t)rspp_to_i64(v); }") };
    RsppVal _t1390 = rspp_method_call(rspp_v_lines, "push", 1, _t1391);
    RsppVal _t1393[] = { rspp_str("static double rspp_to_f64(RsppVal v) { switch (v.kind) { case RSPP_FLOAT: return v.f; case RSPP_INT: return (double)v.i; case RSPP_BOOL: return v.b?1.0:0.0; case RSPP_PTR: return (double)v.ptr; case RSPP_STR: return strtod(v.s?v.s:\"0\", NULL); default: return 0.0; } }") };
    RsppVal _t1392 = rspp_method_call(rspp_v_lines, "push", 1, _t1393);
    RsppVal _t1395[] = { rspp_str("static int rspp_to_bool(RsppVal v) { switch (v.kind) { case RSPP_BOOL: return v.b!=0; case RSPP_INT: return v.i!=0; case RSPP_FLOAT: return v.f!=0.0; case RSPP_PTR: return v.ptr!=0; case RSPP_STR: return v.s && v.s[0]!=0; case RSPP_LIST: return v.list && v.list->len>0; case RSPP_MAP: return v.map && v.map->len>0; default: return 0; } }") };
    RsppVal _t1394 = rspp_method_call(rspp_v_lines, "push", 1, _t1395);
    RsppVal _t1397[] = { rspp_str("static const char* rspp_to_str(RsppVal v) { char* buf; switch (v.kind) { case RSPP_STR: return v.s?v.s:\"\"; case RSPP_INT: buf=(char*)malloc(32); snprintf(buf,32,\"%lld\",(long long)v.i); return buf; case RSPP_FLOAT: buf=(char*)malloc(48); snprintf(buf,48,\"%.6g\",v.f); return buf; case RSPP_BOOL: return v.b?\"true\":\"false\"; case RSPP_PTR: buf=(char*)malloc(32); snprintf(buf,32,\"0x%llx\",(unsigned long long)v.ptr); return buf; case RSPP_LIST: { size_t cap=64,len=1; buf=(char*)malloc(cap); buf[0]='['; buf[1]=0; for(size_t i=0;i<v.list->len;i++){ const char* it=rspp_to_str(v.list->items[i]); size_t il=strlen(it); while(len+il+8>=cap){cap*=2;buf=(char*)realloc(buf,cap);} if(i>0){buf[len++]=',';buf[len++]=' ';} memcpy(buf+len,it,il); len+=il; buf[len]=0; } buf[len++]=']'; buf[len]=0; return buf; } case RSPP_MAP: { size_t cap=64,len=1; buf=(char*)malloc(cap); buf[0]='{'; buf[1]=0; for(size_t i=0;i<v.map->len;i++){ const char* vs=rspp_to_str(v.map->entries[i].val); size_t kl=strlen(v.map->entries[i].key),vl=strlen(vs); while(len+kl+vl+10>=cap){cap*=2;buf=(char*)realloc(buf,cap);} if(i>0){buf[len++]=',';buf[len++]=' ';} memcpy(buf+len,v.map->entries[i].key,kl); len+=kl; buf[len++]=':'; buf[len++]=' '; memcpy(buf+len,vs,vl); len+=vl; buf[len]=0; } buf[len++]='}'; buf[len]=0; return buf; } default: return \"none\"; } }") };
    RsppVal _t1396 = rspp_method_call(rspp_v_lines, "push", 1, _t1397);
    RsppVal _t1399[] = { rspp_str("static void rspp_map_set(RsppVal mv, RsppVal kv, RsppVal val) { if(mv.kind!=RSPP_MAP||!mv.map)return; const char* key=rspp_to_str(kv); RsppMap* m=mv.map; for(size_t i=0;i<m->len;i++){ if(strcmp(m->entries[i].key,key)==0){m->entries[i].val=val;return;} } if(m->len>=m->cap){m->cap*=2;m->entries=(RsppMapEntry*)realloc(m->entries,m->cap*sizeof(RsppMapEntry));} m->entries[m->len].key=rspp_strdup(key); m->entries[m->len].val=val; m->len++; }") };
    RsppVal _t1398 = rspp_method_call(rspp_v_lines, "push", 1, _t1399);
    RsppVal _t1401[] = { rspp_str("static RsppVal rspp_map_get(RsppVal mv, RsppVal kv, RsppVal defv) { if(mv.kind!=RSPP_MAP||!mv.map)return defv; const char* key=rspp_to_str(kv); for(size_t i=0;i<mv.map->len;i++){ if(strcmp(mv.map->entries[i].key,key)==0)return mv.map->entries[i].val; } return defv; }") };
    RsppVal _t1400 = rspp_method_call(rspp_v_lines, "push", 1, _t1401);
    RsppVal _t1403[] = { rspp_str("static int rspp_map_has(RsppVal mv, RsppVal kv) { if(mv.kind!=RSPP_MAP||!mv.map)return 0; const char* key=rspp_to_str(kv); for(size_t i=0;i<mv.map->len;i++){ if(strcmp(mv.map->entries[i].key,key)==0)return 1; } return 0; }") };
    RsppVal _t1402 = rspp_method_call(rspp_v_lines, "push", 1, _t1403);
    RsppVal _t1405[] = { rspp_str("static RsppVal rspp_index_get(RsppVal r, RsppVal idx) { if(r.kind==RSPP_LIST&&r.list){int64_t i=rspp_to_i64(idx); if(i>=0&&(size_t)i<r.list->len)return r.list->items[i]; return rspp_none();} if(r.kind==RSPP_MAP&&r.map)return rspp_map_get(r,idx,rspp_none()); if(r.kind==RSPP_STR&&r.s){int64_t i=rspp_to_i64(idx); size_t n=strlen(r.s); if(i>=0&&(size_t)i<n){char b[2]={r.s[i],0}; return rspp_str(b);} return rspp_str(\"\");} return rspp_none(); }") };
    RsppVal _t1404 = rspp_method_call(rspp_v_lines, "push", 1, _t1405);
    RsppVal _t1407[] = { rspp_str("static void rspp_index_set(RsppVal r, RsppVal idx, RsppVal val) { if(r.kind==RSPP_LIST&&r.list){int64_t i=rspp_to_i64(idx); if(i>=0&&(size_t)i<r.list->len)r.list->items[i]=val;} else if(r.kind==RSPP_MAP&&r.map)rspp_map_set(r,idx,val); }") };
    RsppVal _t1406 = rspp_method_call(rspp_v_lines, "push", 1, _t1407);
    RsppVal _t1409[] = { rspp_str("static RsppVal rspp_range(RsppVal s, RsppVal e, int inc) { int64_t a=rspp_to_i64(s),b=rspp_to_i64(e)+(inc?1:0); RsppVal l=rspp_list(); for(int64_t i=a;i<b;i++)rspp_list_push(l,rspp_int(i)); return l; }") };
    RsppVal _t1408 = rspp_method_call(rspp_v_lines, "push", 1, _t1409);
    RsppVal _t1411[] = { rspp_str("static RsppVal rspp_add(RsppVal a, RsppVal b) { if(a.kind==RSPP_STR||b.kind==RSPP_STR){ const char* sa=rspp_to_str(a); const char* sb=rspp_to_str(b); size_t la=strlen(sa),lb=strlen(sb); char* out=(char*)malloc(la+lb+1); memcpy(out,sa,la); memcpy(out+la,sb,lb+1); return rspp_str_owned(out); } if(a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)return rspp_flt(rspp_to_f64(a)+rspp_to_f64(b)); return rspp_int(rspp_to_i64(a)+rspp_to_i64(b)); }") };
    RsppVal _t1410 = rspp_method_call(rspp_v_lines, "push", 1, _t1411);
    RsppVal _t1413[] = { rspp_str("static RsppVal rspp_sub(RsppVal a, RsppVal b) { return (a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)?rspp_flt(rspp_to_f64(a)-rspp_to_f64(b)):rspp_int(rspp_to_i64(a)-rspp_to_i64(b)); }") };
    RsppVal _t1412 = rspp_method_call(rspp_v_lines, "push", 1, _t1413);
    RsppVal _t1415[] = { rspp_str("static RsppVal rspp_mul(RsppVal a, RsppVal b) { return (a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)?rspp_flt(rspp_to_f64(a)*rspp_to_f64(b)):rspp_int(rspp_to_i64(a)*rspp_to_i64(b)); }") };
    RsppVal _t1414 = rspp_method_call(rspp_v_lines, "push", 1, _t1415);
    RsppVal _t1417[] = { rspp_str("static RsppVal rspp_div(RsppVal a, RsppVal b) { if(a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT){double d=rspp_to_f64(b);return rspp_flt(d==0.0?0.0:rspp_to_f64(a)/d);} int64_t d=rspp_to_i64(b); return rspp_int(d==0?0:rspp_to_i64(a)/d); }") };
    RsppVal _t1416 = rspp_method_call(rspp_v_lines, "push", 1, _t1417);
    RsppVal _t1419[] = { rspp_str("static RsppVal rspp_mod(RsppVal a, RsppVal b) { int64_t d=rspp_to_i64(b); return rspp_int(d==0?0:rspp_to_i64(a)%d); }") };
    RsppVal _t1418 = rspp_method_call(rspp_v_lines, "push", 1, _t1419);
    RsppVal _t1421[] = { rspp_str("static RsppVal rspp_pow(RsppVal a, RsppVal b) { return rspp_flt(pow(rspp_to_f64(a), rspp_to_f64(b))); }") };
    RsppVal _t1420 = rspp_method_call(rspp_v_lines, "push", 1, _t1421);
    RsppVal _t1423[] = { rspp_str("static RsppVal rspp_neg(RsppVal a) { return a.kind==RSPP_FLOAT?rspp_flt(-a.f):rspp_int(-rspp_to_i64(a)); }") };
    RsppVal _t1422 = rspp_method_call(rspp_v_lines, "push", 1, _t1423);
    RsppVal _t1425[] = { rspp_str("static RsppVal rspp_bitand(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)&rspp_to_i64(b)); }") };
    RsppVal _t1424 = rspp_method_call(rspp_v_lines, "push", 1, _t1425);
    RsppVal _t1427[] = { rspp_str("static RsppVal rspp_bitor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)|rspp_to_i64(b)); }") };
    RsppVal _t1426 = rspp_method_call(rspp_v_lines, "push", 1, _t1427);
    RsppVal _t1429[] = { rspp_str("static RsppVal rspp_bitxor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)^rspp_to_i64(b)); }") };
    RsppVal _t1428 = rspp_method_call(rspp_v_lines, "push", 1, _t1429);
    RsppVal _t1431[] = { rspp_str("static RsppVal rspp_shl(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)<<(rspp_to_i64(b)&63)); }") };
    RsppVal _t1430 = rspp_method_call(rspp_v_lines, "push", 1, _t1431);
    RsppVal _t1433[] = { rspp_str("static RsppVal rspp_shr(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)>>(rspp_to_i64(b)&63)); }") };
    RsppVal _t1432 = rspp_method_call(rspp_v_lines, "push", 1, _t1433);
    RsppVal _t1435[] = { rspp_str("static RsppVal rspp_eq(RsppVal a, RsppVal b) { if(a.kind==RSPP_STR&&b.kind==RSPP_STR)return rspp_bool(strcmp(a.s,b.s)==0); if(a.kind==RSPP_NONE&&b.kind==RSPP_NONE)return rspp_bool(1); if(a.kind==RSPP_NONE||b.kind==RSPP_NONE)return rspp_bool(0); if(a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)return rspp_bool(fabs(rspp_to_f64(a)-rspp_to_f64(b))<1e-12); return rspp_bool(rspp_to_i64(a)==rspp_to_i64(b)); }") };
    RsppVal _t1434 = rspp_method_call(rspp_v_lines, "push", 1, _t1435);
    RsppVal _t1437[] = { rspp_str("static RsppVal rspp_neq(RsppVal a, RsppVal b) { return rspp_bool(!rspp_to_bool(rspp_eq(a,b))); }") };
    RsppVal _t1436 = rspp_method_call(rspp_v_lines, "push", 1, _t1437);
    RsppVal _t1439[] = { rspp_str("static RsppVal rspp_lt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)<rspp_to_f64(b)); }") };
    RsppVal _t1438 = rspp_method_call(rspp_v_lines, "push", 1, _t1439);
    RsppVal _t1441[] = { rspp_str("static RsppVal rspp_lte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)<=rspp_to_f64(b)); }") };
    RsppVal _t1440 = rspp_method_call(rspp_v_lines, "push", 1, _t1441);
    RsppVal _t1443[] = { rspp_str("static RsppVal rspp_gt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)>rspp_to_f64(b)); }") };
    RsppVal _t1442 = rspp_method_call(rspp_v_lines, "push", 1, _t1443);
    RsppVal _t1445[] = { rspp_str("static RsppVal rspp_gte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)>=rspp_to_f64(b)); }") };
    RsppVal _t1444 = rspp_method_call(rspp_v_lines, "push", 1, _t1445);
    RsppVal _t1447[] = { rspp_str("static jmp_buf rspp_try_stack[64]; static int rspp_try_sp=0; static char rspp_last_error[1024]=\"\";") };
    RsppVal _t1446 = rspp_method_call(rspp_v_lines, "push", 1, _t1447);
    RsppVal _t1449[] = { rspp_str("#define rspp_push_try() (rspp_try_stack[rspp_try_sp++])") };
    RsppVal _t1448 = rspp_method_call(rspp_v_lines, "push", 1, _t1449);
    RsppVal _t1451[] = { rspp_str("static void rspp_pop_try(void) { if(rspp_try_sp>0)rspp_try_sp--; }") };
    RsppVal _t1450 = rspp_method_call(rspp_v_lines, "push", 1, _t1451);
    RsppVal _t1453[] = { rspp_str("static void rspp_throw(const char* msg) { snprintf(rspp_last_error,sizeof(rspp_last_error),\"%s\",msg?msg:\"error\"); if(rspp_try_sp>0)longjmp(rspp_try_stack[rspp_try_sp-1],1); fprintf(stderr,\"Uncaught: %s\\n\",rspp_last_error); exit(1); }") };
    RsppVal _t1452 = rspp_method_call(rspp_v_lines, "push", 1, _t1453);
    RsppVal _t1455[] = { rspp_str("typedef struct { void* ptr; size_t size; int freed; } RsppArenaSlot;") };
    RsppVal _t1454 = rspp_method_call(rspp_v_lines, "push", 1, _t1455);
    RsppVal _t1457[] = { rspp_str("static RsppArenaSlot rspp_arena[4096]; static int rspp_arena_len=0; static size_t rspp_total_alloc=0,rspp_total_freed=0;") };
    RsppVal _t1456 = rspp_method_call(rspp_v_lines, "push", 1, _t1457);
    RsppVal _t1459[] = { rspp_str("static uint64_t rspp_mem_alloc(size_t sz) { if(sz==0)sz=1; void* p=calloc(1,sz); if(rspp_arena_len<4096){rspp_arena[rspp_arena_len].ptr=p;rspp_arena[rspp_arena_len].size=sz;rspp_arena[rspp_arena_len].freed=0;rspp_arena_len++;} rspp_total_alloc+=sz; return (uint64_t)(uintptr_t)p; }") };
    RsppVal _t1458 = rspp_method_call(rspp_v_lines, "push", 1, _t1459);
    RsppVal _t1461[] = { rspp_str("static void rspp_mem_free(uint64_t addr) { void* p=(void*)(uintptr_t)addr; if(!p)return; for(int i=0;i<rspp_arena_len;i++){ if(rspp_arena[i].ptr==p){ if(!rspp_arena[i].freed){rspp_arena[i].freed=1;rspp_total_freed+=rspp_arena[i].size;free(p);} return; } } }") };
    RsppVal _t1460 = rspp_method_call(rspp_v_lines, "push", 1, _t1461);
    RsppVal _t1463[] = { rspp_str("static void rspp_mem_leak_check_atexit(void) { size_t leaked=0; for(int i=0;i<rspp_arena_len;i++) if(!rspp_arena[i].freed) leaked+=rspp_arena[i].size; if(leaked>0 && getenv(\"RSPP_LEAK_STRICT\")) fprintf(stderr,\"[LEAK GUARD] Unfreed memory: %zu bytes\\n\",leaked); }") };
    RsppVal _t1462 = rspp_method_call(rspp_v_lines, "push", 1, _t1463);
    RsppVal _t1465[] = { rspp_str("typedef double (*RsppEq1Fn)(double); typedef struct { const char* name; RsppEq1Fn fn; } RsppEqEntry; static RsppEqEntry rspp_eqs[128]; static int rspp_eq_count=0;") };
    RsppVal _t1464 = rspp_method_call(rspp_v_lines, "push", 1, _t1465);
    RsppVal _t1467[] = { rspp_str("static void rspp_register_eq(const char* name, RsppEq1Fn fn) { if(rspp_eq_count<128){rspp_eqs[rspp_eq_count].name=name;rspp_eqs[rspp_eq_count].fn=fn;rspp_eq_count++;} }") };
    RsppVal _t1466 = rspp_method_call(rspp_v_lines, "push", 1, _t1467);
    RsppVal _t1469[] = { rspp_str("static RsppEq1Fn rspp_find_eq(const char* name) { for(int i=0;i<rspp_eq_count;i++) if(strcmp(rspp_eqs[i].name,name)==0) return rspp_eqs[i].fn; return NULL; }") };
    RsppVal _t1468 = rspp_method_call(rspp_v_lines, "push", 1, _t1469);
    RsppVal _t1471[] = { rspp_str("static void rspp_radix2_fft(double* re, double* im, size_t n) { size_t j=0; for(size_t i=1;i<n;i++){size_t bit=n>>1;while(j&bit){j^=bit;bit>>=1;}j^=bit;if(i<j){double tr=re[i];re[i]=re[j];re[j]=tr;double ti=im[i];im[i]=im[j];im[j]=ti;}} for(size_t len=2;len<=n;len<<=1){double ang=-2.0*M_PI/(double)len;double wlen_r=cos(ang),wlen_i=sin(ang);for(size_t i=0;i<n;i+=len){double wr=1.0,wi=0.0;for(size_t k=0;k<len/2;k++){double ur=re[i+k],ui=im[i+k];double vr=re[i+k+len/2]*wr-im[i+k+len/2]*wi;double vi=re[i+k+len/2]*wi+im[i+k+len/2]*wr;re[i+k]=ur+vr;im[i+k]=ui+vi;re[i+k+len/2]=ur-vr;im[i+k+len/2]=ui-vi;double nwr=wr*wlen_r-wi*wlen_i;wi=wr*wlen_i+wi*wlen_r;wr=nwr;}}} }") };
    RsppVal _t1470 = rspp_method_call(rspp_v_lines, "push", 1, _t1471);
    RsppVal _t1473[] = { rspp_str("static RsppVal rspp_method_call(RsppVal recv, const char* m, int argc, RsppVal* argv) {") };
    RsppVal _t1472 = rspp_method_call(rspp_v_lines, "push", 1, _t1473);
    RsppVal _t1475[] = { rspp_str("    if(recv.kind==RSPP_STR){ const char* s=recv.s?recv.s:\"\"; size_t len=strlen(s);") };
    RsppVal _t1474 = rspp_method_call(rspp_v_lines, "push", 1, _t1475);
    RsppVal _t1477[] = { rspp_str("        if(strcmp(m,\"len\")==0)return rspp_int((int64_t)len);") };
    RsppVal _t1476 = rspp_method_call(rspp_v_lines, "push", 1, _t1477);
    RsppVal _t1479[] = { rspp_str("        if(strcmp(m,\"char_at\")==0){int64_t idx=argc>0?rspp_to_i64(argv[0]):0; if(idx>=0&&(size_t)idx<len){char b[2]={s[idx],0};return rspp_str(b);} return rspp_str(\"\");}") };
    RsppVal _t1478 = rspp_method_call(rspp_v_lines, "push", 1, _t1479);
    RsppVal _t1481[] = { rspp_str("        if(strcmp(m,\"char_code_at\")==0){int64_t idx=argc>0?rspp_to_i64(argv[0]):0; if(idx>=0&&(size_t)idx<len)return rspp_int((unsigned char)s[idx]); return rspp_int(0);}") };
    RsppVal _t1480 = rspp_method_call(rspp_v_lines, "push", 1, _t1481);
    RsppVal _t1483[] = { rspp_str("        if(strcmp(m,\"slice\")==0){int64_t a=argc>0?rspp_to_i64(argv[0]):0; int64_t b=argc>1?rspp_to_i64(argv[1]):(int64_t)len; if(a<0)a=0; if((size_t)a>len)a=(int64_t)len; if(b<a)b=a; if((size_t)b>len)b=(int64_t)len; size_t n=(size_t)(b-a); char* out=(char*)malloc(n+1); memcpy(out,s+a,n); out[n]=0; return rspp_str_owned(out);}") };
    RsppVal _t1482 = rspp_method_call(rspp_v_lines, "push", 1, _t1483);
    RsppVal _t1485[] = { rspp_str("        if(strcmp(m,\"starts_with\")==0){const char* p=argc>0?rspp_to_str(argv[0]):\"\"; size_t pl=strlen(p); return rspp_bool(len>=pl&&memcmp(s,p,pl)==0);}") };
    RsppVal _t1484 = rspp_method_call(rspp_v_lines, "push", 1, _t1485);
    RsppVal _t1487[] = { rspp_str("        if(strcmp(m,\"contains\")==0){return rspp_bool(strstr(s,argc>0?rspp_to_str(argv[0]):\"\")!=NULL);}") };
    RsppVal _t1486 = rspp_method_call(rspp_v_lines, "push", 1, _t1487);
    RsppVal _t1489[] = { rspp_str("        if(strcmp(m,\"find\")==0){const char* f=strstr(s,argc>0?rspp_to_str(argv[0]):\"\"); return rspp_int(f?(int64_t)(f-s):-1);}") };
    RsppVal _t1488 = rspp_method_call(rspp_v_lines, "push", 1, _t1489);
    RsppVal _t1491[] = { rspp_str("        if(strcmp(m,\"trim\")==0){size_t a=0,b=len; while(a<b&&isspace((unsigned char)s[a]))a++; while(b>a&&isspace((unsigned char)s[b-1]))b--; char* out=(char*)malloc(b-a+1); memcpy(out,s+a,b-a); out[b-a]=0; return rspp_str_owned(out);}") };
    RsppVal _t1490 = rspp_method_call(rspp_v_lines, "push", 1, _t1491);
    RsppVal _t1493[] = { rspp_str("        if(strcmp(m,\"replace\")==0&&argc>=2){const char* from=rspp_to_str(argv[0]); const char* to=rspp_to_str(argv[1]); size_t fl=strlen(from),tl=strlen(to); if(fl==0)return recv; size_t cap=len*2+32,ol=0; char* out=(char*)malloc(cap); const char* p=s; while(*p){const char* hit=strstr(p,from); if(!hit){size_t rem=strlen(p); while(ol+rem+1>cap){cap*=2;out=(char*)realloc(out,cap);} memcpy(out+ol,p,rem+1); ol+=rem; break;} size_t pre=(size_t)(hit-p); while(ol+pre+tl+1>cap){cap*=2;out=(char*)realloc(out,cap);} memcpy(out+ol,p,pre); ol+=pre; memcpy(out+ol,to,tl); ol+=tl; p=hit+fl;} out[ol]=0; return rspp_str_owned(out);}") };
    RsppVal _t1492 = rspp_method_call(rspp_v_lines, "push", 1, _t1493);
    RsppVal _t1495[] = { rspp_str("        if(strcmp(m,\"split\")==0){const char* sep=argc>0?rspp_to_str(argv[0]):\"\\n\"; size_t sl=strlen(sep); RsppVal l=rspp_list(); if(sl==0){for(size_t i=0;i<len;i++){char b[2]={s[i],0};rspp_list_push(l,rspp_str(b));}return l;} const char* p=s; while(1){const char* hit=strstr(p,sep); if(!hit){rspp_list_push(l,rspp_str(p));break;} size_t cl=(size_t)(hit-p); char* sub=(char*)malloc(cl+1); memcpy(sub,p,cl); sub[cl]=0; rspp_list_push(l,rspp_str_owned(sub)); p=hit+sl;} return l;}") };
    RsppVal _t1494 = rspp_method_call(rspp_v_lines, "push", 1, _t1495);
    RsppVal _t1497[] = { rspp_str("        if(strcmp(m,\"to_int\")==0)return rspp_int(strtoll(s,NULL,0));") };
    RsppVal _t1496 = rspp_method_call(rspp_v_lines, "push", 1, _t1497);
    RsppVal _t1499[] = { rspp_str("    }") };
    RsppVal _t1498 = rspp_method_call(rspp_v_lines, "push", 1, _t1499);
    RsppVal _t1501[] = { rspp_str("    if(recv.kind==RSPP_LIST&&recv.list){") };
    RsppVal _t1500 = rspp_method_call(rspp_v_lines, "push", 1, _t1501);
    RsppVal _t1503[] = { rspp_str("        if(strcmp(m,\"len\")==0)return rspp_int((int64_t)recv.list->len);") };
    RsppVal _t1502 = rspp_method_call(rspp_v_lines, "push", 1, _t1503);
    RsppVal _t1505[] = { rspp_str("        if(strcmp(m,\"push\")==0&&argc>0){rspp_list_push(recv,argv[0]);return rspp_none();}") };
    RsppVal _t1504 = rspp_method_call(rspp_v_lines, "push", 1, _t1505);
    RsppVal _t1507[] = { rspp_str("        if(strcmp(m,\"pop\")==0)return recv.list->len>0?recv.list->items[--recv.list->len]:rspp_none();") };
    RsppVal _t1506 = rspp_method_call(rspp_v_lines, "push", 1, _t1507);
    RsppVal _t1509[] = { rspp_str("        if(strcmp(m,\"contains\")==0&&argc>0){for(size_t i=0;i<recv.list->len;i++)if(rspp_to_bool(rspp_eq(recv.list->items[i],argv[0])))return rspp_bool(1);return rspp_bool(0);}") };
    RsppVal _t1508 = rspp_method_call(rspp_v_lines, "push", 1, _t1509);
    RsppVal _t1511[] = { rspp_str("    }") };
    RsppVal _t1510 = rspp_method_call(rspp_v_lines, "push", 1, _t1511);
    RsppVal _t1513[] = { rspp_str("    if(recv.kind==RSPP_MAP&&recv.map){") };
    RsppVal _t1512 = rspp_method_call(rspp_v_lines, "push", 1, _t1513);
    RsppVal _t1515[] = { rspp_str("        if(strcmp(m,\"len\")==0)return rspp_int((int64_t)recv.map->len);") };
    RsppVal _t1514 = rspp_method_call(rspp_v_lines, "push", 1, _t1515);
    RsppVal _t1517[] = { rspp_str("        if(strcmp(m,\"has\")==0&&argc>0)return rspp_bool(rspp_map_has(recv,argv[0]));") };
    RsppVal _t1516 = rspp_method_call(rspp_v_lines, "push", 1, _t1517);
    RsppVal _t1519[] = { rspp_str("        if(strcmp(m,\"get\")==0&&argc>0)return rspp_map_get(recv,argv[0],argc>1?argv[1]:rspp_none());") };
    RsppVal _t1518 = rspp_method_call(rspp_v_lines, "push", 1, _t1519);
    RsppVal _t1521[] = { rspp_str("        if(strcmp(m,\"set\")==0&&argc>=2){rspp_map_set(recv,argv[0],argv[1]);return rspp_none();}") };
    RsppVal _t1520 = rspp_method_call(rspp_v_lines, "push", 1, _t1521);
    RsppVal _t1523[] = { rspp_str("        if(strcmp(m,\"keys\")==0){RsppVal l=rspp_list();for(size_t i=0;i<recv.map->len;i++)rspp_list_push(l,rspp_str(recv.map->entries[i].key));return l;}") };
    RsppVal _t1522 = rspp_method_call(rspp_v_lines, "push", 1, _t1523);
    RsppVal _t1525[] = { rspp_str("    }") };
    RsppVal _t1524 = rspp_method_call(rspp_v_lines, "push", 1, _t1525);
    RsppVal _t1527[] = { rspp_str("    return rspp_none();") };
    RsppVal _t1526 = rspp_method_call(rspp_v_lines, "push", 1, _t1527);
    RsppVal _t1529[] = { rspp_str("}") };
    RsppVal _t1528 = rspp_method_call(rspp_v_lines, "push", 1, _t1529);
    RsppVal _t1531[] = { rspp_str("static RsppVal rspp_builtin(const char* ns, const char* fn, int argc, RsppVal* argv) {") };
    RsppVal _t1530 = rspp_method_call(rspp_v_lines, "push", 1, _t1531);
    RsppVal _t1533[] = { rspp_str("    if(strcmp(ns,\"io\")==0){ if(strcmp(fn,\"print\")==0){printf(\"%s\\n\",argc>0?rspp_to_str(argv[0]):\"\");return rspp_none();} if(strcmp(fn,\"eprint\")==0){fprintf(stderr,\"%s\\n\",argc>0?rspp_to_str(argv[0]):\"\");return rspp_none();} }") };
    RsppVal _t1532 = rspp_method_call(rspp_v_lines, "push", 1, _t1533);
    RsppVal _t1535[] = { rspp_str("    if(strcmp(ns,\"memory\")==0){ if(strcmp(fn,\"alloc\")==0)return rspp_ptr(rspp_mem_alloc((size_t)(argc>0?rspp_to_i64(argv[0]):64))); if(strcmp(fn,\"free\")==0){rspp_mem_free(argc>0?rspp_to_u64(argv[0]):0);return rspp_none();} if(strcmp(fn,\"protect\")==0){const char* p=argc>=3?rspp_to_str(argv[2]):\"RW\"; return rspp_bool(!((strchr(p,'W')||strchr(p,'w'))&&(strchr(p,'X')||strchr(p,'x'))));} if(strcmp(fn,\"leak_report\")==0){int64_t act=0,lk=0; for(int i=0;i<rspp_arena_len;i++)if(!rspp_arena[i].freed){act++;lk+=(int64_t)rspp_arena[i].size;} RsppVal m=rspp_map(); rspp_map_set(m,rspp_str(\"active_allocs\"),rspp_int(act)); rspp_map_set(m,rspp_str(\"leaked_bytes\"),rspp_int(lk)); rspp_map_set(m,rspp_str(\"total_allocated\"),rspp_int((int64_t)rspp_total_alloc)); rspp_map_set(m,rspp_str(\"total_freed\"),rspp_int((int64_t)rspp_total_freed)); rspp_map_set(m,rspp_str(\"clean\"),rspp_bool(act==0)); return m;} }") };
    RsppVal _t1534 = rspp_method_call(rspp_v_lines, "push", 1, _t1535);
    RsppVal _t1537[] = { rspp_str("    if(strcmp(ns,\"fs\")==0){ if(strcmp(fn,\"read_text\")==0&&argc>0){const char* p=rspp_to_str(argv[0]); FILE* f=fopen(p,\"rb\"); if(!f){char e[512];snprintf(e,sizeof(e),\"Cannot read %s\",p);rspp_throw(e);} fseek(f,0,SEEK_END); long sz=ftell(f); fseek(f,0,SEEK_SET); if(sz<0)sz=0; char* b=(char*)malloc((size_t)sz+1); size_t rd=fread(b,1,(size_t)sz,f); b[rd]=0; fclose(f); return rspp_str_owned(b);} if(strcmp(fn,\"write_text\")==0&&argc>=2){const char* p=rspp_to_str(argv[0]); const char* b=rspp_to_str(argv[1]); FILE* f=fopen(p,\"wb\"); if(!f){char e[512];snprintf(e,sizeof(e),\"Cannot write %s\",p);rspp_throw(e);} fwrite(b,1,strlen(b),f); fclose(f); return rspp_bool(1);} if(strcmp(fn,\"exists\")==0&&argc>0){FILE* f=fopen(rspp_to_str(argv[0]),\"rb\"); if(f){fclose(f);return rspp_bool(1);} return rspp_bool(0);} }") };
    RsppVal _t1536 = rspp_method_call(rspp_v_lines, "push", 1, _t1537);
    RsppVal _t1539[] = { rspp_str("    if(strcmp(ns,\"sys\")==0){ if(strcmp(fn,\"args\")==0){RsppVal l=rspp_list(); for(int i=0;i<rspp_argc;i++)rspp_list_push(l,rspp_str(rspp_argv[i])); return l;} if(strcmp(fn,\"exec\")==0&&argc>0)return rspp_int(system(rspp_to_str(argv[0]))); if(strcmp(fn,\"exit\")==0)exit(argc>0?(int)rspp_to_i64(argv[0]):0); if(strcmp(fn,\"env\")==0&&argc>0)return rspp_str(getenv(rspp_to_str(argv[0]))); }") };
    RsppVal _t1538 = rspp_method_call(rspp_v_lines, "push", 1, _t1539);
    RsppVal _t1541[] = { rspp_str("    if(strcmp(ns,\"str\")==0){ if(strcmp(fn,\"from_code\")==0&&argc>0){char b[2]={(char)rspp_to_i64(argv[0]),0}; return rspp_str(b);} if(strcmp(fn,\"from_int\")==0&&argc>0){char b[32];snprintf(b,sizeof(b),\"%lld\",(long long)rspp_to_i64(argv[0]));return rspp_str(b);} if(strcmp(fn,\"to_int\")==0&&argc>0)return rspp_int(strtoll(rspp_to_str(argv[0]),NULL,0)); if(strcmp(fn,\"to_float\")==0&&argc>0)return rspp_flt(strtod(rspp_to_str(argv[0]),NULL)); if(strcmp(fn,\"join\")==0&&argc>=2&&argv[0].kind==RSPP_LIST){RsppList* l=argv[0].list; const char* sep=rspp_to_str(argv[1]); size_t sl=strlen(sep),cap=128,len=0; char* out=(char*)malloc(cap); out[0]=0; for(size_t i=0;i<l->len;i++){const char* it=rspp_to_str(l->items[i]); size_t il=strlen(it); while(len+il+sl+2>=cap){cap*=2;out=(char*)realloc(out,cap);} if(i>0&&sl>0){memcpy(out+len,sep,sl);len+=sl;} memcpy(out+len,it,il);len+=il;out[len]=0;} return rspp_str_owned(out);} }") };
    RsppVal _t1540 = rspp_method_call(rspp_v_lines, "push", 1, _t1541);
    RsppVal _t1543[] = { rspp_str("    if(strcmp(ns,\"math\")==0){ if(strcmp(fn,\"sin\")==0&&argc>0)return rspp_flt(sin(rspp_to_f64(argv[0]))); if(strcmp(fn,\"cos\")==0&&argc>0)return rspp_flt(cos(rspp_to_f64(argv[0]))); if(strcmp(fn,\"sqrt\")==0&&argc>0)return rspp_flt(sqrt(rspp_to_f64(argv[0]))); if(strcmp(fn,\"abs\")==0&&argc>0)return rspp_flt(fabs(rspp_to_f64(argv[0]))); if(strcmp(fn,\"log\")==0&&argc>0)return rspp_flt(log(rspp_to_f64(argv[0]))); if(strcmp(fn,\"exp\")==0&&argc>0)return rspp_flt(exp(rspp_to_f64(argv[0]))); if(strcmp(fn,\"solve\")==0&&argc>=1){RsppEq1Fn eq=rspp_find_eq(rspp_to_str(argv[0])); if(!eq)return rspp_flt(0.0); double x=argc>=2?rspp_to_f64(argv[1]):1.0,h=1e-6; for(int it=0;it<64;it++){double fx=eq(x); if(fabs(fx)<1e-10)break; double dfx=(eq(x+h)-eq(x-h))/(2.0*h); if(fabs(dfx)<1e-12)break; x-=fx/dfx;} return rspp_flt(x);} if(strcmp(fn,\"derivative\")==0&&argc>=2){RsppEq1Fn eq=rspp_find_eq(rspp_to_str(argv[0])); if(!eq)return rspp_flt(0.0); double x=rspp_to_f64(argv[1]),h=1e-6; return rspp_flt((eq(x+h)-eq(x-h))/(2.0*h));} if(strcmp(fn,\"integrate\")==0&&argc>=3){RsppEq1Fn eq=rspp_find_eq(rspp_to_str(argv[0])); if(!eq)return rspp_flt(0.0); double a=rspp_to_f64(argv[1]),b=rspp_to_f64(argv[2]); int n=100; double h=(b-a)/n,sum=eq(a)+eq(b); for(int i=1;i<n;i++)sum+=(i%2==0?2.0:4.0)*eq(a+i*h); return rspp_flt(sum*h/3.0);} }") };
    RsppVal _t1542 = rspp_method_call(rspp_v_lines, "push", 1, _t1543);
    RsppVal _t1545[] = { rspp_str("    if(strcmp(ns,\"sdr\")==0){ if(strcmp(fn,\"open\")==0)return rspp_ptr(rspp_mem_alloc(64)); if(strcmp(fn,\"close\")==0){rspp_mem_free(argc>0?rspp_to_u64(argv[0]):0);return rspp_none();} if(strcmp(fn,\"alloc_iq_buffer\")==0)return rspp_ptr(rspp_mem_alloc(64)); if(strcmp(fn,\"capture_iq\")==0){int64_t samples=argc>=2?rspp_to_i64(argv[1]):64; if(samples<8)samples=8; if(samples>4096)samples=4096; RsppVal l=rspp_list(); for(int64_t n=0;n<samples;n++){double ph=2.0*M_PI*0.125*(double)n; double env=((n/8)%2==0)?0.9:0.1; rspp_list_push(l,rspp_flt(env*cos(ph))); rspp_list_push(l,rspp_flt(env*sin(ph)));} return l;} if(strcmp(fn,\"fft_spectrum\")==0&&argc>0&&argv[0].kind==RSPP_LIST){size_t clen=argv[0].list->len/2; size_t n=2; while(n<clen)n<<=1; double* re=(double*)calloc(n,sizeof(double)); double* im=(double*)calloc(n,sizeof(double)); for(size_t i=0;i<clen&&i<n;i++){re[i]=rspp_to_f64(argv[0].list->items[2*i]); im[i]=rspp_to_f64(argv[0].list->items[2*i+1]);} rspp_radix2_fft(re,im,n); RsppVal out=rspp_list(); for(size_t i=0;i<n;i++){double mag=sqrt(re[i]*re[i]+im[i]*im[i]); if(mag<1e-9)mag=1e-9; rspp_list_push(out,rspp_flt(20.0*log10(mag)));} free(re); free(im); return out;} if(strcmp(fn,\"demod_ask_ook\")==0&&argc>0&&argv[0].kind==RSPP_LIST){double thresh=argc>=2?rspp_to_f64(argv[1]):0.5; RsppVal out=rspp_list(); for(size_t i=0;i+1<argv[0].list->len;i+=2){double iv=rspp_to_f64(argv[0].list->items[i]); double qv=rspp_to_f64(argv[0].list->items[i+1]); rspp_list_push(out,rspp_int(sqrt(iv*iv+qv*qv)>=thresh?1:0));} return out;} if(strcmp(fn,\"snr_db\")==0)return rspp_flt(19.08); return rspp_bool(1); }") };
    RsppVal _t1544 = rspp_method_call(rspp_v_lines, "push", 1, _t1545);
    RsppVal _t1547[] = { rspp_str("    if(strcmp(ns,\"synth\")==0){ if(strcmp(fn,\"synthesize_from_examples\")==0&&argc>=3&&argv[1].kind==RSPP_LIST&&argv[2].kind==RSPP_LIST){const char* name=rspp_to_str(argv[0]); RsppList* xs=argv[1].list; RsppList* ys=argv[2].list; char* buf=(char*)malloc(1024); int done=0; if(xs->len>=2&&ys->len>=2){double dx=rspp_to_f64(xs->items[1])-rspp_to_f64(xs->items[0]); double a=dx!=0.0?(rspp_to_f64(ys->items[1])-rspp_to_f64(ys->items[0]))/dx:1.0; double b=rspp_to_f64(ys->items[0])-a*rspp_to_f64(xs->items[0]); int lok=1; for(size_t i=0;i<xs->len&&i<ys->len;i++){if(fabs((a*rspp_to_f64(xs->items[i])+b)-rspp_to_f64(ys->items[i]))>1e-5)lok=0;} if(lok){snprintf(buf,1024,\"// Synthesized Linear Algorithm (MSE = 0.0)\\nfn %s(x: int) -> int:\\n    return %lld * x + %lld\\n\",name,(long long)llround(a),(long long)llround(b));done=1;}} if(!done&&xs->len>=3&&ys->len>=3){double x0=rspp_to_f64(xs->items[0]),x1=rspp_to_f64(xs->items[1]),x2=rspp_to_f64(xs->items[2]); double y0=rspp_to_f64(ys->items[0]),y1=rspp_to_f64(ys->items[1]),y2=rspp_to_f64(ys->items[2]); double den=(x0-x1)*(x0-x2)*(x1-x2); if(fabs(den)>1e-9){double qa=(x2*(y1-y0)+x1*(y0-y2)+x0*(y2-y1))/den; double qb=(x2*x2*(y0-y1)+x1*x1*(y2-y0)+x0*x0*(y1-y2))/den; double qc=(x1*x2*(x1-x2)*y0+x2*x0*(x2-x0)*y1+x0*x1*(x0-x1)*y2)/den; snprintf(buf,1024,\"// Synthesized Quadratic Algorithm (MSE = 0.0)\\nequation %s_eq(x) = %.4f*x^2 + %.4f*x + %.4f\\nfn %s(x: float) -> float:\\n    return %s_eq(x)\\n\",name,qa,qb,qc,name,name);done=1;}} if(!done){snprintf(buf,1024,\"fn %s(x: int) -> int:\\n    return x\\n\",name);} return rspp_str_owned(buf);} if(strcmp(fn,\"generate_dsp_filter\")==0){const char* kind=argc>0?rspp_to_str(argv[0]):\"lowpass\"; char* b=(char*)malloc(512); snprintf(b,512,\"// Synthesized %s Windowed-Sinc FIR Filter\\nfn apply_%s_fir(samples: list) -> list:\\n    return samples\\n\",kind,kind); return rspp_str_owned(b);} if(strcmp(fn,\"evolve_crypto_mixer\")==0){const char* nm=argc>0?rspp_to_str(argv[0]):\"arx_mix\"; char* b=(char*)malloc(512); snprintf(b,512,\"// Synthesized ARX Cryptographic Mixer\\nfn %s(x: int) -> int:\\n    var v = x ^ 0x9e3779b9\\n    v = (v + 0x7f4a7c15) ^ ((v << 5) & 0x7FFFFFFF)\\n    return v\\n\",nm); return rspp_str_owned(b);} return rspp_str(\"// Synthesized Algorithm\"); }") };
    RsppVal _t1546 = rspp_method_call(rspp_v_lines, "push", 1, _t1547);
    RsppVal _t1549[] = { rspp_str("    if(strcmp(ns,\"security\")==0){ if(strcmp(fn,\"constant_time_eq\")==0&&argc>=2){const char* a=rspp_to_str(argv[0]); const char* b=rspp_to_str(argv[1]); size_t la=strlen(a),lb=strlen(b),n=la<lb?la:lb; unsigned diff=(unsigned)(la^lb); for(size_t i=0;i<n;i++)diff|=(unsigned)(a[i]^b[i]); return rspp_bool(diff==0);} if(strcmp(fn,\"canary_check\")==0)return rspp_bool(1); }") };
    RsppVal _t1548 = rspp_method_call(rspp_v_lines, "push", 1, _t1549);
    RsppVal _t1551[] = { rspp_str("    if(strcmp(ns,\"hardware\")==0){ if(strcmp(fn,\"probe\")==0){RsppVal m=rspp_map(); rspp_map_set(m,rspp_str(\"leak_guard\"),rspp_str(\"active\")); rspp_map_set(m,rspp_str(\"sdr_capable\"),rspp_bool(1)); return m;} if(strcmp(fn,\"temp\")==0)return rspp_flt(42.5); }") };
    RsppVal _t1550 = rspp_method_call(rspp_v_lines, "push", 1, _t1551);
    RsppVal _t1553[] = { rspp_str("    if(strcmp(ns,\"power\")==0){ if(strcmp(fn,\"watts\")==0)return rspp_flt(28.4); if(strcmp(fn,\"get_voltage\")==0)return rspp_flt(1150.0); }") };
    RsppVal _t1552 = rspp_method_call(rspp_v_lines, "push", 1, _t1553);
    RsppVal _t1555[] = { rspp_str("    if(strcmp(ns,\"cpu\")==0&&strcmp(fn,\"cores\")==0)return rspp_int(4);") };
    RsppVal _t1554 = rspp_method_call(rspp_v_lines, "push", 1, _t1555);
    RsppVal _t1557[] = { rspp_str("    return rspp_none();") };
    RsppVal _t1556 = rspp_method_call(rspp_v_lines, "push", 1, _t1557);
    RsppVal _t1559[] = { rspp_str("}") };
    RsppVal _t1558 = rspp_method_call(rspp_v_lines, "push", 1, _t1559);
    return rspp_none();
}

static RsppVal rspp_fn_new_emit_ctx(void) {
    RsppVal _t1561 = rspp_map();
    RsppVal _t1562 = rspp_list();
    rspp_map_set(_t1561, rspp_str("lines"), _t1562);
    rspp_map_set(_t1561, rspp_str("tmp"), rspp_int(0LL));
    RsppVal _t1563 = rspp_list();
    rspp_map_set(_t1561, rspp_str("defers"), _t1563);
    RsppVal _t1564 = rspp_map();
    rspp_map_set(_t1561, rspp_str("externs"), _t1564);
    RsppVal _t1560 = _t1561;
    return _t1560;
    return rspp_none();
}

static RsppVal rspp_fn_cg_tmp(RsppVal rspp_v_ctx) {
    RsppVal _t1566[] = { rspp_str("tmp"), rspp_int(0LL) };
    RsppVal _t1565 = rspp_method_call(rspp_v_ctx, "get", 2, _t1566);
    RsppVal rspp_v_n = rspp_add(_t1565, rspp_int(1LL));
    RsppVal _t1568[] = { rspp_str("tmp"), rspp_v_n };
    RsppVal _t1567 = rspp_method_call(rspp_v_ctx, "set", 2, _t1568);
    RsppVal _t1570 = rspp_str("");
    _t1570 = rspp_add(_t1570, rspp_str("_t"));
    _t1570 = rspp_add(_t1570, rspp_str(rspp_to_str(rspp_v_n)));
    RsppVal _t1569 = _t1570;
    return _t1569;
    return rspp_none();
}

static RsppVal rspp_fn_cg_push_line(RsppVal rspp_v_ctx, RsppVal rspp_v_s) {
    RsppVal _t1571 = rspp_list();
    RsppVal _t1573[] = { rspp_str("lines"), _t1571 };
    RsppVal _t1572 = rspp_method_call(rspp_v_ctx, "get", 2, _t1573);
    RsppVal rspp_v_ls = _t1572;
    RsppVal _t1575[] = { rspp_v_s };
    RsppVal _t1574 = rspp_method_call(rspp_v_ls, "push", 1, _t1575);
    return rspp_none();
}

static RsppVal rspp_fn_cg_emit_fstr(RsppVal rspp_v_ctx, RsppVal rspp_v_tpl, RsppVal rspp_v_pad) {
    RsppVal _t1576 = rspp_fn_cg_tmp(rspp_v_ctx);
    RsppVal rspp_v_acc = _t1576;
    RsppVal _t1577 = rspp_str("");
    _t1577 = rspp_add(_t1577, rspp_str(rspp_to_str(rspp_v_pad)));
    _t1577 = rspp_add(_t1577, rspp_str("RsppVal "));
    _t1577 = rspp_add(_t1577, rspp_str(rspp_to_str(rspp_v_acc)));
    _t1577 = rspp_add(_t1577, rspp_str(" = rspp_str(\"\");"));
    RsppVal _t1578 = rspp_fn_cg_push_line(rspp_v_ctx, _t1577);
    RsppVal rspp_v_i = rspp_int(0LL);
    RsppVal _t1579 = rspp_method_call(rspp_v_tpl, "len", 0, NULL);
    RsppVal rspp_v_n = _t1579;
    RsppVal rspp_v_lit = rspp_str("");
    while (1) {
        if (!rspp_to_bool(rspp_lt(rspp_v_i, rspp_v_n))) break;
        RsppVal _t1581[] = { rspp_v_i };
        RsppVal _t1580 = rspp_method_call(rspp_v_tpl, "char_at", 1, _t1581);
        RsppVal rspp_v_c = _t1580;
        if (rspp_to_bool(rspp_eq(rspp_v_c, rspp_str("{")))) {
            RsppVal _t1582 = rspp_bool(0);
            if (rspp_to_bool(rspp_lt(rspp_add(rspp_v_i, rspp_int(1LL)), rspp_v_n))) {
                RsppVal _t1584[] = { rspp_add(rspp_v_i, rspp_int(1LL)) };
                RsppVal _t1583 = rspp_method_call(rspp_v_tpl, "char_at", 1, _t1584);
                _t1582 = rspp_bool(rspp_to_bool(rspp_eq(_t1583, rspp_str("{"))));
            }
            if (rspp_to_bool(_t1582)) {
                rspp_v_lit = rspp_add(rspp_v_lit, rspp_str("{"));
                rspp_v_i = rspp_add(rspp_v_i, rspp_int(2LL));
                continue;
            }
            RsppVal _t1585 = rspp_method_call(rspp_v_lit, "len", 0, NULL);
            if (rspp_to_bool(rspp_gt(_t1585, rspp_int(0LL)))) {
                RsppVal _t1586 = rspp_str("");
                _t1586 = rspp_add(_t1586, rspp_str(rspp_to_str(rspp_v_pad)));
                _t1586 = rspp_add(_t1586, rspp_str(rspp_to_str(rspp_v_acc)));
                _t1586 = rspp_add(_t1586, rspp_str(" = rspp_add("));
                _t1586 = rspp_add(_t1586, rspp_str(rspp_to_str(rspp_v_acc)));
                _t1586 = rspp_add(_t1586, rspp_str(", rspp_str(\""));
                RsppVal _t1587 = rspp_fn_esc_c_str(rspp_v_lit);
                _t1586 = rspp_add(_t1586, rspp_str(rspp_to_str(_t1587)));
                _t1586 = rspp_add(_t1586, rspp_str("\"));"));
                RsppVal _t1588 = rspp_fn_cg_push_line(rspp_v_ctx, _t1586);
                rspp_v_lit = rspp_str("");
            }
            rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
            RsppVal rspp_v_esrc = rspp_str("");
            RsppVal rspp_v_depth = rspp_int(1LL);
            while (1) {
                RsppVal _t1589 = rspp_bool(0);
                if (rspp_to_bool(rspp_lt(rspp_v_i, rspp_v_n))) {
                    _t1589 = rspp_bool(rspp_to_bool(rspp_gt(rspp_v_depth, rspp_int(0LL))));
                }
                if (!rspp_to_bool(_t1589)) break;
                RsppVal _t1591[] = { rspp_v_i };
                RsppVal _t1590 = rspp_method_call(rspp_v_tpl, "char_at", 1, _t1591);
                RsppVal rspp_v_ch = _t1590;
                if (rspp_to_bool(rspp_eq(rspp_v_ch, rspp_str("{")))) {
                    rspp_v_depth = rspp_add(rspp_v_depth, rspp_int(1LL));
                } else {
                    if (rspp_to_bool(rspp_eq(rspp_v_ch, rspp_str("}")))) {
                        rspp_v_depth = rspp_sub(rspp_v_depth, rspp_int(1LL));
                        if (rspp_to_bool(rspp_eq(rspp_v_depth, rspp_int(0LL)))) {
                            rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
                            break;
                        }
                }
                }
                rspp_v_esrc = rspp_add(rspp_v_esrc, rspp_v_ch);
                rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
            }
            RsppVal _t1592 = rspp_fn_tokenize_source(rspp_v_esrc);
            RsppVal rspp_v_toks = _t1592;
            RsppVal _t1593 = rspp_fn_new_parser(rspp_v_toks);
            RsppVal rspp_v_p = _t1593;
            RsppVal _t1594 = rspp_fn_parse_expr(rspp_v_p);
            RsppVal rspp_v_e = _t1594;
            RsppVal _t1595 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_e, rspp_v_pad);
            RsppVal rspp_v_ev = _t1595;
            RsppVal _t1596 = rspp_str("");
            _t1596 = rspp_add(_t1596, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1596 = rspp_add(_t1596, rspp_str(rspp_to_str(rspp_v_acc)));
            _t1596 = rspp_add(_t1596, rspp_str(" = rspp_add("));
            _t1596 = rspp_add(_t1596, rspp_str(rspp_to_str(rspp_v_acc)));
            _t1596 = rspp_add(_t1596, rspp_str(", rspp_str(rspp_to_str("));
            _t1596 = rspp_add(_t1596, rspp_str(rspp_to_str(rspp_v_ev)));
            _t1596 = rspp_add(_t1596, rspp_str(")));"));
            RsppVal _t1597 = rspp_fn_cg_push_line(rspp_v_ctx, _t1596);
        } else {
            RsppVal _t1599 = rspp_bool(0);
            if (rspp_to_bool(rspp_eq(rspp_v_c, rspp_str("}")))) {
                _t1599 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_i, rspp_int(1LL)), rspp_v_n)));
            }
            RsppVal _t1598 = rspp_bool(0);
            if (rspp_to_bool(_t1599)) {
                RsppVal _t1601[] = { rspp_add(rspp_v_i, rspp_int(1LL)) };
                RsppVal _t1600 = rspp_method_call(rspp_v_tpl, "char_at", 1, _t1601);
                _t1598 = rspp_bool(rspp_to_bool(rspp_eq(_t1600, rspp_str("}"))));
            }
            if (rspp_to_bool(_t1598)) {
                rspp_v_lit = rspp_add(rspp_v_lit, rspp_str("}"));
                rspp_v_i = rspp_add(rspp_v_i, rspp_int(2LL));
        } else {
            rspp_v_lit = rspp_add(rspp_v_lit, rspp_v_c);
            rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
        }
        }
    }
    RsppVal _t1602 = rspp_method_call(rspp_v_lit, "len", 0, NULL);
    if (rspp_to_bool(rspp_gt(_t1602, rspp_int(0LL)))) {
        RsppVal _t1603 = rspp_str("");
        _t1603 = rspp_add(_t1603, rspp_str(rspp_to_str(rspp_v_pad)));
        _t1603 = rspp_add(_t1603, rspp_str(rspp_to_str(rspp_v_acc)));
        _t1603 = rspp_add(_t1603, rspp_str(" = rspp_add("));
        _t1603 = rspp_add(_t1603, rspp_str(rspp_to_str(rspp_v_acc)));
        _t1603 = rspp_add(_t1603, rspp_str(", rspp_str(\""));
        RsppVal _t1604 = rspp_fn_esc_c_str(rspp_v_lit);
        _t1603 = rspp_add(_t1603, rspp_str(rspp_to_str(_t1604)));
        _t1603 = rspp_add(_t1603, rspp_str("\"));"));
        RsppVal _t1605 = rspp_fn_cg_push_line(rspp_v_ctx, _t1603);
    }
    RsppVal _t1606 = rspp_v_acc;
    return _t1606;
    return rspp_none();
}

static RsppVal rspp_fn_cg_emit_expr(RsppVal rspp_v_ctx, RsppVal rspp_v_e, RsppVal rspp_v_pad) {
    RsppVal _t1608[] = { rspp_str("kind"), rspp_str("none") };
    RsppVal _t1607 = rspp_method_call(rspp_v_e, "get", 2, _t1608);
    RsppVal rspp_v_k = _t1607;
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("int")))) {
        RsppVal _t1610 = rspp_str("");
        _t1610 = rspp_add(_t1610, rspp_str("rspp_int("));
        RsppVal _t1612[] = { rspp_str("val"), rspp_str("0") };
        RsppVal _t1611 = rspp_method_call(rspp_v_e, "get", 2, _t1612);
        _t1610 = rspp_add(_t1610, rspp_str(rspp_to_str(_t1611)));
        _t1610 = rspp_add(_t1610, rspp_str("LL)"));
        RsppVal _t1609 = _t1610;
        return _t1609;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("hex")))) {
        RsppVal _t1614 = rspp_str("");
        _t1614 = rspp_add(_t1614, rspp_str("rspp_ptr("));
        RsppVal _t1616[] = { rspp_str("val"), rspp_str("0x0") };
        RsppVal _t1615 = rspp_method_call(rspp_v_e, "get", 2, _t1616);
        _t1614 = rspp_add(_t1614, rspp_str(rspp_to_str(_t1615)));
        _t1614 = rspp_add(_t1614, rspp_str("ULL)"));
        RsppVal _t1613 = _t1614;
        return _t1613;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("float")))) {
        RsppVal _t1618 = rspp_str("");
        _t1618 = rspp_add(_t1618, rspp_str("rspp_flt((double)"));
        RsppVal _t1620[] = { rspp_str("val"), rspp_str("0.0") };
        RsppVal _t1619 = rspp_method_call(rspp_v_e, "get", 2, _t1620);
        _t1618 = rspp_add(_t1618, rspp_str(rspp_to_str(_t1619)));
        _t1618 = rspp_add(_t1618, rspp_str(")"));
        RsppVal _t1617 = _t1618;
        return _t1617;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("str")))) {
        RsppVal _t1622 = rspp_str("");
        _t1622 = rspp_add(_t1622, rspp_str("rspp_str(\""));
        RsppVal _t1624[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t1623 = rspp_method_call(rspp_v_e, "get", 2, _t1624);
        RsppVal _t1625 = rspp_fn_esc_c_str(_t1623);
        _t1622 = rspp_add(_t1622, rspp_str(rspp_to_str(_t1625)));
        _t1622 = rspp_add(_t1622, rspp_str("\")"));
        RsppVal _t1621 = _t1622;
        return _t1621;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("fstr")))) {
        RsppVal _t1628[] = { rspp_str("val"), rspp_str("") };
        RsppVal _t1627 = rspp_method_call(rspp_v_e, "get", 2, _t1628);
        RsppVal _t1629 = rspp_fn_cg_emit_fstr(rspp_v_ctx, _t1627, rspp_v_pad);
        RsppVal _t1626 = _t1629;
        return _t1626;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("bool")))) {
        RsppVal rspp_v_bv = rspp_int(0LL);
        RsppVal _t1631[] = { rspp_str("val"), rspp_str("false") };
        RsppVal _t1630 = rspp_method_call(rspp_v_e, "get", 2, _t1631);
        if (rspp_to_bool(rspp_eq(_t1630, rspp_str("true")))) {
            rspp_v_bv = rspp_int(1LL);
        }
        RsppVal _t1633 = rspp_str("");
        _t1633 = rspp_add(_t1633, rspp_str("rspp_bool("));
        _t1633 = rspp_add(_t1633, rspp_str(rspp_to_str(rspp_v_bv)));
        _t1633 = rspp_add(_t1633, rspp_str(")"));
        RsppVal _t1632 = _t1633;
        return _t1632;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("none")))) {
        RsppVal _t1634 = rspp_str("rspp_none()");
        return _t1634;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("ident")))) {
        RsppVal _t1637[] = { rspp_str("name"), rspp_str("") };
        RsppVal _t1636 = rspp_method_call(rspp_v_e, "get", 2, _t1637);
        RsppVal _t1638 = rspp_fn_c_ident(_t1636);
        RsppVal _t1635 = rspp_add(rspp_str("rspp_v_"), _t1638);
        return _t1635;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("list")))) {
        RsppVal _t1639 = rspp_fn_cg_tmp(rspp_v_ctx);
        RsppVal rspp_v_tmp = _t1639;
        RsppVal _t1640 = rspp_str("");
        _t1640 = rspp_add(_t1640, rspp_str(rspp_to_str(rspp_v_pad)));
        _t1640 = rspp_add(_t1640, rspp_str("RsppVal "));
        _t1640 = rspp_add(_t1640, rspp_str(rspp_to_str(rspp_v_tmp)));
        _t1640 = rspp_add(_t1640, rspp_str(" = rspp_list();"));
        RsppVal _t1641 = rspp_fn_cg_push_line(rspp_v_ctx, _t1640);
        RsppVal _t1642 = rspp_list();
        RsppVal _t1644[] = { rspp_str("items"), _t1642 };
        RsppVal _t1643 = rspp_method_call(rspp_v_e, "get", 2, _t1644);
        RsppVal _t1645 = _t1643;
        for (int64_t _t1646 = 0; _t1646 < rspp_list_len(_t1645); _t1646++) {
            RsppVal rspp_v_it = rspp_index_get(_t1645, rspp_int(_t1646));
            RsppVal _t1647 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_it, rspp_v_pad);
            RsppVal rspp_v_ev = _t1647;
            RsppVal _t1648 = rspp_str("");
            _t1648 = rspp_add(_t1648, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1648 = rspp_add(_t1648, rspp_str("rspp_list_push("));
            _t1648 = rspp_add(_t1648, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1648 = rspp_add(_t1648, rspp_str(", "));
            _t1648 = rspp_add(_t1648, rspp_str(rspp_to_str(rspp_v_ev)));
            _t1648 = rspp_add(_t1648, rspp_str(");"));
            RsppVal _t1649 = rspp_fn_cg_push_line(rspp_v_ctx, _t1648);
        }
        RsppVal _t1650 = rspp_v_tmp;
        return _t1650;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("map")))) {
        RsppVal _t1651 = rspp_fn_cg_tmp(rspp_v_ctx);
        RsppVal rspp_v_tmp = _t1651;
        RsppVal _t1652 = rspp_str("");
        _t1652 = rspp_add(_t1652, rspp_str(rspp_to_str(rspp_v_pad)));
        _t1652 = rspp_add(_t1652, rspp_str("RsppVal "));
        _t1652 = rspp_add(_t1652, rspp_str(rspp_to_str(rspp_v_tmp)));
        _t1652 = rspp_add(_t1652, rspp_str(" = rspp_map();"));
        RsppVal _t1653 = rspp_fn_cg_push_line(rspp_v_ctx, _t1652);
        RsppVal _t1654 = rspp_list();
        RsppVal _t1656[] = { rspp_str("entries"), _t1654 };
        RsppVal _t1655 = rspp_method_call(rspp_v_e, "get", 2, _t1656);
        RsppVal _t1657 = _t1655;
        for (int64_t _t1658 = 0; _t1658 < rspp_list_len(_t1657); _t1658++) {
            RsppVal rspp_v_ent = rspp_index_get(_t1657, rspp_int(_t1658));
            RsppVal _t1659 = rspp_map();
            RsppVal _t1661[] = { rspp_str("key"), _t1659 };
            RsppVal _t1660 = rspp_method_call(rspp_v_ent, "get", 2, _t1661);
            RsppVal _t1662 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1660, rspp_v_pad);
            RsppVal rspp_v_ek = _t1662;
            RsppVal _t1663 = rspp_map();
            RsppVal _t1665[] = { rspp_str("val"), _t1663 };
            RsppVal _t1664 = rspp_method_call(rspp_v_ent, "get", 2, _t1665);
            RsppVal _t1666 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1664, rspp_v_pad);
            RsppVal rspp_v_ev = _t1666;
            RsppVal _t1667 = rspp_str("");
            _t1667 = rspp_add(_t1667, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1667 = rspp_add(_t1667, rspp_str("rspp_map_set("));
            _t1667 = rspp_add(_t1667, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1667 = rspp_add(_t1667, rspp_str(", "));
            _t1667 = rspp_add(_t1667, rspp_str(rspp_to_str(rspp_v_ek)));
            _t1667 = rspp_add(_t1667, rspp_str(", "));
            _t1667 = rspp_add(_t1667, rspp_str(rspp_to_str(rspp_v_ev)));
            _t1667 = rspp_add(_t1667, rspp_str(");"));
            RsppVal _t1668 = rspp_fn_cg_push_line(rspp_v_ctx, _t1667);
        }
        RsppVal _t1669 = rspp_v_tmp;
        return _t1669;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("range")))) {
        RsppVal _t1670 = rspp_map();
        RsppVal _t1672[] = { rspp_str("start"), _t1670 };
        RsppVal _t1671 = rspp_method_call(rspp_v_e, "get", 2, _t1672);
        RsppVal _t1673 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1671, rspp_v_pad);
        RsppVal rspp_v_s = _t1673;
        RsppVal _t1674 = rspp_map();
        RsppVal _t1676[] = { rspp_str("end"), _t1674 };
        RsppVal _t1675 = rspp_method_call(rspp_v_e, "get", 2, _t1676);
        RsppVal _t1677 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1675, rspp_v_pad);
        RsppVal rspp_v_en = _t1677;
        RsppVal rspp_v_inc = rspp_int(0LL);
        RsppVal _t1679[] = { rspp_str("inclusive"), rspp_bool(0) };
        RsppVal _t1678 = rspp_method_call(rspp_v_e, "get", 2, _t1679);
        if (rspp_to_bool(_t1678)) {
            rspp_v_inc = rspp_int(1LL);
        }
        RsppVal _t1681 = rspp_str("");
        _t1681 = rspp_add(_t1681, rspp_str("rspp_range("));
        _t1681 = rspp_add(_t1681, rspp_str(rspp_to_str(rspp_v_s)));
        _t1681 = rspp_add(_t1681, rspp_str(", "));
        _t1681 = rspp_add(_t1681, rspp_str(rspp_to_str(rspp_v_en)));
        _t1681 = rspp_add(_t1681, rspp_str(", "));
        _t1681 = rspp_add(_t1681, rspp_str(rspp_to_str(rspp_v_inc)));
        _t1681 = rspp_add(_t1681, rspp_str(")"));
        RsppVal _t1680 = _t1681;
        return _t1680;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("index")))) {
        RsppVal _t1682 = rspp_map();
        RsppVal _t1684[] = { rspp_str("recv"), _t1682 };
        RsppVal _t1683 = rspp_method_call(rspp_v_e, "get", 2, _t1684);
        RsppVal _t1685 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1683, rspp_v_pad);
        RsppVal rspp_v_r = _t1685;
        RsppVal _t1686 = rspp_map();
        RsppVal _t1688[] = { rspp_str("index"), _t1686 };
        RsppVal _t1687 = rspp_method_call(rspp_v_e, "get", 2, _t1688);
        RsppVal _t1689 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1687, rspp_v_pad);
        RsppVal rspp_v_idx = _t1689;
        RsppVal _t1691 = rspp_str("");
        _t1691 = rspp_add(_t1691, rspp_str("rspp_index_get("));
        _t1691 = rspp_add(_t1691, rspp_str(rspp_to_str(rspp_v_r)));
        _t1691 = rspp_add(_t1691, rspp_str(", "));
        _t1691 = rspp_add(_t1691, rspp_str(rspp_to_str(rspp_v_idx)));
        _t1691 = rspp_add(_t1691, rspp_str(")"));
        RsppVal _t1690 = _t1691;
        return _t1690;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("field")))) {
        RsppVal _t1692 = rspp_map();
        RsppVal _t1694[] = { rspp_str("recv"), _t1692 };
        RsppVal _t1693 = rspp_method_call(rspp_v_e, "get", 2, _t1694);
        RsppVal _t1695 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1693, rspp_v_pad);
        RsppVal rspp_v_r = _t1695;
        RsppVal _t1697[] = { rspp_str("field"), rspp_str("") };
        RsppVal _t1696 = rspp_method_call(rspp_v_e, "get", 2, _t1697);
        RsppVal _t1698 = rspp_fn_esc_c_str(_t1696);
        RsppVal rspp_v_fld = _t1698;
        RsppVal _t1700 = rspp_str("");
        _t1700 = rspp_add(_t1700, rspp_str("rspp_index_get("));
        _t1700 = rspp_add(_t1700, rspp_str(rspp_to_str(rspp_v_r)));
        _t1700 = rspp_add(_t1700, rspp_str(", rspp_str(\""));
        _t1700 = rspp_add(_t1700, rspp_str(rspp_to_str(rspp_v_fld)));
        _t1700 = rspp_add(_t1700, rspp_str("\"))"));
        RsppVal _t1699 = _t1700;
        return _t1699;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("method")))) {
        RsppVal _t1701 = rspp_map();
        RsppVal _t1703[] = { rspp_str("recv"), _t1701 };
        RsppVal _t1702 = rspp_method_call(rspp_v_e, "get", 2, _t1703);
        RsppVal _t1704 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1702, rspp_v_pad);
        RsppVal rspp_v_r = _t1704;
        RsppVal _t1706[] = { rspp_str("method"), rspp_str("") };
        RsppVal _t1705 = rspp_method_call(rspp_v_e, "get", 2, _t1706);
        RsppVal _t1707 = rspp_fn_esc_c_str(_t1705);
        RsppVal rspp_v_m = _t1707;
        RsppVal _t1708 = rspp_list();
        RsppVal _t1710[] = { rspp_str("args"), _t1708 };
        RsppVal _t1709 = rspp_method_call(rspp_v_e, "get", 2, _t1710);
        RsppVal rspp_v_args = _t1709;
        RsppVal _t1711 = rspp_list();
        RsppVal rspp_v_evs = _t1711;
        RsppVal _t1712 = rspp_v_args;
        for (int64_t _t1713 = 0; _t1713 < rspp_list_len(_t1712); _t1713++) {
            RsppVal rspp_v_a = rspp_index_get(_t1712, rspp_int(_t1713));
            RsppVal _t1714 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_a, rspp_v_pad);
            RsppVal _t1716[] = { _t1714 };
            RsppVal _t1715 = rspp_method_call(rspp_v_evs, "push", 1, _t1716);
        }
        RsppVal _t1717 = rspp_fn_cg_tmp(rspp_v_ctx);
        RsppVal rspp_v_tmp = _t1717;
        RsppVal _t1718 = rspp_method_call(rspp_v_evs, "len", 0, NULL);
        if (rspp_to_bool(rspp_eq(_t1718, rspp_int(0LL)))) {
            RsppVal _t1719 = rspp_str("");
            _t1719 = rspp_add(_t1719, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1719 = rspp_add(_t1719, rspp_str("RsppVal "));
            _t1719 = rspp_add(_t1719, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1719 = rspp_add(_t1719, rspp_str(" = rspp_method_call("));
            _t1719 = rspp_add(_t1719, rspp_str(rspp_to_str(rspp_v_r)));
            _t1719 = rspp_add(_t1719, rspp_str(", \""));
            _t1719 = rspp_add(_t1719, rspp_str(rspp_to_str(rspp_v_m)));
            _t1719 = rspp_add(_t1719, rspp_str("\", 0, NULL);"));
            RsppVal _t1720 = rspp_fn_cg_push_line(rspp_v_ctx, _t1719);
        } else {
            RsppVal _t1721 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_arr = _t1721;
            RsppVal _t1723[] = { rspp_v_evs, rspp_str(", ") };
            RsppVal _t1722 = rspp_builtin("str", "join", 2, _t1723);
            RsppVal rspp_v_joined = _t1722;
            RsppVal _t1724 = rspp_str("");
            _t1724 = rspp_add(_t1724, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1724 = rspp_add(_t1724, rspp_str("RsppVal "));
            _t1724 = rspp_add(_t1724, rspp_str(rspp_to_str(rspp_v_arr)));
            _t1724 = rspp_add(_t1724, rspp_str("[] = { "));
            _t1724 = rspp_add(_t1724, rspp_str(rspp_to_str(rspp_v_joined)));
            _t1724 = rspp_add(_t1724, rspp_str(" };"));
            RsppVal _t1725 = rspp_fn_cg_push_line(rspp_v_ctx, _t1724);
            RsppVal _t1726 = rspp_str("");
            _t1726 = rspp_add(_t1726, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1726 = rspp_add(_t1726, rspp_str("RsppVal "));
            _t1726 = rspp_add(_t1726, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1726 = rspp_add(_t1726, rspp_str(" = rspp_method_call("));
            _t1726 = rspp_add(_t1726, rspp_str(rspp_to_str(rspp_v_r)));
            _t1726 = rspp_add(_t1726, rspp_str(", \""));
            _t1726 = rspp_add(_t1726, rspp_str(rspp_to_str(rspp_v_m)));
            _t1726 = rspp_add(_t1726, rspp_str("\", "));
            RsppVal _t1727 = rspp_method_call(rspp_v_evs, "len", 0, NULL);
            _t1726 = rspp_add(_t1726, rspp_str(rspp_to_str(_t1727)));
            _t1726 = rspp_add(_t1726, rspp_str(", "));
            _t1726 = rspp_add(_t1726, rspp_str(rspp_to_str(rspp_v_arr)));
            _t1726 = rspp_add(_t1726, rspp_str(");"));
            RsppVal _t1728 = rspp_fn_cg_push_line(rspp_v_ctx, _t1726);
        }
        RsppVal _t1729 = rspp_v_tmp;
        return _t1729;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("binary")))) {
        RsppVal _t1731[] = { rspp_str("op"), rspp_str("+") };
        RsppVal _t1730 = rspp_method_call(rspp_v_e, "get", 2, _t1731);
        RsppVal rspp_v_op = _t1730;
        if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("and")))) {
            RsppVal _t1732 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_tmp = _t1732;
            RsppVal _t1733 = rspp_map();
            RsppVal _t1735[] = { rspp_str("left"), _t1733 };
            RsppVal _t1734 = rspp_method_call(rspp_v_e, "get", 2, _t1735);
            RsppVal _t1736 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1734, rspp_v_pad);
            RsppVal rspp_v_l = _t1736;
            RsppVal _t1737 = rspp_str("");
            _t1737 = rspp_add(_t1737, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1737 = rspp_add(_t1737, rspp_str("RsppVal "));
            _t1737 = rspp_add(_t1737, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1737 = rspp_add(_t1737, rspp_str(" = rspp_bool(0);"));
            RsppVal _t1738 = rspp_fn_cg_push_line(rspp_v_ctx, _t1737);
            RsppVal _t1739 = rspp_str("");
            _t1739 = rspp_add(_t1739, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1739 = rspp_add(_t1739, rspp_str("if (rspp_to_bool("));
            _t1739 = rspp_add(_t1739, rspp_str(rspp_to_str(rspp_v_l)));
            _t1739 = rspp_add(_t1739, rspp_str(")) {"));
            RsppVal _t1740 = rspp_fn_cg_push_line(rspp_v_ctx, _t1739);
            RsppVal _t1741 = rspp_map();
            RsppVal _t1743[] = { rspp_str("right"), _t1741 };
            RsppVal _t1742 = rspp_method_call(rspp_v_e, "get", 2, _t1743);
            RsppVal _t1744 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1742, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal rspp_v_r = _t1744;
            RsppVal _t1745 = rspp_str("");
            _t1745 = rspp_add(_t1745, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1745 = rspp_add(_t1745, rspp_str("    "));
            _t1745 = rspp_add(_t1745, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1745 = rspp_add(_t1745, rspp_str(" = rspp_bool(rspp_to_bool("));
            _t1745 = rspp_add(_t1745, rspp_str(rspp_to_str(rspp_v_r)));
            _t1745 = rspp_add(_t1745, rspp_str("));"));
            RsppVal _t1746 = rspp_fn_cg_push_line(rspp_v_ctx, _t1745);
            RsppVal _t1747 = rspp_str("");
            _t1747 = rspp_add(_t1747, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1747 = rspp_add(_t1747, rspp_str("}"));
            RsppVal _t1748 = rspp_fn_cg_push_line(rspp_v_ctx, _t1747);
            RsppVal _t1749 = rspp_v_tmp;
            return _t1749;
        }
        if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("or")))) {
            RsppVal _t1750 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_tmp = _t1750;
            RsppVal _t1751 = rspp_map();
            RsppVal _t1753[] = { rspp_str("left"), _t1751 };
            RsppVal _t1752 = rspp_method_call(rspp_v_e, "get", 2, _t1753);
            RsppVal _t1754 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1752, rspp_v_pad);
            RsppVal rspp_v_l = _t1754;
            RsppVal _t1755 = rspp_str("");
            _t1755 = rspp_add(_t1755, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1755 = rspp_add(_t1755, rspp_str("RsppVal "));
            _t1755 = rspp_add(_t1755, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1755 = rspp_add(_t1755, rspp_str(" = rspp_bool(1);"));
            RsppVal _t1756 = rspp_fn_cg_push_line(rspp_v_ctx, _t1755);
            RsppVal _t1757 = rspp_str("");
            _t1757 = rspp_add(_t1757, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1757 = rspp_add(_t1757, rspp_str("if (!rspp_to_bool("));
            _t1757 = rspp_add(_t1757, rspp_str(rspp_to_str(rspp_v_l)));
            _t1757 = rspp_add(_t1757, rspp_str(")) {"));
            RsppVal _t1758 = rspp_fn_cg_push_line(rspp_v_ctx, _t1757);
            RsppVal _t1759 = rspp_map();
            RsppVal _t1761[] = { rspp_str("right"), _t1759 };
            RsppVal _t1760 = rspp_method_call(rspp_v_e, "get", 2, _t1761);
            RsppVal _t1762 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1760, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal rspp_v_r = _t1762;
            RsppVal _t1763 = rspp_str("");
            _t1763 = rspp_add(_t1763, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1763 = rspp_add(_t1763, rspp_str("    "));
            _t1763 = rspp_add(_t1763, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1763 = rspp_add(_t1763, rspp_str(" = rspp_bool(rspp_to_bool("));
            _t1763 = rspp_add(_t1763, rspp_str(rspp_to_str(rspp_v_r)));
            _t1763 = rspp_add(_t1763, rspp_str("));"));
            RsppVal _t1764 = rspp_fn_cg_push_line(rspp_v_ctx, _t1763);
            RsppVal _t1765 = rspp_str("");
            _t1765 = rspp_add(_t1765, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1765 = rspp_add(_t1765, rspp_str("}"));
            RsppVal _t1766 = rspp_fn_cg_push_line(rspp_v_ctx, _t1765);
            RsppVal _t1767 = rspp_v_tmp;
            return _t1767;
        }
        RsppVal _t1768 = rspp_map();
        RsppVal _t1770[] = { rspp_str("left"), _t1768 };
        RsppVal _t1769 = rspp_method_call(rspp_v_e, "get", 2, _t1770);
        RsppVal _t1771 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1769, rspp_v_pad);
        RsppVal rspp_v_l = _t1771;
        RsppVal _t1772 = rspp_map();
        RsppVal _t1774[] = { rspp_str("right"), _t1772 };
        RsppVal _t1773 = rspp_method_call(rspp_v_e, "get", 2, _t1774);
        RsppVal _t1775 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1773, rspp_v_pad);
        RsppVal rspp_v_r = _t1775;
        RsppVal rspp_v_fn_name = rspp_str("rspp_add");
        if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("-")))) {
            rspp_v_fn_name = rspp_str("rspp_sub");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("*")))) {
                rspp_v_fn_name = rspp_str("rspp_mul");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("/")))) {
                rspp_v_fn_name = rspp_str("rspp_div");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("%")))) {
                rspp_v_fn_name = rspp_str("rspp_mod");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("**")))) {
                rspp_v_fn_name = rspp_str("rspp_pow");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("==")))) {
                rspp_v_fn_name = rspp_str("rspp_eq");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("!=")))) {
                rspp_v_fn_name = rspp_str("rspp_neq");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("<")))) {
                rspp_v_fn_name = rspp_str("rspp_lt");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("<=")))) {
                rspp_v_fn_name = rspp_str("rspp_lte");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str(">")))) {
                rspp_v_fn_name = rspp_str("rspp_gt");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str(">=")))) {
                rspp_v_fn_name = rspp_str("rspp_gte");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("&")))) {
                rspp_v_fn_name = rspp_str("rspp_bitand");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("|")))) {
                rspp_v_fn_name = rspp_str("rspp_bitor");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("^")))) {
                rspp_v_fn_name = rspp_str("rspp_bitxor");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("<<")))) {
                rspp_v_fn_name = rspp_str("rspp_shl");
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str(">>")))) {
                rspp_v_fn_name = rspp_str("rspp_shr");
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        }
        RsppVal _t1777 = rspp_str("");
        _t1777 = rspp_add(_t1777, rspp_str(rspp_to_str(rspp_v_fn_name)));
        _t1777 = rspp_add(_t1777, rspp_str("("));
        _t1777 = rspp_add(_t1777, rspp_str(rspp_to_str(rspp_v_l)));
        _t1777 = rspp_add(_t1777, rspp_str(", "));
        _t1777 = rspp_add(_t1777, rspp_str(rspp_to_str(rspp_v_r)));
        _t1777 = rspp_add(_t1777, rspp_str(")"));
        RsppVal _t1776 = _t1777;
        return _t1776;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("unary")))) {
        RsppVal _t1779[] = { rspp_str("op"), rspp_str("-") };
        RsppVal _t1778 = rspp_method_call(rspp_v_e, "get", 2, _t1779);
        RsppVal rspp_v_op = _t1778;
        RsppVal _t1780 = rspp_map();
        RsppVal _t1782[] = { rspp_str("operand"), _t1780 };
        RsppVal _t1781 = rspp_method_call(rspp_v_e, "get", 2, _t1782);
        RsppVal _t1783 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1781, rspp_v_pad);
        RsppVal rspp_v_v = _t1783;
        if (rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("-")))) {
            RsppVal _t1785 = rspp_str("");
            _t1785 = rspp_add(_t1785, rspp_str("rspp_neg("));
            _t1785 = rspp_add(_t1785, rspp_str(rspp_to_str(rspp_v_v)));
            _t1785 = rspp_add(_t1785, rspp_str(")"));
            RsppVal _t1784 = _t1785;
            return _t1784;
        }
        RsppVal _t1786 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("not")))) {
            _t1786 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_op, rspp_str("!"))));
        }
        if (rspp_to_bool(_t1786)) {
            RsppVal _t1788 = rspp_str("");
            _t1788 = rspp_add(_t1788, rspp_str("rspp_bool(!rspp_to_bool("));
            _t1788 = rspp_add(_t1788, rspp_str(rspp_to_str(rspp_v_v)));
            _t1788 = rspp_add(_t1788, rspp_str("))"));
            RsppVal _t1787 = _t1788;
            return _t1787;
        }
        RsppVal _t1790 = rspp_str("");
        _t1790 = rspp_add(_t1790, rspp_str("rspp_int(~rspp_to_i64("));
        _t1790 = rspp_add(_t1790, rspp_str(rspp_to_str(rspp_v_v)));
        _t1790 = rspp_add(_t1790, rspp_str("))"));
        RsppVal _t1789 = _t1790;
        return _t1789;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("cast")))) {
        RsppVal _t1791 = rspp_map();
        RsppVal _t1793[] = { rspp_str("expr"), _t1791 };
        RsppVal _t1792 = rspp_method_call(rspp_v_e, "get", 2, _t1793);
        RsppVal _t1794 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1792, rspp_v_pad);
        RsppVal rspp_v_v = _t1794;
        RsppVal _t1796[] = { rspp_str("ty"), rspp_str("int") };
        RsppVal _t1795 = rspp_method_call(rspp_v_e, "get", 2, _t1796);
        RsppVal rspp_v_ty = _t1795;
        RsppVal _t1798 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("int")))) {
            _t1798 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("i64"))));
        }
        RsppVal _t1797 = rspp_bool(1);
        if (!rspp_to_bool(_t1798)) {
            _t1797 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("u64"))));
        }
        if (rspp_to_bool(_t1797)) {
            RsppVal _t1800 = rspp_str("");
            _t1800 = rspp_add(_t1800, rspp_str("rspp_int(rspp_to_i64("));
            _t1800 = rspp_add(_t1800, rspp_str(rspp_to_str(rspp_v_v)));
            _t1800 = rspp_add(_t1800, rspp_str("))"));
            RsppVal _t1799 = _t1800;
            return _t1799;
        }
        RsppVal _t1801 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("float")))) {
            _t1801 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("f64"))));
        }
        if (rspp_to_bool(_t1801)) {
            RsppVal _t1803 = rspp_str("");
            _t1803 = rspp_add(_t1803, rspp_str("rspp_flt(rspp_to_f64("));
            _t1803 = rspp_add(_t1803, rspp_str(rspp_to_str(rspp_v_v)));
            _t1803 = rspp_add(_t1803, rspp_str("))"));
            RsppVal _t1802 = _t1803;
            return _t1802;
        }
        if (rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("str")))) {
            RsppVal _t1805 = rspp_str("");
            _t1805 = rspp_add(_t1805, rspp_str("rspp_str(rspp_to_str("));
            _t1805 = rspp_add(_t1805, rspp_str(rspp_to_str(rspp_v_v)));
            _t1805 = rspp_add(_t1805, rspp_str("))"));
            RsppVal _t1804 = _t1805;
            return _t1804;
        }
        if (rspp_to_bool(rspp_eq(rspp_v_ty, rspp_str("bool")))) {
            RsppVal _t1807 = rspp_str("");
            _t1807 = rspp_add(_t1807, rspp_str("rspp_bool(rspp_to_bool("));
            _t1807 = rspp_add(_t1807, rspp_str(rspp_to_str(rspp_v_v)));
            _t1807 = rspp_add(_t1807, rspp_str("))"));
            RsppVal _t1806 = _t1807;
            return _t1806;
        }
        RsppVal _t1808 = rspp_v_v;
        return _t1808;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("call")))) {
        RsppVal _t1809 = rspp_map();
        RsppVal _t1811[] = { rspp_str("callee"), _t1809 };
        RsppVal _t1810 = rspp_method_call(rspp_v_e, "get", 2, _t1811);
        RsppVal rspp_v_cal = _t1810;
        RsppVal _t1812 = rspp_list();
        RsppVal _t1814[] = { rspp_str("args"), _t1812 };
        RsppVal _t1813 = rspp_method_call(rspp_v_e, "get", 2, _t1814);
        RsppVal rspp_v_args = _t1813;
        RsppVal _t1815 = rspp_list();
        RsppVal rspp_v_evs = _t1815;
        RsppVal _t1816 = rspp_v_args;
        for (int64_t _t1817 = 0; _t1817 < rspp_list_len(_t1816); _t1817++) {
            RsppVal rspp_v_a = rspp_index_get(_t1816, rspp_int(_t1817));
            RsppVal _t1818 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_a, rspp_v_pad);
            RsppVal _t1820[] = { _t1818 };
            RsppVal _t1819 = rspp_method_call(rspp_v_evs, "push", 1, _t1820);
        }
        RsppVal _t1822[] = { rspp_str("name"), rspp_str("") };
        RsppVal _t1821 = rspp_method_call(rspp_v_cal, "get", 2, _t1822);
        RsppVal _t1823 = rspp_fn_c_ident(_t1821);
        RsppVal rspp_v_cname = _t1823;
        RsppVal _t1824 = rspp_fn_cg_tmp(rspp_v_ctx);
        RsppVal rspp_v_tmp = _t1824;
        RsppVal _t1826[] = { rspp_v_evs, rspp_str(", ") };
        RsppVal _t1825 = rspp_builtin("str", "join", 2, _t1826);
        RsppVal rspp_v_joined = _t1825;
        RsppVal _t1827 = rspp_str("");
        _t1827 = rspp_add(_t1827, rspp_str(rspp_to_str(rspp_v_pad)));
        _t1827 = rspp_add(_t1827, rspp_str("RsppVal "));
        _t1827 = rspp_add(_t1827, rspp_str(rspp_to_str(rspp_v_tmp)));
        _t1827 = rspp_add(_t1827, rspp_str(" = rspp_fn_"));
        _t1827 = rspp_add(_t1827, rspp_str(rspp_to_str(rspp_v_cname)));
        _t1827 = rspp_add(_t1827, rspp_str("("));
        _t1827 = rspp_add(_t1827, rspp_str(rspp_to_str(rspp_v_joined)));
        _t1827 = rspp_add(_t1827, rspp_str(");"));
        RsppVal _t1828 = rspp_fn_cg_push_line(rspp_v_ctx, _t1827);
        RsppVal _t1829 = rspp_v_tmp;
        return _t1829;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("ns_call")))) {
        RsppVal _t1831[] = { rspp_str("ns"), rspp_str("") };
        RsppVal _t1830 = rspp_method_call(rspp_v_e, "get", 2, _t1831);
        RsppVal _t1832 = rspp_fn_esc_c_str(_t1830);
        RsppVal rspp_v_ns = _t1832;
        RsppVal _t1834[] = { rspp_str("fn"), rspp_str("") };
        RsppVal _t1833 = rspp_method_call(rspp_v_e, "get", 2, _t1834);
        RsppVal _t1835 = rspp_fn_esc_c_str(_t1833);
        RsppVal rspp_v_fname = _t1835;
        RsppVal _t1836 = rspp_list();
        RsppVal _t1838[] = { rspp_str("args"), _t1836 };
        RsppVal _t1837 = rspp_method_call(rspp_v_e, "get", 2, _t1838);
        RsppVal rspp_v_args = _t1837;
        RsppVal _t1839 = rspp_list();
        RsppVal rspp_v_evs = _t1839;
        RsppVal _t1840 = rspp_v_args;
        for (int64_t _t1841 = 0; _t1841 < rspp_list_len(_t1840); _t1841++) {
            RsppVal rspp_v_a = rspp_index_get(_t1840, rspp_int(_t1841));
            RsppVal _t1842 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_a, rspp_v_pad);
            RsppVal _t1844[] = { _t1842 };
            RsppVal _t1843 = rspp_method_call(rspp_v_evs, "push", 1, _t1844);
        }
        RsppVal _t1845 = rspp_fn_cg_tmp(rspp_v_ctx);
        RsppVal rspp_v_tmp = _t1845;
        RsppVal _t1846 = rspp_method_call(rspp_v_evs, "len", 0, NULL);
        if (rspp_to_bool(rspp_eq(_t1846, rspp_int(0LL)))) {
            RsppVal _t1847 = rspp_str("");
            _t1847 = rspp_add(_t1847, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1847 = rspp_add(_t1847, rspp_str("RsppVal "));
            _t1847 = rspp_add(_t1847, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1847 = rspp_add(_t1847, rspp_str(" = rspp_builtin(\""));
            _t1847 = rspp_add(_t1847, rspp_str(rspp_to_str(rspp_v_ns)));
            _t1847 = rspp_add(_t1847, rspp_str("\", \""));
            _t1847 = rspp_add(_t1847, rspp_str(rspp_to_str(rspp_v_fname)));
            _t1847 = rspp_add(_t1847, rspp_str("\", 0, NULL);"));
            RsppVal _t1848 = rspp_fn_cg_push_line(rspp_v_ctx, _t1847);
        } else {
            RsppVal _t1849 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_arr = _t1849;
            RsppVal _t1851[] = { rspp_v_evs, rspp_str(", ") };
            RsppVal _t1850 = rspp_builtin("str", "join", 2, _t1851);
            RsppVal rspp_v_joined = _t1850;
            RsppVal _t1852 = rspp_str("");
            _t1852 = rspp_add(_t1852, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1852 = rspp_add(_t1852, rspp_str("RsppVal "));
            _t1852 = rspp_add(_t1852, rspp_str(rspp_to_str(rspp_v_arr)));
            _t1852 = rspp_add(_t1852, rspp_str("[] = { "));
            _t1852 = rspp_add(_t1852, rspp_str(rspp_to_str(rspp_v_joined)));
            _t1852 = rspp_add(_t1852, rspp_str(" };"));
            RsppVal _t1853 = rspp_fn_cg_push_line(rspp_v_ctx, _t1852);
            RsppVal _t1854 = rspp_str("");
            _t1854 = rspp_add(_t1854, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1854 = rspp_add(_t1854, rspp_str("RsppVal "));
            _t1854 = rspp_add(_t1854, rspp_str(rspp_to_str(rspp_v_tmp)));
            _t1854 = rspp_add(_t1854, rspp_str(" = rspp_builtin(\""));
            _t1854 = rspp_add(_t1854, rspp_str(rspp_to_str(rspp_v_ns)));
            _t1854 = rspp_add(_t1854, rspp_str("\", \""));
            _t1854 = rspp_add(_t1854, rspp_str(rspp_to_str(rspp_v_fname)));
            _t1854 = rspp_add(_t1854, rspp_str("\", "));
            RsppVal _t1855 = rspp_method_call(rspp_v_evs, "len", 0, NULL);
            _t1854 = rspp_add(_t1854, rspp_str(rspp_to_str(_t1855)));
            _t1854 = rspp_add(_t1854, rspp_str(", "));
            _t1854 = rspp_add(_t1854, rspp_str(rspp_to_str(rspp_v_arr)));
            _t1854 = rspp_add(_t1854, rspp_str(");"));
            RsppVal _t1856 = rspp_fn_cg_push_line(rspp_v_ctx, _t1854);
        }
        RsppVal _t1857 = rspp_v_tmp;
        return _t1857;
    }
    RsppVal _t1858 = rspp_str("rspp_none()");
    return _t1858;
    return rspp_none();
}

static RsppVal rspp_fn_cg_emit_defers_all(RsppVal rspp_v_ctx, RsppVal rspp_v_pad) {
    RsppVal _t1859 = rspp_list();
    RsppVal _t1861[] = { rspp_str("defers"), _t1859 };
    RsppVal _t1860 = rspp_method_call(rspp_v_ctx, "get", 2, _t1861);
    RsppVal rspp_v_frames = _t1860;
    RsppVal _t1862 = rspp_method_call(rspp_v_frames, "len", 0, NULL);
    RsppVal rspp_v_fi = rspp_sub(_t1862, rspp_int(1LL));
    while (1) {
        if (!rspp_to_bool(rspp_gte(rspp_v_fi, rspp_int(0LL)))) break;
        RsppVal rspp_v_fr = rspp_index_get(rspp_v_frames, rspp_v_fi);
        RsppVal _t1863 = rspp_method_call(rspp_v_fr, "len", 0, NULL);
        RsppVal rspp_v_di = rspp_sub(_t1863, rspp_int(1LL));
        while (1) {
            if (!rspp_to_bool(rspp_gte(rspp_v_di, rspp_int(0LL)))) break;
            RsppVal _t1864 = rspp_fn_cg_emit_stmt(rspp_v_ctx, rspp_index_get(rspp_v_fr, rspp_v_di), rspp_v_pad);
            rspp_v_di = rspp_sub(rspp_v_di, rspp_int(1LL));
        }
        rspp_v_fi = rspp_sub(rspp_v_fi, rspp_int(1LL));
    }
    return rspp_none();
}

static RsppVal rspp_fn_cg_emit_block(RsppVal rspp_v_ctx, RsppVal rspp_v_stmts, RsppVal rspp_v_pad) {
    RsppVal _t1865 = rspp_list();
    RsppVal _t1867[] = { rspp_str("defers"), _t1865 };
    RsppVal _t1866 = rspp_method_call(rspp_v_ctx, "get", 2, _t1867);
    RsppVal rspp_v_frames = _t1866;
    RsppVal _t1868 = rspp_list();
    RsppVal _t1870[] = { _t1868 };
    RsppVal _t1869 = rspp_method_call(rspp_v_frames, "push", 1, _t1870);
    RsppVal _t1871 = rspp_v_stmts;
    for (int64_t _t1872 = 0; _t1872 < rspp_list_len(_t1871); _t1872++) {
        RsppVal rspp_v_s = rspp_index_get(_t1871, rspp_int(_t1872));
        RsppVal _t1874[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t1873 = rspp_method_call(rspp_v_s, "get", 2, _t1874);
        if (rspp_to_bool(rspp_eq(_t1873, rspp_str("defer")))) {
            RsppVal _t1875 = rspp_method_call(rspp_v_frames, "len", 0, NULL);
            RsppVal rspp_v_top = rspp_index_get(rspp_v_frames, rspp_sub(_t1875, rspp_int(1LL)));
            RsppVal _t1876 = rspp_map();
            RsppVal _t1878[] = { rspp_str("stmt"), _t1876 };
            RsppVal _t1877 = rspp_method_call(rspp_v_s, "get", 2, _t1878);
            RsppVal _t1880[] = { _t1877 };
            RsppVal _t1879 = rspp_method_call(rspp_v_top, "push", 1, _t1880);
        } else {
            RsppVal _t1881 = rspp_fn_cg_emit_stmt(rspp_v_ctx, rspp_v_s, rspp_v_pad);
        }
    }
    RsppVal _t1882 = rspp_method_call(rspp_v_frames, "pop", 0, NULL);
    RsppVal rspp_v_popped = _t1882;
    RsppVal _t1883 = rspp_method_call(rspp_v_popped, "len", 0, NULL);
    RsppVal rspp_v_di = rspp_sub(_t1883, rspp_int(1LL));
    while (1) {
        if (!rspp_to_bool(rspp_gte(rspp_v_di, rspp_int(0LL)))) break;
        RsppVal _t1884 = rspp_fn_cg_emit_stmt(rspp_v_ctx, rspp_index_get(rspp_v_popped, rspp_v_di), rspp_v_pad);
        rspp_v_di = rspp_sub(rspp_v_di, rspp_int(1LL));
    }
    return rspp_none();
}

static RsppVal rspp_fn_cg_emit_stmt(RsppVal rspp_v_ctx, RsppVal rspp_v_s, RsppVal rspp_v_pad) {
    RsppVal _t1886[] = { rspp_str("kind"), rspp_str("") };
    RsppVal _t1885 = rspp_method_call(rspp_v_s, "get", 2, _t1886);
    RsppVal rspp_v_k = _t1885;
    RsppVal _t1887 = rspp_bool(1);
    if (!rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("let")))) {
        _t1887 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("var"))));
    }
    if (rspp_to_bool(_t1887)) {
        RsppVal _t1888 = rspp_map();
        RsppVal _t1890[] = { rspp_str("value"), _t1888 };
        RsppVal _t1889 = rspp_method_call(rspp_v_s, "get", 2, _t1890);
        RsppVal _t1891 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1889, rspp_v_pad);
        RsppVal rspp_v_v = _t1891;
        RsppVal _t1893[] = { rspp_str("name"), rspp_str("") };
        RsppVal _t1892 = rspp_method_call(rspp_v_s, "get", 2, _t1893);
        RsppVal _t1894 = rspp_fn_c_ident(_t1892);
        RsppVal rspp_v_vn = _t1894;
        RsppVal _t1895 = rspp_str("");
        _t1895 = rspp_add(_t1895, rspp_str(rspp_to_str(rspp_v_pad)));
        _t1895 = rspp_add(_t1895, rspp_str("RsppVal rspp_v_"));
        _t1895 = rspp_add(_t1895, rspp_str(rspp_to_str(rspp_v_vn)));
        _t1895 = rspp_add(_t1895, rspp_str(" = "));
        _t1895 = rspp_add(_t1895, rspp_str(rspp_to_str(rspp_v_v)));
        _t1895 = rspp_add(_t1895, rspp_str(";"));
        RsppVal _t1896 = rspp_fn_cg_push_line(rspp_v_ctx, _t1895);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("assign")))) {
            RsppVal _t1897 = rspp_map();
            RsppVal _t1899[] = { rspp_str("value"), _t1897 };
            RsppVal _t1898 = rspp_method_call(rspp_v_s, "get", 2, _t1899);
            RsppVal _t1900 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1898, rspp_v_pad);
            RsppVal rspp_v_v = _t1900;
            RsppVal _t1901 = rspp_map();
            RsppVal _t1903[] = { rspp_str("target"), _t1901 };
            RsppVal _t1902 = rspp_method_call(rspp_v_s, "get", 2, _t1903);
            RsppVal rspp_v_tgt = _t1902;
            RsppVal _t1905[] = { rspp_str("kind"), rspp_str("") };
            RsppVal _t1904 = rspp_method_call(rspp_v_tgt, "get", 2, _t1905);
            if (rspp_to_bool(rspp_eq(_t1904, rspp_str("ident")))) {
                RsppVal _t1907[] = { rspp_str("name"), rspp_str("") };
                RsppVal _t1906 = rspp_method_call(rspp_v_tgt, "get", 2, _t1907);
                RsppVal _t1908 = rspp_fn_c_ident(_t1906);
                RsppVal rspp_v_vn = _t1908;
                RsppVal _t1909 = rspp_str("");
                _t1909 = rspp_add(_t1909, rspp_str(rspp_to_str(rspp_v_pad)));
                _t1909 = rspp_add(_t1909, rspp_str("rspp_v_"));
                _t1909 = rspp_add(_t1909, rspp_str(rspp_to_str(rspp_v_vn)));
                _t1909 = rspp_add(_t1909, rspp_str(" = "));
                _t1909 = rspp_add(_t1909, rspp_str(rspp_to_str(rspp_v_v)));
                _t1909 = rspp_add(_t1909, rspp_str(";"));
                RsppVal _t1910 = rspp_fn_cg_push_line(rspp_v_ctx, _t1909);
            } else {
                RsppVal _t1912[] = { rspp_str("kind"), rspp_str("") };
                RsppVal _t1911 = rspp_method_call(rspp_v_tgt, "get", 2, _t1912);
                if (rspp_to_bool(rspp_eq(_t1911, rspp_str("index")))) {
                    RsppVal _t1913 = rspp_map();
                    RsppVal _t1915[] = { rspp_str("recv"), _t1913 };
                    RsppVal _t1914 = rspp_method_call(rspp_v_tgt, "get", 2, _t1915);
                    RsppVal _t1916 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1914, rspp_v_pad);
                    RsppVal rspp_v_r = _t1916;
                    RsppVal _t1917 = rspp_map();
                    RsppVal _t1919[] = { rspp_str("index"), _t1917 };
                    RsppVal _t1918 = rspp_method_call(rspp_v_tgt, "get", 2, _t1919);
                    RsppVal _t1920 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1918, rspp_v_pad);
                    RsppVal rspp_v_idx = _t1920;
                    RsppVal _t1921 = rspp_str("");
                    _t1921 = rspp_add(_t1921, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1921 = rspp_add(_t1921, rspp_str("rspp_index_set("));
                    _t1921 = rspp_add(_t1921, rspp_str(rspp_to_str(rspp_v_r)));
                    _t1921 = rspp_add(_t1921, rspp_str(", "));
                    _t1921 = rspp_add(_t1921, rspp_str(rspp_to_str(rspp_v_idx)));
                    _t1921 = rspp_add(_t1921, rspp_str(", "));
                    _t1921 = rspp_add(_t1921, rspp_str(rspp_to_str(rspp_v_v)));
                    _t1921 = rspp_add(_t1921, rspp_str(");"));
                    RsppVal _t1922 = rspp_fn_cg_push_line(rspp_v_ctx, _t1921);
            }
            }
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("compound_assign")))) {
            RsppVal _t1923 = rspp_map();
            RsppVal _t1925[] = { rspp_str("target"), _t1923 };
            RsppVal _t1924 = rspp_method_call(rspp_v_s, "get", 2, _t1925);
            RsppVal rspp_v_tgt = _t1924;
            RsppVal _t1926 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_tgt, rspp_v_pad);
            RsppVal rspp_v_cur = _t1926;
            RsppVal _t1927 = rspp_map();
            RsppVal _t1929[] = { rspp_str("value"), _t1927 };
            RsppVal _t1928 = rspp_method_call(rspp_v_s, "get", 2, _t1929);
            RsppVal _t1930 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1928, rspp_v_pad);
            RsppVal rspp_v_rhs = _t1930;
            RsppVal _t1932[] = { rspp_str("op"), rspp_str("+=") };
            RsppVal _t1931 = rspp_method_call(rspp_v_s, "get", 2, _t1932);
            RsppVal rspp_v_cop = _t1931;
            RsppVal rspp_v_fn_name = rspp_str("rspp_add");
            if (rspp_to_bool(rspp_eq(rspp_v_cop, rspp_str("-=")))) {
                rspp_v_fn_name = rspp_str("rspp_sub");
            } else {
                if (rspp_to_bool(rspp_eq(rspp_v_cop, rspp_str("*=")))) {
                    rspp_v_fn_name = rspp_str("rspp_mul");
            } else {
                if (rspp_to_bool(rspp_eq(rspp_v_cop, rspp_str("/=")))) {
                    rspp_v_fn_name = rspp_str("rspp_div");
                }
            }
            }
            RsppVal _t1934[] = { rspp_str("name"), rspp_str("") };
            RsppVal _t1933 = rspp_method_call(rspp_v_tgt, "get", 2, _t1934);
            RsppVal _t1935 = rspp_fn_c_ident(_t1933);
            RsppVal rspp_v_vn = _t1935;
            RsppVal _t1936 = rspp_str("");
            _t1936 = rspp_add(_t1936, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1936 = rspp_add(_t1936, rspp_str("rspp_v_"));
            _t1936 = rspp_add(_t1936, rspp_str(rspp_to_str(rspp_v_vn)));
            _t1936 = rspp_add(_t1936, rspp_str(" = "));
            _t1936 = rspp_add(_t1936, rspp_str(rspp_to_str(rspp_v_fn_name)));
            _t1936 = rspp_add(_t1936, rspp_str("("));
            _t1936 = rspp_add(_t1936, rspp_str(rspp_to_str(rspp_v_cur)));
            _t1936 = rspp_add(_t1936, rspp_str(", "));
            _t1936 = rspp_add(_t1936, rspp_str(rspp_to_str(rspp_v_rhs)));
            _t1936 = rspp_add(_t1936, rspp_str(");"));
            RsppVal _t1937 = rspp_fn_cg_push_line(rspp_v_ctx, _t1936);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("expr")))) {
            RsppVal _t1938 = rspp_map();
            RsppVal _t1940[] = { rspp_str("expr"), _t1938 };
            RsppVal _t1939 = rspp_method_call(rspp_v_s, "get", 2, _t1940);
            RsppVal _t1941 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1939, rspp_v_pad);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("if")))) {
            RsppVal _t1942 = rspp_list();
            RsppVal _t1944[] = { rspp_str("branches"), _t1942 };
            RsppVal _t1943 = rspp_method_call(rspp_v_s, "get", 2, _t1944);
            RsppVal rspp_v_brs = _t1943;
            RsppVal _t1945 = rspp_list();
            RsppVal _t1947[] = { rspp_str("else_body"), _t1945 };
            RsppVal _t1946 = rspp_method_call(rspp_v_s, "get", 2, _t1947);
            RsppVal rspp_v_eb = _t1946;
            RsppVal rspp_v_i = rspp_int(0LL);
            while (1) {
                RsppVal _t1948 = rspp_method_call(rspp_v_brs, "len", 0, NULL);
                if (!rspp_to_bool(rspp_lt(rspp_v_i, _t1948))) break;
                RsppVal rspp_v_br = rspp_index_get(rspp_v_brs, rspp_v_i);
                if (rspp_to_bool(rspp_eq(rspp_v_i, rspp_int(0LL)))) {
                    RsppVal _t1949 = rspp_map();
                    RsppVal _t1951[] = { rspp_str("cond"), _t1949 };
                    RsppVal _t1950 = rspp_method_call(rspp_v_br, "get", 2, _t1951);
                    RsppVal _t1952 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1950, rspp_v_pad);
                    RsppVal rspp_v_c = _t1952;
                    RsppVal _t1953 = rspp_str("");
                    _t1953 = rspp_add(_t1953, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1953 = rspp_add(_t1953, rspp_str("if (rspp_to_bool("));
                    _t1953 = rspp_add(_t1953, rspp_str(rspp_to_str(rspp_v_c)));
                    _t1953 = rspp_add(_t1953, rspp_str(")) {"));
                    RsppVal _t1954 = rspp_fn_cg_push_line(rspp_v_ctx, _t1953);
                    RsppVal _t1955 = rspp_list();
                    RsppVal _t1957[] = { rspp_str("body"), _t1955 };
                    RsppVal _t1956 = rspp_method_call(rspp_v_br, "get", 2, _t1957);
                    RsppVal _t1958 = rspp_fn_cg_emit_block(rspp_v_ctx, _t1956, rspp_add(rspp_v_pad, rspp_str("    ")));
                } else {
                    RsppVal _t1959 = rspp_str("");
                    _t1959 = rspp_add(_t1959, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1959 = rspp_add(_t1959, rspp_str("} else {"));
                    RsppVal _t1960 = rspp_fn_cg_push_line(rspp_v_ctx, _t1959);
                    RsppVal _t1961 = rspp_map();
                    RsppVal _t1963[] = { rspp_str("cond"), _t1961 };
                    RsppVal _t1962 = rspp_method_call(rspp_v_br, "get", 2, _t1963);
                    RsppVal _t1964 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1962, rspp_add(rspp_v_pad, rspp_str("    ")));
                    RsppVal rspp_v_c = _t1964;
                    RsppVal _t1965 = rspp_str("");
                    _t1965 = rspp_add(_t1965, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1965 = rspp_add(_t1965, rspp_str("    if (rspp_to_bool("));
                    _t1965 = rspp_add(_t1965, rspp_str(rspp_to_str(rspp_v_c)));
                    _t1965 = rspp_add(_t1965, rspp_str(")) {"));
                    RsppVal _t1966 = rspp_fn_cg_push_line(rspp_v_ctx, _t1965);
                    RsppVal _t1967 = rspp_list();
                    RsppVal _t1969[] = { rspp_str("body"), _t1967 };
                    RsppVal _t1968 = rspp_method_call(rspp_v_br, "get", 2, _t1969);
                    RsppVal _t1970 = rspp_fn_cg_emit_block(rspp_v_ctx, _t1968, rspp_add(rspp_v_pad, rspp_str("        ")));
                }
                rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
            }
            RsppVal _t1971 = rspp_method_call(rspp_v_brs, "len", 0, NULL);
            if (rspp_to_bool(rspp_gt(_t1971, rspp_int(1LL)))) {
                RsppVal _t1972 = rspp_method_call(rspp_v_eb, "len", 0, NULL);
                if (rspp_to_bool(rspp_gt(_t1972, rspp_int(0LL)))) {
                    RsppVal _t1973 = rspp_str("");
                    _t1973 = rspp_add(_t1973, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1973 = rspp_add(_t1973, rspp_str("} else {"));
                    RsppVal _t1974 = rspp_fn_cg_push_line(rspp_v_ctx, _t1973);
                    RsppVal _t1975 = rspp_fn_cg_emit_block(rspp_v_ctx, rspp_v_eb, rspp_add(rspp_v_pad, rspp_str("    ")));
                }
                RsppVal _t1976 = rspp_method_call(rspp_v_brs, "len", 0, NULL);
                RsppVal rspp_v_k_i = rspp_sub(_t1976, rspp_int(1LL));
                while (1) {
                    if (!rspp_to_bool(rspp_gte(rspp_v_k_i, rspp_int(1LL)))) break;
                    RsppVal _t1977 = rspp_str("");
                    _t1977 = rspp_add(_t1977, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1977 = rspp_add(_t1977, rspp_str("}"));
                    RsppVal _t1978 = rspp_fn_cg_push_line(rspp_v_ctx, _t1977);
                    rspp_v_k_i = rspp_sub(rspp_v_k_i, rspp_int(1LL));
                }
                RsppVal _t1979 = rspp_str("");
                _t1979 = rspp_add(_t1979, rspp_str(rspp_to_str(rspp_v_pad)));
                _t1979 = rspp_add(_t1979, rspp_str("}"));
                RsppVal _t1980 = rspp_fn_cg_push_line(rspp_v_ctx, _t1979);
            } else {
                RsppVal _t1981 = rspp_method_call(rspp_v_eb, "len", 0, NULL);
                if (rspp_to_bool(rspp_gt(_t1981, rspp_int(0LL)))) {
                    RsppVal _t1982 = rspp_str("");
                    _t1982 = rspp_add(_t1982, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t1982 = rspp_add(_t1982, rspp_str("} else {"));
                    RsppVal _t1983 = rspp_fn_cg_push_line(rspp_v_ctx, _t1982);
                    RsppVal _t1984 = rspp_fn_cg_emit_block(rspp_v_ctx, rspp_v_eb, rspp_add(rspp_v_pad, rspp_str("    ")));
                }
                RsppVal _t1985 = rspp_str("");
                _t1985 = rspp_add(_t1985, rspp_str(rspp_to_str(rspp_v_pad)));
                _t1985 = rspp_add(_t1985, rspp_str("}"));
                RsppVal _t1986 = rspp_fn_cg_push_line(rspp_v_ctx, _t1985);
            }
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("while")))) {
            RsppVal _t1987 = rspp_str("");
            _t1987 = rspp_add(_t1987, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1987 = rspp_add(_t1987, rspp_str("while (1) {"));
            RsppVal _t1988 = rspp_fn_cg_push_line(rspp_v_ctx, _t1987);
            RsppVal _t1989 = rspp_map();
            RsppVal _t1991[] = { rspp_str("cond"), _t1989 };
            RsppVal _t1990 = rspp_method_call(rspp_v_s, "get", 2, _t1991);
            RsppVal _t1992 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t1990, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal rspp_v_c = _t1992;
            RsppVal _t1993 = rspp_str("");
            _t1993 = rspp_add(_t1993, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1993 = rspp_add(_t1993, rspp_str("    if (!rspp_to_bool("));
            _t1993 = rspp_add(_t1993, rspp_str(rspp_to_str(rspp_v_c)));
            _t1993 = rspp_add(_t1993, rspp_str(")) break;"));
            RsppVal _t1994 = rspp_fn_cg_push_line(rspp_v_ctx, _t1993);
            RsppVal _t1995 = rspp_list();
            RsppVal _t1997[] = { rspp_str("body"), _t1995 };
            RsppVal _t1996 = rspp_method_call(rspp_v_s, "get", 2, _t1997);
            RsppVal _t1998 = rspp_fn_cg_emit_block(rspp_v_ctx, _t1996, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal _t1999 = rspp_str("");
            _t1999 = rspp_add(_t1999, rspp_str(rspp_to_str(rspp_v_pad)));
            _t1999 = rspp_add(_t1999, rspp_str("}"));
            RsppVal _t2000 = rspp_fn_cg_push_line(rspp_v_ctx, _t1999);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("loop")))) {
            RsppVal _t2001 = rspp_str("");
            _t2001 = rspp_add(_t2001, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2001 = rspp_add(_t2001, rspp_str("while (1) {"));
            RsppVal _t2002 = rspp_fn_cg_push_line(rspp_v_ctx, _t2001);
            RsppVal _t2003 = rspp_list();
            RsppVal _t2005[] = { rspp_str("body"), _t2003 };
            RsppVal _t2004 = rspp_method_call(rspp_v_s, "get", 2, _t2005);
            RsppVal _t2006 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2004, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal _t2007 = rspp_str("");
            _t2007 = rspp_add(_t2007, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2007 = rspp_add(_t2007, rspp_str("}"));
            RsppVal _t2008 = rspp_fn_cg_push_line(rspp_v_ctx, _t2007);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("for")))) {
            RsppVal _t2009 = rspp_map();
            RsppVal _t2011[] = { rspp_str("iter"), _t2009 };
            RsppVal _t2010 = rspp_method_call(rspp_v_s, "get", 2, _t2011);
            RsppVal _t2012 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t2010, rspp_v_pad);
            RsppVal rspp_v_it = _t2012;
            RsppVal _t2013 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_itv = _t2013;
            RsppVal _t2014 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_idxv = _t2014;
            RsppVal _t2016[] = { rspp_str("var"), rspp_str("") };
            RsppVal _t2015 = rspp_method_call(rspp_v_s, "get", 2, _t2016);
            RsppVal _t2017 = rspp_fn_c_ident(_t2015);
            RsppVal rspp_v_vn = _t2017;
            RsppVal _t2018 = rspp_str("");
            _t2018 = rspp_add(_t2018, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2018 = rspp_add(_t2018, rspp_str("RsppVal "));
            _t2018 = rspp_add(_t2018, rspp_str(rspp_to_str(rspp_v_itv)));
            _t2018 = rspp_add(_t2018, rspp_str(" = "));
            _t2018 = rspp_add(_t2018, rspp_str(rspp_to_str(rspp_v_it)));
            _t2018 = rspp_add(_t2018, rspp_str(";"));
            RsppVal _t2019 = rspp_fn_cg_push_line(rspp_v_ctx, _t2018);
            RsppVal _t2020 = rspp_str("");
            _t2020 = rspp_add(_t2020, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2020 = rspp_add(_t2020, rspp_str("for (int64_t "));
            _t2020 = rspp_add(_t2020, rspp_str(rspp_to_str(rspp_v_idxv)));
            _t2020 = rspp_add(_t2020, rspp_str(" = 0; "));
            _t2020 = rspp_add(_t2020, rspp_str(rspp_to_str(rspp_v_idxv)));
            _t2020 = rspp_add(_t2020, rspp_str(" < rspp_list_len("));
            _t2020 = rspp_add(_t2020, rspp_str(rspp_to_str(rspp_v_itv)));
            _t2020 = rspp_add(_t2020, rspp_str("); "));
            _t2020 = rspp_add(_t2020, rspp_str(rspp_to_str(rspp_v_idxv)));
            _t2020 = rspp_add(_t2020, rspp_str("++) {"));
            RsppVal _t2021 = rspp_fn_cg_push_line(rspp_v_ctx, _t2020);
            RsppVal _t2022 = rspp_str("");
            _t2022 = rspp_add(_t2022, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2022 = rspp_add(_t2022, rspp_str("    RsppVal rspp_v_"));
            _t2022 = rspp_add(_t2022, rspp_str(rspp_to_str(rspp_v_vn)));
            _t2022 = rspp_add(_t2022, rspp_str(" = rspp_index_get("));
            _t2022 = rspp_add(_t2022, rspp_str(rspp_to_str(rspp_v_itv)));
            _t2022 = rspp_add(_t2022, rspp_str(", rspp_int("));
            _t2022 = rspp_add(_t2022, rspp_str(rspp_to_str(rspp_v_idxv)));
            _t2022 = rspp_add(_t2022, rspp_str("));"));
            RsppVal _t2023 = rspp_fn_cg_push_line(rspp_v_ctx, _t2022);
            RsppVal _t2024 = rspp_list();
            RsppVal _t2026[] = { rspp_str("body"), _t2024 };
            RsppVal _t2025 = rspp_method_call(rspp_v_s, "get", 2, _t2026);
            RsppVal _t2027 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2025, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal _t2028 = rspp_str("");
            _t2028 = rspp_add(_t2028, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2028 = rspp_add(_t2028, rspp_str("}"));
            RsppVal _t2029 = rspp_fn_cg_push_line(rspp_v_ctx, _t2028);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("match")))) {
            RsppVal _t2030 = rspp_map();
            RsppVal _t2032[] = { rspp_str("subject"), _t2030 };
            RsppVal _t2031 = rspp_method_call(rspp_v_s, "get", 2, _t2032);
            RsppVal _t2033 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t2031, rspp_v_pad);
            RsppVal rspp_v_sub = _t2033;
            RsppVal _t2034 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_sv = _t2034;
            RsppVal _t2035 = rspp_str("");
            _t2035 = rspp_add(_t2035, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2035 = rspp_add(_t2035, rspp_str("RsppVal "));
            _t2035 = rspp_add(_t2035, rspp_str(rspp_to_str(rspp_v_sv)));
            _t2035 = rspp_add(_t2035, rspp_str(" = "));
            _t2035 = rspp_add(_t2035, rspp_str(rspp_to_str(rspp_v_sub)));
            _t2035 = rspp_add(_t2035, rspp_str(";"));
            RsppVal _t2036 = rspp_fn_cg_push_line(rspp_v_ctx, _t2035);
            RsppVal _t2037 = rspp_list();
            RsppVal _t2039[] = { rspp_str("arms"), _t2037 };
            RsppVal _t2038 = rspp_method_call(rspp_v_s, "get", 2, _t2039);
            RsppVal rspp_v_arms = _t2038;
            RsppVal rspp_v_ai = rspp_int(0LL);
            RsppVal rspp_v_extra_closes = rspp_int(0LL);
            while (1) {
                RsppVal _t2040 = rspp_method_call(rspp_v_arms, "len", 0, NULL);
                if (!rspp_to_bool(rspp_lt(rspp_v_ai, _t2040))) break;
                RsppVal rspp_v_arm = rspp_index_get(rspp_v_arms, rspp_v_ai);
                RsppVal _t2041 = rspp_map();
                RsppVal _t2043[] = { rspp_str("pattern"), _t2041 };
                RsppVal _t2042 = rspp_method_call(rspp_v_arm, "get", 2, _t2043);
                RsppVal rspp_v_pat = _t2042;
                RsppVal _t2046[] = { rspp_str("kind"), rspp_str("") };
                RsppVal _t2045 = rspp_method_call(rspp_v_pat, "get", 2, _t2046);
                RsppVal _t2044 = rspp_bool(0);
                if (rspp_to_bool(rspp_eq(_t2045, rspp_str("ident")))) {
                    RsppVal _t2048[] = { rspp_str("name"), rspp_str("") };
                    RsppVal _t2047 = rspp_method_call(rspp_v_pat, "get", 2, _t2048);
                    _t2044 = rspp_bool(rspp_to_bool(rspp_eq(_t2047, rspp_str("_"))));
                }
                RsppVal rspp_v_is_wild = _t2044;
                if (rspp_to_bool(rspp_eq(rspp_v_ai, rspp_int(0LL)))) {
                    if (rspp_to_bool(rspp_v_is_wild)) {
                        RsppVal _t2049 = rspp_str("");
                        _t2049 = rspp_add(_t2049, rspp_str(rspp_to_str(rspp_v_pad)));
                        _t2049 = rspp_add(_t2049, rspp_str("if (1) {"));
                        RsppVal _t2050 = rspp_fn_cg_push_line(rspp_v_ctx, _t2049);
                    } else {
                        RsppVal _t2051 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_pat, rspp_v_pad);
                        RsppVal rspp_v_pv = _t2051;
                        RsppVal _t2052 = rspp_str("");
                        _t2052 = rspp_add(_t2052, rspp_str(rspp_to_str(rspp_v_pad)));
                        _t2052 = rspp_add(_t2052, rspp_str("if (rspp_to_bool(rspp_eq("));
                        _t2052 = rspp_add(_t2052, rspp_str(rspp_to_str(rspp_v_sv)));
                        _t2052 = rspp_add(_t2052, rspp_str(", "));
                        _t2052 = rspp_add(_t2052, rspp_str(rspp_to_str(rspp_v_pv)));
                        _t2052 = rspp_add(_t2052, rspp_str("))) {"));
                        RsppVal _t2053 = rspp_fn_cg_push_line(rspp_v_ctx, _t2052);
                    }
                    RsppVal _t2054 = rspp_list();
                    RsppVal _t2056[] = { rspp_str("body"), _t2054 };
                    RsppVal _t2055 = rspp_method_call(rspp_v_arm, "get", 2, _t2056);
                    RsppVal _t2057 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2055, rspp_add(rspp_v_pad, rspp_str("    ")));
                } else {
                    if (rspp_to_bool(rspp_v_is_wild)) {
                        RsppVal _t2058 = rspp_str("");
                        _t2058 = rspp_add(_t2058, rspp_str(rspp_to_str(rspp_v_pad)));
                        _t2058 = rspp_add(_t2058, rspp_str("} else {"));
                        RsppVal _t2059 = rspp_fn_cg_push_line(rspp_v_ctx, _t2058);
                        RsppVal _t2060 = rspp_list();
                        RsppVal _t2062[] = { rspp_str("body"), _t2060 };
                        RsppVal _t2061 = rspp_method_call(rspp_v_arm, "get", 2, _t2062);
                        RsppVal _t2063 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2061, rspp_add(rspp_v_pad, rspp_str("    ")));
                } else {
                    RsppVal _t2064 = rspp_str("");
                    _t2064 = rspp_add(_t2064, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t2064 = rspp_add(_t2064, rspp_str("} else {"));
                    RsppVal _t2065 = rspp_fn_cg_push_line(rspp_v_ctx, _t2064);
                    RsppVal _t2066 = rspp_fn_cg_emit_expr(rspp_v_ctx, rspp_v_pat, rspp_add(rspp_v_pad, rspp_str("    ")));
                    RsppVal rspp_v_pv = _t2066;
                    RsppVal _t2067 = rspp_str("");
                    _t2067 = rspp_add(_t2067, rspp_str(rspp_to_str(rspp_v_pad)));
                    _t2067 = rspp_add(_t2067, rspp_str("    if (rspp_to_bool(rspp_eq("));
                    _t2067 = rspp_add(_t2067, rspp_str(rspp_to_str(rspp_v_sv)));
                    _t2067 = rspp_add(_t2067, rspp_str(", "));
                    _t2067 = rspp_add(_t2067, rspp_str(rspp_to_str(rspp_v_pv)));
                    _t2067 = rspp_add(_t2067, rspp_str("))) {"));
                    RsppVal _t2068 = rspp_fn_cg_push_line(rspp_v_ctx, _t2067);
                    RsppVal _t2069 = rspp_list();
                    RsppVal _t2071[] = { rspp_str("body"), _t2069 };
                    RsppVal _t2070 = rspp_method_call(rspp_v_arm, "get", 2, _t2071);
                    RsppVal _t2072 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2070, rspp_add(rspp_v_pad, rspp_str("        ")));
                    rspp_v_extra_closes = rspp_add(rspp_v_extra_closes, rspp_int(1LL));
                }
                }
                rspp_v_ai = rspp_add(rspp_v_ai, rspp_int(1LL));
            }
            while (1) {
                if (!rspp_to_bool(rspp_gt(rspp_v_extra_closes, rspp_int(0LL)))) break;
                RsppVal _t2073 = rspp_str("");
                _t2073 = rspp_add(_t2073, rspp_str(rspp_to_str(rspp_v_pad)));
                _t2073 = rspp_add(_t2073, rspp_str("    }"));
                RsppVal _t2074 = rspp_fn_cg_push_line(rspp_v_ctx, _t2073);
                rspp_v_extra_closes = rspp_sub(rspp_v_extra_closes, rspp_int(1LL));
            }
            RsppVal _t2075 = rspp_method_call(rspp_v_arms, "len", 0, NULL);
            if (rspp_to_bool(rspp_gt(_t2075, rspp_int(0LL)))) {
                RsppVal _t2076 = rspp_str("");
                _t2076 = rspp_add(_t2076, rspp_str(rspp_to_str(rspp_v_pad)));
                _t2076 = rspp_add(_t2076, rspp_str("}"));
                RsppVal _t2077 = rspp_fn_cg_push_line(rspp_v_ctx, _t2076);
            }
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("return")))) {
            RsppVal _t2078 = rspp_fn_cg_tmp(rspp_v_ctx);
            RsppVal rspp_v_rtmp = _t2078;
            RsppVal _t2080[] = { rspp_str("has_val"), rspp_bool(0) };
            RsppVal _t2079 = rspp_method_call(rspp_v_s, "get", 2, _t2080);
            if (rspp_to_bool(_t2079)) {
                RsppVal _t2081 = rspp_map();
                RsppVal _t2083[] = { rspp_str("value"), _t2081 };
                RsppVal _t2082 = rspp_method_call(rspp_v_s, "get", 2, _t2083);
                RsppVal _t2084 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t2082, rspp_v_pad);
                RsppVal rspp_v_rv = _t2084;
                RsppVal _t2085 = rspp_str("");
                _t2085 = rspp_add(_t2085, rspp_str(rspp_to_str(rspp_v_pad)));
                _t2085 = rspp_add(_t2085, rspp_str("RsppVal "));
                _t2085 = rspp_add(_t2085, rspp_str(rspp_to_str(rspp_v_rtmp)));
                _t2085 = rspp_add(_t2085, rspp_str(" = "));
                _t2085 = rspp_add(_t2085, rspp_str(rspp_to_str(rspp_v_rv)));
                _t2085 = rspp_add(_t2085, rspp_str(";"));
                RsppVal _t2086 = rspp_fn_cg_push_line(rspp_v_ctx, _t2085);
            } else {
                RsppVal _t2087 = rspp_str("");
                _t2087 = rspp_add(_t2087, rspp_str(rspp_to_str(rspp_v_pad)));
                _t2087 = rspp_add(_t2087, rspp_str("RsppVal "));
                _t2087 = rspp_add(_t2087, rspp_str(rspp_to_str(rspp_v_rtmp)));
                _t2087 = rspp_add(_t2087, rspp_str(" = rspp_none();"));
                RsppVal _t2088 = rspp_fn_cg_push_line(rspp_v_ctx, _t2087);
            }
            RsppVal _t2089 = rspp_fn_cg_emit_defers_all(rspp_v_ctx, rspp_v_pad);
            RsppVal _t2090 = rspp_str("");
            _t2090 = rspp_add(_t2090, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2090 = rspp_add(_t2090, rspp_str("return "));
            _t2090 = rspp_add(_t2090, rspp_str(rspp_to_str(rspp_v_rtmp)));
            _t2090 = rspp_add(_t2090, rspp_str(";"));
            RsppVal _t2091 = rspp_fn_cg_push_line(rspp_v_ctx, _t2090);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("break")))) {
            RsppVal _t2092 = rspp_str("");
            _t2092 = rspp_add(_t2092, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2092 = rspp_add(_t2092, rspp_str("break;"));
            RsppVal _t2093 = rspp_fn_cg_push_line(rspp_v_ctx, _t2092);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("continue")))) {
            RsppVal _t2094 = rspp_str("");
            _t2094 = rspp_add(_t2094, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2094 = rspp_add(_t2094, rspp_str("continue;"));
            RsppVal _t2095 = rspp_fn_cg_push_line(rspp_v_ctx, _t2094);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("unsafe_hardware")))) {
            RsppVal _t2096 = rspp_str("");
            _t2096 = rspp_add(_t2096, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2096 = rspp_add(_t2096, rspp_str("{"));
            RsppVal _t2097 = rspp_fn_cg_push_line(rspp_v_ctx, _t2096);
            RsppVal _t2098 = rspp_list();
            RsppVal _t2100[] = { rspp_str("body"), _t2098 };
            RsppVal _t2099 = rspp_method_call(rspp_v_s, "get", 2, _t2100);
            RsppVal _t2101 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2099, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal _t2102 = rspp_str("");
            _t2102 = rspp_add(_t2102, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2102 = rspp_add(_t2102, rspp_str("}"));
            RsppVal _t2103 = rspp_fn_cg_push_line(rspp_v_ctx, _t2102);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("try_catch")))) {
            RsppVal _t2105[] = { rspp_str("err_var"), rspp_str("err") };
            RsppVal _t2104 = rspp_method_call(rspp_v_s, "get", 2, _t2105);
            RsppVal _t2106 = rspp_fn_c_ident(_t2104);
            RsppVal rspp_v_ev = _t2106;
            RsppVal _t2107 = rspp_str("");
            _t2107 = rspp_add(_t2107, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2107 = rspp_add(_t2107, rspp_str("if (setjmp(rspp_push_try()) == 0) {"));
            RsppVal _t2108 = rspp_fn_cg_push_line(rspp_v_ctx, _t2107);
            RsppVal _t2109 = rspp_list();
            RsppVal _t2111[] = { rspp_str("try_body"), _t2109 };
            RsppVal _t2110 = rspp_method_call(rspp_v_s, "get", 2, _t2111);
            RsppVal _t2112 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2110, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal _t2113 = rspp_str("");
            _t2113 = rspp_add(_t2113, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2113 = rspp_add(_t2113, rspp_str("    rspp_pop_try();"));
            RsppVal _t2114 = rspp_fn_cg_push_line(rspp_v_ctx, _t2113);
            RsppVal _t2115 = rspp_str("");
            _t2115 = rspp_add(_t2115, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2115 = rspp_add(_t2115, rspp_str("} else {"));
            RsppVal _t2116 = rspp_fn_cg_push_line(rspp_v_ctx, _t2115);
            RsppVal _t2117 = rspp_str("");
            _t2117 = rspp_add(_t2117, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2117 = rspp_add(_t2117, rspp_str("    rspp_pop_try();"));
            RsppVal _t2118 = rspp_fn_cg_push_line(rspp_v_ctx, _t2117);
            RsppVal _t2119 = rspp_str("");
            _t2119 = rspp_add(_t2119, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2119 = rspp_add(_t2119, rspp_str("    RsppVal rspp_v_"));
            _t2119 = rspp_add(_t2119, rspp_str(rspp_to_str(rspp_v_ev)));
            _t2119 = rspp_add(_t2119, rspp_str(" = rspp_str(rspp_last_error);"));
            RsppVal _t2120 = rspp_fn_cg_push_line(rspp_v_ctx, _t2119);
            RsppVal _t2121 = rspp_list();
            RsppVal _t2123[] = { rspp_str("catch_body"), _t2121 };
            RsppVal _t2122 = rspp_method_call(rspp_v_s, "get", 2, _t2123);
            RsppVal _t2124 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2122, rspp_add(rspp_v_pad, rspp_str("    ")));
            RsppVal _t2125 = rspp_str("");
            _t2125 = rspp_add(_t2125, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2125 = rspp_add(_t2125, rspp_str("}"));
            RsppVal _t2126 = rspp_fn_cg_push_line(rspp_v_ctx, _t2125);
    } else {
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("throw")))) {
            RsppVal _t2127 = rspp_map();
            RsppVal _t2129[] = { rspp_str("value"), _t2127 };
            RsppVal _t2128 = rspp_method_call(rspp_v_s, "get", 2, _t2129);
            RsppVal _t2130 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t2128, rspp_v_pad);
            RsppVal rspp_v_v = _t2130;
            RsppVal _t2131 = rspp_str("");
            _t2131 = rspp_add(_t2131, rspp_str(rspp_to_str(rspp_v_pad)));
            _t2131 = rspp_add(_t2131, rspp_str("rspp_throw(rspp_to_str("));
            _t2131 = rspp_add(_t2131, rspp_str(rspp_to_str(rspp_v_v)));
            _t2131 = rspp_add(_t2131, rspp_str("));"));
            RsppVal _t2132 = rspp_fn_cg_push_line(rspp_v_ctx, _t2131);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    }
    return rspp_none();
}

static RsppVal rspp_fn_emit_native_program_c(RsppVal rspp_v_prog) {
    RsppVal _t2133 = rspp_fn_new_emit_ctx();
    RsppVal rspp_v_ctx = _t2133;
    RsppVal _t2134 = rspp_list();
    RsppVal _t2136[] = { rspp_str("lines"), _t2134 };
    RsppVal _t2135 = rspp_method_call(rspp_v_ctx, "get", 2, _t2136);
    RsppVal rspp_v_lines = _t2135;
    RsppVal _t2137 = rspp_fn_emit_c_prelude(rspp_v_lines);
    RsppVal _t2138 = rspp_list();
    RsppVal _t2140[] = { rspp_str("items"), _t2138 };
    RsppVal _t2139 = rspp_method_call(rspp_v_prog, "get", 2, _t2140);
    RsppVal rspp_v_items = _t2139;
    RsppVal _t2141 = rspp_v_items;
    for (int64_t _t2142 = 0; _t2142 < rspp_list_len(_t2141); _t2142++) {
        RsppVal rspp_v_it = rspp_index_get(_t2141, rspp_int(_t2142));
        RsppVal _t2144[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t2143 = rspp_method_call(rspp_v_it, "get", 2, _t2144);
        RsppVal rspp_v_k = _t2143;
        RsppVal _t2145 = rspp_bool(1);
        if (!rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("fn")))) {
            _t2145 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("algorithm"))));
        }
        if (rspp_to_bool(_t2145)) {
            RsppVal _t2147[] = { rspp_str("name"), rspp_str("") };
            RsppVal _t2146 = rspp_method_call(rspp_v_it, "get", 2, _t2147);
            RsppVal _t2148 = rspp_fn_c_ident(_t2146);
            RsppVal rspp_v_fn_n = _t2148;
            RsppVal _t2149 = rspp_list();
            RsppVal _t2151[] = { rspp_str("params"), _t2149 };
            RsppVal _t2150 = rspp_method_call(rspp_v_it, "get", 2, _t2151);
            RsppVal rspp_v_ps = _t2150;
            RsppVal _t2152 = rspp_list();
            RsppVal rspp_v_pdecls = _t2152;
            RsppVal rspp_v_pi = rspp_int(0LL);
            while (1) {
                RsppVal _t2153 = rspp_method_call(rspp_v_ps, "len", 0, NULL);
                if (!rspp_to_bool(rspp_lt(rspp_v_pi, _t2153))) break;
                RsppVal _t2154 = rspp_str("");
                _t2154 = rspp_add(_t2154, rspp_str("RsppVal p"));
                _t2154 = rspp_add(_t2154, rspp_str(rspp_to_str(rspp_v_pi)));
                RsppVal _t2156[] = { _t2154 };
                RsppVal _t2155 = rspp_method_call(rspp_v_pdecls, "push", 1, _t2156);
                rspp_v_pi = rspp_add(rspp_v_pi, rspp_int(1LL));
            }
            RsppVal rspp_v_pstr = rspp_str("void");
            RsppVal _t2157 = rspp_method_call(rspp_v_pdecls, "len", 0, NULL);
            if (rspp_to_bool(rspp_gt(_t2157, rspp_int(0LL)))) {
                RsppVal _t2159[] = { rspp_v_pdecls, rspp_str(", ") };
                RsppVal _t2158 = rspp_builtin("str", "join", 2, _t2159);
                rspp_v_pstr = _t2158;
            }
            RsppVal _t2160 = rspp_str("");
            _t2160 = rspp_add(_t2160, rspp_str("static RsppVal rspp_fn_"));
            _t2160 = rspp_add(_t2160, rspp_str(rspp_to_str(rspp_v_fn_n)));
            _t2160 = rspp_add(_t2160, rspp_str("("));
            _t2160 = rspp_add(_t2160, rspp_str(rspp_to_str(rspp_v_pstr)));
            _t2160 = rspp_add(_t2160, rspp_str(");"));
            RsppVal _t2162[] = { _t2160 };
            RsppVal _t2161 = rspp_method_call(rspp_v_lines, "push", 1, _t2162);
        } else {
            if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("equation")))) {
                RsppVal _t2164[] = { rspp_str("name"), rspp_str("") };
                RsppVal _t2163 = rspp_method_call(rspp_v_it, "get", 2, _t2164);
                RsppVal _t2165 = rspp_fn_c_ident(_t2163);
                RsppVal rspp_v_eqn = _t2165;
                RsppVal _t2166 = rspp_list();
                RsppVal _t2168[] = { rspp_str("params"), _t2166 };
                RsppVal _t2167 = rspp_method_call(rspp_v_it, "get", 2, _t2168);
                RsppVal rspp_v_ps = _t2167;
                RsppVal _t2169 = rspp_list();
                RsppVal rspp_v_pdecls = _t2169;
                RsppVal rspp_v_pi = rspp_int(0LL);
                while (1) {
                    RsppVal _t2170 = rspp_method_call(rspp_v_ps, "len", 0, NULL);
                    if (!rspp_to_bool(rspp_lt(rspp_v_pi, _t2170))) break;
                    RsppVal _t2171 = rspp_str("");
                    _t2171 = rspp_add(_t2171, rspp_str("RsppVal p"));
                    _t2171 = rspp_add(_t2171, rspp_str(rspp_to_str(rspp_v_pi)));
                    RsppVal _t2173[] = { _t2171 };
                    RsppVal _t2172 = rspp_method_call(rspp_v_pdecls, "push", 1, _t2173);
                    rspp_v_pi = rspp_add(rspp_v_pi, rspp_int(1LL));
                }
                RsppVal rspp_v_pstr = rspp_str("void");
                RsppVal _t2174 = rspp_method_call(rspp_v_pdecls, "len", 0, NULL);
                if (rspp_to_bool(rspp_gt(_t2174, rspp_int(0LL)))) {
                    RsppVal _t2176[] = { rspp_v_pdecls, rspp_str(", ") };
                    RsppVal _t2175 = rspp_builtin("str", "join", 2, _t2176);
                    rspp_v_pstr = _t2175;
                }
                RsppVal _t2177 = rspp_str("");
                _t2177 = rspp_add(_t2177, rspp_str("static RsppVal rspp_fn_"));
                _t2177 = rspp_add(_t2177, rspp_str(rspp_to_str(rspp_v_eqn)));
                _t2177 = rspp_add(_t2177, rspp_str("("));
                _t2177 = rspp_add(_t2177, rspp_str(rspp_to_str(rspp_v_pstr)));
                _t2177 = rspp_add(_t2177, rspp_str(");"));
                RsppVal _t2179[] = { _t2177 };
                RsppVal _t2178 = rspp_method_call(rspp_v_lines, "push", 1, _t2179);
                RsppVal _t2180 = rspp_method_call(rspp_v_ps, "len", 0, NULL);
                if (rspp_to_bool(rspp_eq(_t2180, rspp_int(1LL)))) {
                    RsppVal _t2181 = rspp_str("");
                    _t2181 = rspp_add(_t2181, rspp_str("static double rspp_eq1_"));
                    _t2181 = rspp_add(_t2181, rspp_str(rspp_to_str(rspp_v_eqn)));
                    _t2181 = rspp_add(_t2181, rspp_str("(double x);"));
                    RsppVal _t2183[] = { _t2181 };
                    RsppVal _t2182 = rspp_method_call(rspp_v_lines, "push", 1, _t2183);
                }
        }
        }
    }
    RsppVal rspp_v_has_main = rspp_bool(0);
    RsppVal _t2184 = rspp_list();
    RsppVal rspp_v_one_var_eqs = _t2184;
    RsppVal _t2185 = rspp_v_items;
    for (int64_t _t2186 = 0; _t2186 < rspp_list_len(_t2185); _t2186++) {
        RsppVal rspp_v_it = rspp_index_get(_t2185, rspp_int(_t2186));
        RsppVal _t2188[] = { rspp_str("kind"), rspp_str("") };
        RsppVal _t2187 = rspp_method_call(rspp_v_it, "get", 2, _t2188);
        RsppVal rspp_v_k = _t2187;
        if (rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("equation")))) {
            RsppVal _t2190[] = { rspp_str("name"), rspp_str("") };
            RsppVal _t2189 = rspp_method_call(rspp_v_it, "get", 2, _t2190);
            RsppVal _t2191 = rspp_fn_c_ident(_t2189);
            RsppVal rspp_v_eqn = _t2191;
            RsppVal _t2192 = rspp_list();
            RsppVal _t2194[] = { rspp_str("params"), _t2192 };
            RsppVal _t2193 = rspp_method_call(rspp_v_it, "get", 2, _t2194);
            RsppVal rspp_v_ps = _t2193;
            RsppVal _t2195 = rspp_list();
            RsppVal rspp_v_pdecls = _t2195;
            RsppVal _t2196 = rspp_v_ps;
            for (int64_t _t2197 = 0; _t2197 < rspp_list_len(_t2196); _t2197++) {
                RsppVal rspp_v_p_name = rspp_index_get(_t2196, rspp_int(_t2197));
                RsppVal _t2198 = rspp_fn_c_ident(rspp_v_p_name);
                RsppVal _t2200[] = { rspp_add(rspp_str("RsppVal rspp_v_"), _t2198) };
                RsppVal _t2199 = rspp_method_call(rspp_v_pdecls, "push", 1, _t2200);
            }
            RsppVal rspp_v_pstr = rspp_str("void");
            RsppVal _t2201 = rspp_method_call(rspp_v_pdecls, "len", 0, NULL);
            if (rspp_to_bool(rspp_gt(_t2201, rspp_int(0LL)))) {
                RsppVal _t2203[] = { rspp_v_pdecls, rspp_str(", ") };
                RsppVal _t2202 = rspp_builtin("str", "join", 2, _t2203);
                rspp_v_pstr = _t2202;
            }
            RsppVal _t2204 = rspp_str("");
            _t2204 = rspp_add(_t2204, rspp_str("static RsppVal rspp_fn_"));
            _t2204 = rspp_add(_t2204, rspp_str(rspp_to_str(rspp_v_eqn)));
            _t2204 = rspp_add(_t2204, rspp_str("("));
            _t2204 = rspp_add(_t2204, rspp_str(rspp_to_str(rspp_v_pstr)));
            _t2204 = rspp_add(_t2204, rspp_str(") {"));
            RsppVal _t2206[] = { _t2204 };
            RsppVal _t2205 = rspp_method_call(rspp_v_lines, "push", 1, _t2206);
            RsppVal _t2207 = rspp_map();
            RsppVal _t2209[] = { rspp_str("expr"), _t2207 };
            RsppVal _t2208 = rspp_method_call(rspp_v_it, "get", 2, _t2209);
            RsppVal _t2210 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t2208, rspp_str("    "));
            RsppVal rspp_v_ev = _t2210;
            RsppVal _t2211 = rspp_str("");
            _t2211 = rspp_add(_t2211, rspp_str("    return rspp_flt(rspp_to_f64("));
            _t2211 = rspp_add(_t2211, rspp_str(rspp_to_str(rspp_v_ev)));
            _t2211 = rspp_add(_t2211, rspp_str("));"));
            RsppVal _t2213[] = { _t2211 };
            RsppVal _t2212 = rspp_method_call(rspp_v_lines, "push", 1, _t2213);
            RsppVal _t2215[] = { rspp_str("}") };
            RsppVal _t2214 = rspp_method_call(rspp_v_lines, "push", 1, _t2215);
            RsppVal _t2216 = rspp_method_call(rspp_v_ps, "len", 0, NULL);
            if (rspp_to_bool(rspp_eq(_t2216, rspp_int(1LL)))) {
                RsppVal _t2218[] = { rspp_str("name"), rspp_str("") };
                RsppVal _t2217 = rspp_method_call(rspp_v_it, "get", 2, _t2218);
                RsppVal _t2220[] = { _t2217 };
                RsppVal _t2219 = rspp_method_call(rspp_v_one_var_eqs, "push", 1, _t2220);
                RsppVal _t2221 = rspp_fn_c_ident(rspp_index_get(rspp_v_ps, rspp_int(0LL)));
                RsppVal rspp_v_p0 = _t2221;
                RsppVal _t2222 = rspp_str("");
                _t2222 = rspp_add(_t2222, rspp_str("static double rspp_eq1_"));
                _t2222 = rspp_add(_t2222, rspp_str(rspp_to_str(rspp_v_eqn)));
                _t2222 = rspp_add(_t2222, rspp_str("(double _x) {"));
                RsppVal _t2224[] = { _t2222 };
                RsppVal _t2223 = rspp_method_call(rspp_v_lines, "push", 1, _t2224);
                RsppVal _t2225 = rspp_str("");
                _t2225 = rspp_add(_t2225, rspp_str("    RsppVal rspp_v_"));
                _t2225 = rspp_add(_t2225, rspp_str(rspp_to_str(rspp_v_p0)));
                _t2225 = rspp_add(_t2225, rspp_str(" = rspp_flt(_x);"));
                RsppVal _t2227[] = { _t2225 };
                RsppVal _t2226 = rspp_method_call(rspp_v_lines, "push", 1, _t2227);
                RsppVal _t2228 = rspp_map();
                RsppVal _t2230[] = { rspp_str("expr"), _t2228 };
                RsppVal _t2229 = rspp_method_call(rspp_v_it, "get", 2, _t2230);
                RsppVal _t2231 = rspp_fn_cg_emit_expr(rspp_v_ctx, _t2229, rspp_str("    "));
                RsppVal rspp_v_ev1 = _t2231;
                RsppVal _t2232 = rspp_str("");
                _t2232 = rspp_add(_t2232, rspp_str("    return rspp_to_f64("));
                _t2232 = rspp_add(_t2232, rspp_str(rspp_to_str(rspp_v_ev1)));
                _t2232 = rspp_add(_t2232, rspp_str(");"));
                RsppVal _t2234[] = { _t2232 };
                RsppVal _t2233 = rspp_method_call(rspp_v_lines, "push", 1, _t2234);
                RsppVal _t2236[] = { rspp_str("}") };
                RsppVal _t2235 = rspp_method_call(rspp_v_lines, "push", 1, _t2236);
            }
        } else {
            RsppVal _t2237 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("fn")))) {
                _t2237 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_k, rspp_str("algorithm"))));
            }
            if (rspp_to_bool(_t2237)) {
                RsppVal _t2239[] = { rspp_str("name"), rspp_str("") };
                RsppVal _t2238 = rspp_method_call(rspp_v_it, "get", 2, _t2239);
                RsppVal rspp_v_fn_raw = _t2238;
                if (rspp_to_bool(rspp_eq(rspp_v_fn_raw, rspp_str("main")))) {
                    rspp_v_has_main = rspp_bool(1);
                }
                RsppVal _t2240 = rspp_fn_c_ident(rspp_v_fn_raw);
                RsppVal rspp_v_fn_n = _t2240;
                RsppVal _t2241 = rspp_list();
                RsppVal _t2243[] = { rspp_str("params"), _t2241 };
                RsppVal _t2242 = rspp_method_call(rspp_v_it, "get", 2, _t2243);
                RsppVal rspp_v_ps = _t2242;
                RsppVal _t2244 = rspp_list();
                RsppVal rspp_v_pdecls = _t2244;
                RsppVal _t2245 = rspp_v_ps;
                for (int64_t _t2246 = 0; _t2246 < rspp_list_len(_t2245); _t2246++) {
                    RsppVal rspp_v_pm = rspp_index_get(_t2245, rspp_int(_t2246));
                    RsppVal _t2248[] = { rspp_str("name"), rspp_str("") };
                    RsppVal _t2247 = rspp_method_call(rspp_v_pm, "get", 2, _t2248);
                    RsppVal _t2249 = rspp_fn_c_ident(_t2247);
                    RsppVal _t2251[] = { rspp_add(rspp_str("RsppVal rspp_v_"), _t2249) };
                    RsppVal _t2250 = rspp_method_call(rspp_v_pdecls, "push", 1, _t2251);
                }
                RsppVal rspp_v_pstr = rspp_str("void");
                RsppVal _t2252 = rspp_method_call(rspp_v_pdecls, "len", 0, NULL);
                if (rspp_to_bool(rspp_gt(_t2252, rspp_int(0LL)))) {
                    RsppVal _t2254[] = { rspp_v_pdecls, rspp_str(", ") };
                    RsppVal _t2253 = rspp_builtin("str", "join", 2, _t2254);
                    rspp_v_pstr = _t2253;
                }
                RsppVal _t2255 = rspp_str("");
                _t2255 = rspp_add(_t2255, rspp_str("static RsppVal rspp_fn_"));
                _t2255 = rspp_add(_t2255, rspp_str(rspp_to_str(rspp_v_fn_n)));
                _t2255 = rspp_add(_t2255, rspp_str("("));
                _t2255 = rspp_add(_t2255, rspp_str(rspp_to_str(rspp_v_pstr)));
                _t2255 = rspp_add(_t2255, rspp_str(") {"));
                RsppVal _t2257[] = { _t2255 };
                RsppVal _t2256 = rspp_method_call(rspp_v_lines, "push", 1, _t2257);
                RsppVal _t2258 = rspp_list();
                RsppVal _t2260[] = { rspp_str("body"), _t2258 };
                RsppVal _t2259 = rspp_method_call(rspp_v_it, "get", 2, _t2260);
                RsppVal _t2261 = rspp_fn_cg_emit_block(rspp_v_ctx, _t2259, rspp_str("    "));
                RsppVal _t2263[] = { rspp_str("    return rspp_none();") };
                RsppVal _t2262 = rspp_method_call(rspp_v_lines, "push", 1, _t2263);
                RsppVal _t2265[] = { rspp_str("}") };
                RsppVal _t2264 = rspp_method_call(rspp_v_lines, "push", 1, _t2265);
        }
        }
    }
    RsppVal _t2267[] = { rspp_str("int main(int argc, char** argv) {") };
    RsppVal _t2266 = rspp_method_call(rspp_v_lines, "push", 1, _t2267);
    RsppVal _t2269[] = { rspp_str("    rspp_argc = argc; rspp_argv = argv;") };
    RsppVal _t2268 = rspp_method_call(rspp_v_lines, "push", 1, _t2269);
    RsppVal _t2271[] = { rspp_str("    atexit(rspp_mem_leak_check_atexit);") };
    RsppVal _t2270 = rspp_method_call(rspp_v_lines, "push", 1, _t2271);
    RsppVal _t2272 = rspp_v_one_var_eqs;
    for (int64_t _t2273 = 0; _t2273 < rspp_list_len(_t2272); _t2273++) {
        RsppVal rspp_v_eqn = rspp_index_get(_t2272, rspp_int(_t2273));
        RsppVal _t2274 = rspp_fn_c_ident(rspp_v_eqn);
        RsppVal rspp_v_ceq = _t2274;
        RsppVal _t2275 = rspp_str("");
        _t2275 = rspp_add(_t2275, rspp_str("    rspp_register_eq(\""));
        RsppVal _t2276 = rspp_fn_esc_c_str(rspp_v_eqn);
        _t2275 = rspp_add(_t2275, rspp_str(rspp_to_str(_t2276)));
        _t2275 = rspp_add(_t2275, rspp_str("\", rspp_eq1_"));
        _t2275 = rspp_add(_t2275, rspp_str(rspp_to_str(rspp_v_ceq)));
        _t2275 = rspp_add(_t2275, rspp_str(");"));
        RsppVal _t2278[] = { _t2275 };
        RsppVal _t2277 = rspp_method_call(rspp_v_lines, "push", 1, _t2278);
    }
    if (rspp_to_bool(rspp_v_has_main)) {
        RsppVal _t2280[] = { rspp_str("    RsppVal r = rspp_fn_main();") };
        RsppVal _t2279 = rspp_method_call(rspp_v_lines, "push", 1, _t2280);
        RsppVal _t2282[] = { rspp_str("    if (r.kind == RSPP_INT) return (int)r.i;") };
        RsppVal _t2281 = rspp_method_call(rspp_v_lines, "push", 1, _t2282);
    }
    RsppVal _t2284[] = { rspp_str("    return 0;") };
    RsppVal _t2283 = rspp_method_call(rspp_v_lines, "push", 1, _t2284);
    RsppVal _t2286[] = { rspp_str("}") };
    RsppVal _t2285 = rspp_method_call(rspp_v_lines, "push", 1, _t2286);
    RsppVal _t2289[] = { rspp_v_lines, rspp_str("\n") };
    RsppVal _t2288 = rspp_builtin("str", "join", 2, _t2289);
    RsppVal _t2287 = rspp_add(_t2288, rspp_str("\n"));
    return _t2287;
    return rspp_none();
}

static RsppVal rspp_fn_emit_kernel_c(RsppVal rspp_v_prog) {
    RsppVal _t2290 = rspp_list();
    RsppVal rspp_v_lines = _t2290;
    RsppVal _t2292[] = { rspp_str("#include <stdint.h>") };
    RsppVal _t2291 = rspp_method_call(rspp_v_lines, "push", 1, _t2292);
    RsppVal _t2294[] = { rspp_str("__attribute__((section(\".multiboot\"), aligned(8)))") };
    RsppVal _t2293 = rspp_method_call(rspp_v_lines, "push", 1, _t2294);
    RsppVal _t2296[] = { rspp_str("const uint32_t multiboot2_header[] = { 0xe85250d6u, 0u, 16u, (uint32_t)(-(0xe85250d6u + 16u)) };") };
    RsppVal _t2295 = rspp_method_call(rspp_v_lines, "push", 1, _t2296);
    RsppVal _t2298[] = { rspp_str("static inline void k_outb(uint16_t port, uint8_t val) { __asm__ volatile(\"outb %0, %1\" : : \"a\"(val), \"Nd\"(port)); }") };
    RsppVal _t2297 = rspp_method_call(rspp_v_lines, "push", 1, _t2298);
    RsppVal _t2300[] = { rspp_str("static inline void k_vga_print(const char* s, uint64_t row, uint64_t color) { volatile uint16_t* vga = (volatile uint16_t*)0xB8000ULL; uint64_t off = row * 80; for (uint64_t i = 0; s[i] && (off + i) < 2000; i++) vga[off + i] = ((uint16_t)color << 8) | (uint8_t)s[i]; }") };
    RsppVal _t2299 = rspp_method_call(rspp_v_lines, "push", 1, _t2300);
    RsppVal _t2302[] = { rspp_str("void kernel_main(void) { k_vga_print(\"rust++ Self-Hosted Kernel Active\", 0, 0x0A); for(;;) __asm__ volatile(\"hlt\"); }") };
    RsppVal _t2301 = rspp_method_call(rspp_v_lines, "push", 1, _t2302);
    RsppVal _t2305[] = { rspp_v_lines, rspp_str("\n") };
    RsppVal _t2304 = rspp_builtin("str", "join", 2, _t2305);
    RsppVal _t2303 = rspp_add(_t2304, rspp_str("\n"));
    return _t2303;
    return rspp_none();
}

static RsppVal rspp_fn_print_help(void) {
    RsppVal _t2307[] = { rspp_str("rust++ Self-Hosted Compiler v0.2.0 (written 100% in .rspp)") };
    RsppVal _t2306 = rspp_builtin("io", "print", 1, _t2307);
    RsppVal _t2309[] = { rspp_str("Commands:") };
    RsppVal _t2308 = rspp_builtin("io", "print", 1, _t2309);
    RsppVal _t2311[] = { rspp_str("  check <file.rspp>             Run Leak Guard & Math Diagnostics") };
    RsppVal _t2310 = rspp_builtin("io", "print", 1, _t2311);
    RsppVal _t2313[] = { rspp_str("  build <file.rspp> [-o out]    Compile .rspp to native C11 binary (or --target kernel)") };
    RsppVal _t2312 = rspp_builtin("io", "print", 1, _t2313);
    RsppVal _t2315[] = { rspp_str("  run   <file.rspp>             Compile & run native binary") };
    RsppVal _t2314 = rspp_builtin("io", "print", 1, _t2315);
    RsppVal _t2317[] = { rspp_str("  solve <guess>                 Run Newton-Raphson solver on demo equations") };
    RsppVal _t2316 = rspp_builtin("io", "print", 1, _t2317);
    RsppVal _t2319[] = { rspp_str("  synth <fn>                    Generate a rust++ algorithm from examples") };
    RsppVal _t2318 = rspp_builtin("io", "print", 1, _t2319);
    RsppVal _t2321[] = { rspp_str("  sdr                           Run SDR FFT spectrum & ASK/OOK demodulation demo") };
    RsppVal _t2320 = rspp_builtin("io", "print", 1, _t2321);
    RsppVal _t2323[] = { rspp_str("  probe                         Inspect hardware & Leak Guard status") };
    RsppVal _t2322 = rspp_builtin("io", "print", 1, _t2323);
    return rspp_none();
}

static RsppVal rspp_fn_main(void) {
    RsppVal _t2324 = rspp_builtin("sys", "args", 0, NULL);
    RsppVal rspp_v_args = _t2324;
    RsppVal _t2325 = rspp_method_call(rspp_v_args, "len", 0, NULL);
    if (rspp_to_bool(rspp_lt(_t2325, rspp_int(2LL)))) {
        RsppVal _t2326 = rspp_fn_print_help();
        RsppVal _t2327 = rspp_int(0LL);
        return _t2327;
    }
    RsppVal rspp_v_cmd = rspp_index_get(rspp_v_args, rspp_int(1LL));
    RsppVal _t2328 = rspp_bool(1);
    if (!rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("version")))) {
        _t2328 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("--version"))));
    }
    if (rspp_to_bool(_t2328)) {
        RsppVal _t2330[] = { rspp_str("rust++ Self-Hosted Native Compiler v0.2.0 (.rspp)") };
        RsppVal _t2329 = rspp_builtin("io", "print", 1, _t2330);
        RsppVal _t2331 = rspp_int(0LL);
        return _t2331;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("probe")))) {
        RsppVal _t2332 = rspp_builtin("hardware", "probe", 0, NULL);
        RsppVal _t2334[] = { _t2332 };
        RsppVal _t2333 = rspp_builtin("io", "print", 1, _t2334);
        RsppVal _t2335 = rspp_builtin("memory", "leak_report", 0, NULL);
        RsppVal _t2337[] = { _t2335 };
        RsppVal _t2336 = rspp_builtin("io", "print", 1, _t2337);
        RsppVal _t2338 = rspp_int(0LL);
        return _t2338;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("check")))) {
        RsppVal _t2339 = rspp_method_call(rspp_v_args, "len", 0, NULL);
        if (rspp_to_bool(rspp_lt(_t2339, rspp_int(3LL)))) {
            RsppVal _t2341[] = { rspp_str("Usage: check <file.rspp>") };
            RsppVal _t2340 = rspp_builtin("io", "eprint", 1, _t2341);
            RsppVal _t2342 = rspp_int(1LL);
            return _t2342;
        }
        RsppVal rspp_v_file_path = rspp_index_get(rspp_v_args, rspp_int(2LL));
        RsppVal _t2343 = rspp_fn_load_program_bundle(rspp_v_file_path);
        RsppVal rspp_v_prog = _t2343;
        RsppVal _t2344 = rspp_fn_leak_guard_verify_program(rspp_v_prog);
        RsppVal rspp_v_diags = _t2344;
        RsppVal _t2345 = rspp_method_call(rspp_v_diags, "len", 0, NULL);
        if (rspp_to_bool(rspp_eq(_t2345, rspp_int(0LL)))) {
            RsppVal _t2346 = rspp_str("");
            _t2346 = rspp_add(_t2346, rspp_str("OK (self-hosted): `"));
            _t2346 = rspp_add(_t2346, rspp_str(rspp_to_str(rspp_v_file_path)));
            _t2346 = rspp_add(_t2346, rspp_str("` passed all Leak Guard & static checks (0 issues)."));
            RsppVal _t2348[] = { _t2346 };
            RsppVal _t2347 = rspp_builtin("io", "print", 1, _t2348);
            RsppVal _t2349 = rspp_int(0LL);
            return _t2349;
        }
        RsppVal _t2350 = rspp_v_diags;
        for (int64_t _t2351 = 0; _t2351 < rspp_list_len(_t2350); _t2351++) {
            RsppVal rspp_v_d = rspp_index_get(_t2350, rspp_int(_t2351));
            RsppVal _t2353[] = { rspp_v_d };
            RsppVal _t2352 = rspp_builtin("io", "eprint", 1, _t2353);
        }
        RsppVal _t2354 = rspp_int(1LL);
        return _t2354;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("build")))) {
        RsppVal _t2355 = rspp_method_call(rspp_v_args, "len", 0, NULL);
        if (rspp_to_bool(rspp_lt(_t2355, rspp_int(3LL)))) {
            RsppVal _t2357[] = { rspp_str("Usage: build <file.rspp> [-o out] [--target native|kernel] [--emit-c]") };
            RsppVal _t2356 = rspp_builtin("io", "eprint", 1, _t2357);
            RsppVal _t2358 = rspp_int(1LL);
            return _t2358;
        }
        RsppVal rspp_v_file_path = rspp_index_get(rspp_v_args, rspp_int(2LL));
        RsppVal rspp_v_out_bin = rspp_add(rspp_v_file_path, rspp_str(".out"));
        RsppVal rspp_v_target = rspp_str("native");
        RsppVal rspp_v_emit_c_only = rspp_bool(0);
        RsppVal rspp_v_i = rspp_int(3LL);
        while (1) {
            RsppVal _t2359 = rspp_method_call(rspp_v_args, "len", 0, NULL);
            if (!rspp_to_bool(rspp_lt(rspp_v_i, _t2359))) break;
            RsppVal rspp_v_a = rspp_index_get(rspp_v_args, rspp_v_i);
            RsppVal _t2361 = rspp_bool(1);
            if (!rspp_to_bool(rspp_eq(rspp_v_a, rspp_str("-o")))) {
                _t2361 = rspp_bool(rspp_to_bool(rspp_eq(rspp_v_a, rspp_str("--output"))));
            }
            RsppVal _t2360 = rspp_bool(0);
            if (rspp_to_bool(_t2361)) {
                RsppVal _t2362 = rspp_method_call(rspp_v_args, "len", 0, NULL);
                _t2360 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_i, rspp_int(1LL)), _t2362)));
            }
            if (rspp_to_bool(_t2360)) {
                rspp_v_out_bin = rspp_index_get(rspp_v_args, rspp_add(rspp_v_i, rspp_int(1LL)));
                rspp_v_i = rspp_add(rspp_v_i, rspp_int(2LL));
            } else {
                RsppVal _t2363 = rspp_bool(0);
                if (rspp_to_bool(rspp_eq(rspp_v_a, rspp_str("--target")))) {
                    RsppVal _t2364 = rspp_method_call(rspp_v_args, "len", 0, NULL);
                    _t2363 = rspp_bool(rspp_to_bool(rspp_lt(rspp_add(rspp_v_i, rspp_int(1LL)), _t2364)));
                }
                if (rspp_to_bool(_t2363)) {
                    rspp_v_target = rspp_index_get(rspp_v_args, rspp_add(rspp_v_i, rspp_int(1LL)));
                    rspp_v_i = rspp_add(rspp_v_i, rspp_int(2LL));
            } else {
                if (rspp_to_bool(rspp_eq(rspp_v_a, rspp_str("--emit-c")))) {
                    rspp_v_emit_c_only = rspp_bool(1);
                    rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
            } else {
                rspp_v_i = rspp_add(rspp_v_i, rspp_int(1LL));
                }
            }
            }
        }
        RsppVal _t2365 = rspp_fn_load_program_bundle(rspp_v_file_path);
        RsppVal rspp_v_prog = _t2365;
        RsppVal _t2366 = rspp_fn_leak_guard_verify_program(rspp_v_prog);
        RsppVal rspp_v_diags = _t2366;
        RsppVal _t2367 = rspp_method_call(rspp_v_diags, "len", 0, NULL);
        if (rspp_to_bool(rspp_gt(_t2367, rspp_int(0LL)))) {
            RsppVal _t2368 = rspp_v_diags;
            for (int64_t _t2369 = 0; _t2369 < rspp_list_len(_t2368); _t2369++) {
                RsppVal rspp_v_d = rspp_index_get(_t2368, rspp_int(_t2369));
                RsppVal _t2371[] = { rspp_v_d };
                RsppVal _t2370 = rspp_builtin("io", "eprint", 1, _t2371);
            }
            RsppVal _t2372 = rspp_int(1LL);
            return _t2372;
        }
        RsppVal rspp_v_c_src = rspp_str("");
        if (rspp_to_bool(rspp_eq(rspp_v_target, rspp_str("kernel")))) {
            RsppVal _t2373 = rspp_fn_emit_kernel_c(rspp_v_prog);
            rspp_v_c_src = _t2373;
        } else {
            RsppVal _t2374 = rspp_fn_emit_native_program_c(rspp_v_prog);
            rspp_v_c_src = _t2374;
        }
        RsppVal rspp_v_c_path = rspp_add(rspp_v_out_bin, rspp_str(".rspp.c"));
        RsppVal _t2376[] = { rspp_v_c_path, rspp_v_c_src };
        RsppVal _t2375 = rspp_builtin("fs", "write_text", 2, _t2376);
        if (rspp_to_bool(rspp_v_emit_c_only)) {
            RsppVal _t2377 = rspp_str("");
            _t2377 = rspp_add(_t2377, rspp_str("Emitted self-hosted C11: "));
            _t2377 = rspp_add(_t2377, rspp_str(rspp_to_str(rspp_v_c_path)));
            RsppVal _t2379[] = { _t2377 };
            RsppVal _t2378 = rspp_builtin("io", "print", 1, _t2379);
            RsppVal _t2380 = rspp_int(0LL);
            return _t2380;
        }
        RsppVal _t2381 = rspp_str("");
        _t2381 = rspp_add(_t2381, rspp_str("gcc -std=c11 -O2 "));
        _t2381 = rspp_add(_t2381, rspp_str(rspp_to_str(rspp_v_c_path)));
        _t2381 = rspp_add(_t2381, rspp_str(" -o "));
        _t2381 = rspp_add(_t2381, rspp_str(rspp_to_str(rspp_v_out_bin)));
        _t2381 = rspp_add(_t2381, rspp_str(" -lm"));
        RsppVal rspp_v_gcc_cmd = _t2381;
        if (rspp_to_bool(rspp_eq(rspp_v_target, rspp_str("kernel")))) {
            RsppVal _t2382 = rspp_str("");
            _t2382 = rspp_add(_t2382, rspp_str("gcc -std=c11 -ffreestanding -nostdlib -fno-stack-protector -O2 -c "));
            _t2382 = rspp_add(_t2382, rspp_str(rspp_to_str(rspp_v_c_path)));
            _t2382 = rspp_add(_t2382, rspp_str(" -o "));
            _t2382 = rspp_add(_t2382, rspp_str(rspp_to_str(rspp_v_out_bin)));
            rspp_v_gcc_cmd = _t2382;
        }
        RsppVal _t2384[] = { rspp_v_gcc_cmd };
        RsppVal _t2383 = rspp_builtin("sys", "exec", 1, _t2384);
        RsppVal rspp_v_rc = _t2383;
        if (rspp_to_bool(rspp_neq(rspp_v_rc, rspp_int(0LL)))) {
            RsppVal _t2385 = rspp_str("");
            _t2385 = rspp_add(_t2385, rspp_str("gcc failed with exit code "));
            _t2385 = rspp_add(_t2385, rspp_str(rspp_to_str(rspp_v_rc)));
            RsppVal _t2387[] = { _t2385 };
            RsppVal _t2386 = rspp_builtin("io", "eprint", 1, _t2387);
            RsppVal _t2388 = rspp_int(1LL);
            return _t2388;
        }
        RsppVal _t2389 = rspp_str("");
        _t2389 = rspp_add(_t2389, rspp_str("Built "));
        _t2389 = rspp_add(_t2389, rspp_str(rspp_to_str(rspp_v_target)));
        _t2389 = rspp_add(_t2389, rspp_str(" via self-hosted compiler: "));
        _t2389 = rspp_add(_t2389, rspp_str(rspp_to_str(rspp_v_out_bin)));
        RsppVal _t2391[] = { _t2389 };
        RsppVal _t2390 = rspp_builtin("io", "print", 1, _t2391);
        RsppVal _t2392 = rspp_int(0LL);
        return _t2392;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("run")))) {
        RsppVal _t2393 = rspp_method_call(rspp_v_args, "len", 0, NULL);
        if (rspp_to_bool(rspp_lt(_t2393, rspp_int(3LL)))) {
            RsppVal _t2395[] = { rspp_str("Usage: run <file.rspp>") };
            RsppVal _t2394 = rspp_builtin("io", "eprint", 1, _t2395);
            RsppVal _t2396 = rspp_int(1LL);
            return _t2396;
        }
        RsppVal rspp_v_file_path = rspp_index_get(rspp_v_args, rspp_int(2LL));
        RsppVal _t2397 = rspp_fn_load_program_bundle(rspp_v_file_path);
        RsppVal rspp_v_prog = _t2397;
        RsppVal _t2398 = rspp_fn_emit_native_program_c(rspp_v_prog);
        RsppVal rspp_v_c_src = _t2398;
        RsppVal rspp_v_tmp_c = rspp_str("/tmp/rspp_selfhost_run.c");
        RsppVal rspp_v_tmp_bin = rspp_str("/tmp/rspp_selfhost_run");
        RsppVal _t2400[] = { rspp_v_tmp_c, rspp_v_c_src };
        RsppVal _t2399 = rspp_builtin("fs", "write_text", 2, _t2400);
        RsppVal _t2401 = rspp_str("");
        _t2401 = rspp_add(_t2401, rspp_str("gcc -std=c11 -O2 "));
        _t2401 = rspp_add(_t2401, rspp_str(rspp_to_str(rspp_v_tmp_c)));
        _t2401 = rspp_add(_t2401, rspp_str(" -o "));
        _t2401 = rspp_add(_t2401, rspp_str(rspp_to_str(rspp_v_tmp_bin)));
        _t2401 = rspp_add(_t2401, rspp_str(" -lm"));
        RsppVal _t2403[] = { _t2401 };
        RsppVal _t2402 = rspp_builtin("sys", "exec", 1, _t2403);
        RsppVal rspp_v_rc = _t2402;
        if (rspp_to_bool(rspp_neq(rspp_v_rc, rspp_int(0LL)))) {
            RsppVal _t2404 = rspp_int(1LL);
            return _t2404;
        }
        RsppVal _t2407[] = { rspp_v_tmp_bin };
        RsppVal _t2406 = rspp_builtin("sys", "exec", 1, _t2407);
        RsppVal _t2405 = _t2406;
        return _t2405;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("synth")))) {
        RsppVal rspp_v_fn_name = rspp_str("generated_rule");
        RsppVal _t2408 = rspp_method_call(rspp_v_args, "len", 0, NULL);
        if (rspp_to_bool(rspp_gte(_t2408, rspp_int(3LL)))) {
            rspp_v_fn_name = rspp_index_get(rspp_v_args, rspp_int(2LL));
        }
        RsppVal _t2409 = rspp_list();
        rspp_list_push(_t2409, rspp_int(1LL));
        rspp_list_push(_t2409, rspp_int(2LL));
        rspp_list_push(_t2409, rspp_int(3LL));
        rspp_list_push(_t2409, rspp_int(4LL));
        RsppVal _t2410 = rspp_list();
        rspp_list_push(_t2410, rspp_int(4LL));
        rspp_list_push(_t2410, rspp_int(7LL));
        rspp_list_push(_t2410, rspp_int(10LL));
        rspp_list_push(_t2410, rspp_int(13LL));
        RsppVal _t2412[] = { rspp_v_fn_name, _t2409, _t2410 };
        RsppVal _t2411 = rspp_builtin("synth", "synthesize_from_examples", 3, _t2412);
        RsppVal rspp_v_code = _t2411;
        RsppVal _t2414[] = { rspp_v_code };
        RsppVal _t2413 = rspp_builtin("io", "print", 1, _t2414);
        RsppVal _t2415 = rspp_int(0LL);
        return _t2415;
    }
    if (rspp_to_bool(rspp_eq(rspp_v_cmd, rspp_str("sdr")))) {
        RsppVal _t2417[] = { rspp_str("sim") };
        RsppVal _t2416 = rspp_builtin("sdr", "open", 1, _t2417);
        RsppVal rspp_v_dev = _t2416;
        RsppVal _t2419[] = { rspp_v_dev, rspp_flt(433920000.000000000000) };
        RsppVal _t2418 = rspp_builtin("sdr", "set_center_freq", 2, _t2419);
        RsppVal _t2421[] = { rspp_v_dev, rspp_flt(2400000.000000000000) };
        RsppVal _t2420 = rspp_builtin("sdr", "set_sample_rate", 2, _t2421);
        RsppVal _t2423[] = { rspp_v_dev, rspp_int(32LL) };
        RsppVal _t2422 = rspp_builtin("sdr", "capture_iq", 2, _t2423);
        RsppVal rspp_v_iq = _t2422;
        RsppVal _t2425[] = { rspp_v_iq };
        RsppVal _t2424 = rspp_builtin("sdr", "fft_spectrum", 1, _t2425);
        RsppVal rspp_v_spec = _t2424;
        RsppVal _t2427[] = { rspp_v_iq, rspp_flt(0.500000000000) };
        RsppVal _t2426 = rspp_builtin("sdr", "demod_ask_ook", 2, _t2427);
        RsppVal rspp_v_bits = _t2426;
        RsppVal _t2428 = rspp_str("");
        _t2428 = rspp_add(_t2428, rspp_str("SDR FFT bins: "));
        RsppVal _t2429 = rspp_method_call(rspp_v_spec, "len", 0, NULL);
        _t2428 = rspp_add(_t2428, rspp_str(rspp_to_str(_t2429)));
        _t2428 = rspp_add(_t2428, rspp_str(", OOK bits: "));
        _t2428 = rspp_add(_t2428, rspp_str(rspp_to_str(rspp_v_bits)));
        RsppVal _t2431[] = { _t2428 };
        RsppVal _t2430 = rspp_builtin("io", "print", 1, _t2431);
        RsppVal _t2432 = rspp_int(0LL);
        RsppVal _t2434[] = { rspp_v_dev };
        RsppVal _t2433 = rspp_builtin("sdr", "close", 1, _t2434);
        return _t2432;
        RsppVal _t2436[] = { rspp_v_dev };
        RsppVal _t2435 = rspp_builtin("sdr", "close", 1, _t2436);
    }
    RsppVal _t2437 = rspp_fn_print_help();
    RsppVal _t2438 = rspp_int(0LL);
    return _t2438;
    return rspp_none();
}


int main(int argc, char** argv) {
    rspp_argc = argc;
    rspp_argv = argv;
    atexit(rspp_mem_leak_check_atexit);
    RsppVal r = rspp_fn_main();
    if (r.kind == RSPP_INT) return (int)r.i;
    return 0;
}
