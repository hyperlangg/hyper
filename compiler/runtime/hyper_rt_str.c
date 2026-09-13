#ifdef _MSC_VER
// Disables "secure" warnings from msvc
#define _CRT_SECURE_NO_WARNINGS
#endif

#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "unicode_case.inc"

enum { KIND_I64 = 0, KIND_STR = 2, KIND_NONE = 4, KIND_LIST = 5 };

extern int64_t hyper_rt_list_new(void);
extern void hyper_rt_list_push(int64_t list, int64_t payload, int64_t kind);
extern int64_t hyper_rt_list_len(int64_t list);
extern int64_t hyper_rt_list_get(int64_t list, int64_t index, int64_t *out_kind);
extern int64_t hyper_rt_value_to_str(int64_t payload, int64_t kind);
extern char *hyper_rt_str_dup(const char *s);

static void rt_fatal(int64_t line, const char *msg) {
    fflush(stdout);
    fprintf(stderr, "RuntimeError: line %lld: %s\n", (long long)line, msg);
    exit(70);
}

static char *rt_strdup(const char *s) {
    return hyper_rt_str_dup(s);
}

static const char *require_str(int64_t payload, int64_t kind, int64_t line) {
    if (kind != KIND_STR) {
        rt_fatal(line, "expected a string receiver");
    }
    return payload ? (const char *)(intptr_t)payload : "";
}

static const char *require_str_arg(int64_t payload, int64_t kind, int64_t line, const char *method) {
    if (kind != KIND_STR) {
        char buf[128];
        snprintf(buf, sizeof(buf), "'%s' expects a string argument", method);
        rt_fatal(line, buf);
    }
    return payload ? (const char *)(intptr_t)payload : "";
}

static int64_t require_i64(int64_t payload, int64_t kind, int64_t line, const char *method) {
    if (kind != KIND_I64) {
        char buf[128];
        snprintf(buf, sizeof(buf), "'%s' expects an integer argument", method);
        rt_fatal(line, buf);
    }
    return payload;
}

static char optional_fill(int64_t payload, int64_t kind, int64_t line, const char *method) {
    if (kind == KIND_NONE) {
        return ' ';
    }
    const char *s = require_str_arg(payload, kind, line, method);
    if (!s[0] || s[1]) {
        char buf[128];
        snprintf(buf, sizeof(buf), "'%s' fill character must be a single character", method);
        rt_fatal(line, buf);
    }
    return s[0];
}

static int64_t ret_str(char *owned) {
    return (int64_t)(intptr_t)(owned ? owned : rt_strdup(""));
}

static int64_t ret_copy(const char *s) {
    return ret_str(rt_strdup(s));
}

static size_t utf8_len(const char *s) {
    size_t n = 0;
    while (*s) {
        if ((*s & 0xC0) != 0x80) {
            n++;
        }
        s++;
    }
    return n;
}

static char *ascii_map(const char *s, int (*fn)(int)) {
    return map_string_case(s, fn == toupper);
}

static char *trim_both(const char *s) {
    while (*s && isspace((unsigned char)*s)) {
        s++;
    }
    if (!*s) {
        return rt_strdup("");
    }
    const char *end = s + strlen(s) - 1;
    while (end > s && isspace((unsigned char)*end)) {
        end--;
    }
    size_t len = (size_t)(end - s + 1);
    char *out = (char *)malloc(len + 1);
    if (!out) {
        return NULL;
    }
    memcpy(out, s, len);
    out[len] = '\0';
    return out;
}

static char *trim_start(const char *s) {
    while (*s && isspace((unsigned char)*s)) {
        s++;
    }
    return rt_strdup(s);
}

static char *trim_end(const char *s) {
    size_t n = strlen(s);
    while (n > 0 && isspace((unsigned char)s[n - 1])) {
        n--;
    }
    char *out = (char *)malloc(n + 1);
    if (!out) {
        return NULL;
    }
    memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

static char *capitalize_s(const char *s) {
    return capitalize_utf8(s);
}

static char *title_s(const char *s) {
    return title_utf8(s);
}

static char *swapcase_s(const char *s) {
    return swapcase_utf8(s);
}

static char *pad_center(const char *s, size_t width, char fill) {
    size_t len = utf8_len(s);
    if (width <= len) {
        return rt_strdup(s);
    }
    size_t pad = width - len;
    size_t left = pad / 2;
    size_t right = pad - left;
    size_t sn = strlen(s);
    char *out = (char *)malloc(sn + pad + 1);
    if (!out) {
        return NULL;
    }
    memset(out, fill, left);
    memcpy(out + left, s, sn);
    memset(out + left + sn, fill, right);
    out[left + sn + right] = '\0';
    return out;
}

static char *pad_left(const char *s, size_t width, char fill) {
    size_t len = utf8_len(s);
    if (width <= len) {
        return rt_strdup(s);
    }
    size_t pad = width - len;
    size_t sn = strlen(s);
    char *out = (char *)malloc(sn + pad + 1);
    if (!out) {
        return NULL;
    }
    memset(out, fill, pad);
    memcpy(out + pad, s, sn + 1);
    return out;
}

static char *pad_right(const char *s, size_t width, char fill) {
    size_t len = utf8_len(s);
    if (width <= len) {
        return rt_strdup(s);
    }
    size_t pad = width - len;
    size_t sn = strlen(s);
    char *out = (char *)malloc(sn + pad + 1);
    if (!out) {
        return NULL;
    }
    memcpy(out, s, sn);
    memset(out + sn, fill, pad);
    out[sn + pad] = '\0';
    return out;
}

static char *zfill_s(const char *s, size_t width) {
    size_t len = utf8_len(s);
    if (width <= len) {
        return rt_strdup(s);
    }
    const char *body = s;
    char sign = 0;
    if (*s == '+' || *s == '-') {
        sign = *s;
        body = s + 1;
    }
    size_t body_chars = utf8_len(body) + (sign ? 1 : 0);
    size_t zeros = width > body_chars ? width - body_chars : 0;
    size_t sn = strlen(body);
    char *out = (char *)malloc((sign ? 1 : 0) + zeros + sn + 1);
    if (!out) {
        return NULL;
    }
    size_t i = 0;
    if (sign) {
        out[i++] = sign;
    }
    memset(out + i, '0', zeros);
    i += zeros;
    memcpy(out + i, body, sn + 1);
    return out;
}

static int64_t push_parts(char **parts, size_t n) {
    int64_t list = hyper_rt_list_new();
    for (size_t i = 0; i < n; i++) {
        hyper_rt_list_push(list, (int64_t)(intptr_t)parts[i], KIND_STR);
    }
    return list;
}

static int64_t split_ws(const char *s) {
    int64_t list = hyper_rt_list_new();
    while (*s) {
        while (*s && isspace((unsigned char)*s)) {
            s++;
        }
        if (!*s) {
            break;
        }
        const char *start = s;
        while (*s && !isspace((unsigned char)*s)) {
            s++;
        }
        size_t len = (size_t)(s - start);
        char *part = (char *)malloc(len + 1);
        if (part) {
            memcpy(part, start, len);
            part[len] = '\0';
            hyper_rt_list_push(list, (int64_t)(intptr_t)part, KIND_STR);
        }
    }
    return list;
}

static int64_t split_sep(const char *s, const char *sep, int from_right) {
    int64_t list = hyper_rt_list_new();
    size_t dlen = strlen(sep);
    if (dlen == 0) {
        for (const char *p = s; *p; p++) {
            char ch[2] = {*p, '\0'};
            hyper_rt_list_push(list, (int64_t)(intptr_t)rt_strdup(ch), KIND_STR);
        }
        return list;
    }

    if (!from_right) {
        const char *start = s;
        const char *found;
        while ((found = strstr(start, sep)) != NULL) {
            size_t part_len = (size_t)(found - start);
            char *part = (char *)malloc(part_len + 1);
            if (part) {
                memcpy(part, start, part_len);
                part[part_len] = '\0';
                hyper_rt_list_push(list, (int64_t)(intptr_t)part, KIND_STR);
            }
            start = found + dlen;
        }
        hyper_rt_list_push(list, (int64_t)(intptr_t)rt_strdup(start), KIND_STR);
        return list;
    }

    /* rsplit: collect then reverse order as Python rsplit without maxsplit */
    char **parts = NULL;
    size_t n = 0;
    size_t cap = 0;
    const char *end = s + strlen(s);
    while (end > s) {
        const char *found = NULL;
        for (const char *p = s; p + dlen <= end; p++) {
            if (memcmp(p, sep, dlen) == 0) {
                found = p;
            }
        }
        if (!found) {
            break;
        }
        size_t part_len = (size_t)(end - (found + dlen));
        char *part = (char *)malloc(part_len + 1);
        if (part) {
            memcpy(part, found + dlen, part_len);
            part[part_len] = '\0';
        }
        if (n == cap) {
            cap = cap ? cap * 2 : 8;
            parts = (char **)realloc(parts, cap * sizeof(char *));
        }
        parts[n++] = part;
        end = found;
    }
    size_t first_len = (size_t)(end - s);
    char *first = (char *)malloc(first_len + 1);
    if (first) {
        memcpy(first, s, first_len);
        first[first_len] = '\0';
    }
    if (n == cap) {
        cap = cap ? cap * 2 : 8;
        parts = (char **)realloc(parts, cap * sizeof(char *));
    }
    parts[n++] = first;
    for (size_t i = 0; i < n; i++) {
        hyper_rt_list_push(list, (int64_t)(intptr_t)parts[i], KIND_STR);
    }
    free(parts);
    return list;
}

static int64_t char_find(const char *s, const char *sub, int from_right) {
    if (!*sub) {
        return from_right ? (int64_t)utf8_len(s) : 0;
    }
    const char *found = NULL;
    if (!from_right) {
        found = strstr(s, sub);
    } else {
        for (const char *p = s; (p = strstr(p, sub)) != NULL; p++) {
            found = p;
        }
    }
    if (!found) {
        return -1;
    }
    size_t n = 0;
    for (const char *p = s; p < found; p++) {
        if ((*p & 0xC0) != 0x80) {
            n++;
        }
    }
    return (int64_t)n;
}

static int64_t count_sub(const char *s, const char *sub) {
    if (!*sub) {
        return (int64_t)utf8_len(s) + 1;
    }
    size_t dlen = strlen(sub);
    size_t count = 0;
    const char *p = s;
    while ((p = strstr(p, sub)) != NULL) {
        count++;
        p += dlen;
    }
    return (int64_t)count;
}

static int pred_isdigit(const char *s) {
    if (!*s) {
        return 0;
    }
    for (; *s; s++) {
        if (!isdigit((unsigned char)*s)) {
            return 0;
        }
    }
    return 1;
}

static int pred_isalpha(const char *s) {
    if (!*s) {
        return 0;
    }
    for (; *s; s++) {
        if (!isalpha((unsigned char)*s)) {
            return 0;
        }
    }
    return 1;
}

static int pred_isalnum(const char *s) {
    if (!*s) {
        return 0;
    }
    for (; *s; s++) {
        if (!isalnum((unsigned char)*s)) {
            return 0;
        }
    }
    return 1;
}

static int pred_isspace(const char *s) {
    if (!*s) {
        return 0;
    }
    for (; *s; s++) {
        if (!isspace((unsigned char)*s)) {
            return 0;
        }
    }
    return 1;
}

static int pred_islower(const char *s) {
    int has = 0;
    const char *p = s;
    uint32_t cp;
    while (utf8_next(&p, &cp)) {
        if (is_upper_cp(cp)) {
            return 0;
        }
        if (is_lower_cp(cp)) {
            has = 1;
        }
    }
    return has;
}

static int pred_isupper(const char *s) {
    int has = 0;
    const char *p = s;
    uint32_t cp;
    while (utf8_next(&p, &cp)) {
        if (is_lower_cp(cp)) {
            return 0;
        }
        if (is_upper_cp(cp)) {
            has = 1;
        }
    }
    return has;
}

static int pred_istitle(const char *s) {
    int saw = 0;
    int expect_upper = 1;
    const char *p = s;
    uint32_t cp;
    if (!s || !*s) {
        return 0;
    }
    while (utf8_next(&p, &cp)) {
        if (!is_cased_cp(cp)) {
            expect_upper = 1;
            continue;
        }
        if (is_upper_cp(cp)) {
            if (!expect_upper) {
                return 0;
            }
            saw = 1;
            expect_upper = 0;
        } else if (is_lower_cp(cp)) {
            if (expect_upper) {
                return 0;
            }
            saw = 1;
        }
    }
    return saw;
}

static int pred_isascii(const char *s) {
    for (; *s; s++) {
        if ((unsigned char)*s > 127) {
            return 0;
        }
    }
    return 1;
}

#define STR0(name, body) \
int64_t name(int64_t payload, int64_t kind, int64_t line, int64_t _lk) { \
    (void)_lk; \
    const char *s = require_str(payload, kind, line); \
    body \
}

STR0(hyper_rt_str_upper, return ret_str(ascii_map(s, toupper));)
STR0(hyper_rt_str_lower, return ret_str(ascii_map(s, tolower));)
STR0(hyper_rt_str_capitalize, return ret_str(capitalize_s(s));)
STR0(hyper_rt_str_title, return ret_str(title_s(s));)
STR0(hyper_rt_str_swapcase, return ret_str(swapcase_s(s));)
STR0(hyper_rt_str_strip, return ret_str(trim_both(s));)
STR0(hyper_rt_str_lstrip, return ret_str(trim_start(s));)
STR0(hyper_rt_str_rstrip, return ret_str(trim_end(s));)
STR0(hyper_rt_str_isdigit, return pred_isdigit(s);)
STR0(hyper_rt_str_isalpha, return pred_isalpha(s);)
STR0(hyper_rt_str_isalnum, return pred_isalnum(s);)
STR0(hyper_rt_str_isspace, return pred_isspace(s);)
STR0(hyper_rt_str_islower, return pred_islower(s);)
STR0(hyper_rt_str_isupper, return pred_isupper(s);)
STR0(hyper_rt_str_istitle, return pred_istitle(s);)
STR0(hyper_rt_str_isascii, return pred_isascii(s);)

int64_t hyper_rt_str_startswith(
    int64_t payload, int64_t kind, int64_t prefix, int64_t prefix_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *p = require_str_arg(prefix, prefix_kind, line, "startswith");
    size_t plen = strlen(p);
    return (plen == 0 || strncmp(s, p, plen) == 0) ? 1 : 0;
}

int64_t hyper_rt_str_endswith(
    int64_t payload, int64_t kind, int64_t suffix, int64_t suffix_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *p = require_str_arg(suffix, suffix_kind, line, "endswith");
    size_t slen = strlen(s);
    size_t plen = strlen(p);
    if (plen > slen) {
        return 0;
    }
    return strcmp(s + slen - plen, p) == 0 ? 1 : 0;
}

int64_t hyper_rt_str_split(
    int64_t payload, int64_t kind, int64_t delim, int64_t delim_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    if (delim_kind == KIND_NONE) {
        return split_ws(s);
    }
    const char *sep = require_str_arg(delim, delim_kind, line, "split");
    return split_sep(s, sep, 0);
}

int64_t hyper_rt_str_rsplit(
    int64_t payload, int64_t kind, int64_t delim, int64_t delim_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    if (delim_kind == KIND_NONE) {
        return split_ws(s);
    }
    const char *sep = require_str_arg(delim, delim_kind, line, "rsplit");
    /* Without maxsplit, Python rsplit order matches split. */
    return split_sep(s, sep, 0);
}

int64_t hyper_rt_str_replace(
    int64_t payload, int64_t kind, int64_t oldv, int64_t old_kind, int64_t newv, int64_t new_kind,
    int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *old_s = require_str_arg(oldv, old_kind, line, "replace");
    const char *new_s = require_str_arg(newv, new_kind, line, "replace");
    if (!*old_s) {
        return ret_copy(s);
    }
    size_t old_len = strlen(old_s);
    size_t new_len = strlen(new_s);
    size_t s_len = strlen(s);
    size_t count = 0;
    const char *p = s;
    while ((p = strstr(p, old_s)) != NULL) {
        count++;
        p += old_len;
    }
    size_t out_len = s_len + count * (new_len - old_len);
    char *out = (char *)malloc(out_len + 1);
    if (!out) {
        return 0;
    }
    char *dst = out;
    p = s;
    const char *cur = s;
    while ((p = strstr(cur, old_s)) != NULL) {
        size_t prefix = (size_t)(p - cur);
        memcpy(dst, cur, prefix);
        dst += prefix;
        memcpy(dst, new_s, new_len);
        dst += new_len;
        cur = p + old_len;
    }
    strcpy(dst, cur);
    return (int64_t)(intptr_t)out;
}

int64_t hyper_rt_str_join(
    int64_t payload, int64_t kind, int64_t list, int64_t list_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *sep = require_str(payload, kind, line);
    if (list_kind != KIND_LIST) {
        rt_fatal(line, "'join' expects a list argument");
    }
    int64_t n = hyper_rt_list_len(list);
    size_t sep_len = strlen(sep);
    size_t total = 0;
    char **parts = (char **)calloc((size_t)n, sizeof(char *));
    if (!parts && n > 0) {
        return 0;
    }
    for (int64_t i = 0; i < n; i++) {
        int64_t k = 0;
        int64_t item = hyper_rt_list_get(list, i, &k);
        int64_t text = hyper_rt_value_to_str(item, k);
        parts[i] = text ? (char *)(intptr_t)text : rt_strdup("");
        total += strlen(parts[i]);
        if (i + 1 < n) {
            total += sep_len;
        }
    }
    char *out = (char *)malloc(total + 1);
    if (!out) {
        for (int64_t i = 0; i < n; i++) {
            free(parts[i]);
        }
        free(parts);
        return 0;
    }
    out[0] = '\0';
    for (int64_t i = 0; i < n; i++) {
        strcat(out, parts[i]);
        if (i + 1 < n) {
            strcat(out, sep);
        }
        free(parts[i]);
    }
    free(parts);
    return (int64_t)(intptr_t)out;
}

int64_t hyper_rt_str_find(
    int64_t payload, int64_t kind, int64_t sub, int64_t sub_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *needle = require_str_arg(sub, sub_kind, line, "find");
    return char_find(s, needle, 0);
}

int64_t hyper_rt_str_rfind(
    int64_t payload, int64_t kind, int64_t sub, int64_t sub_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *needle = require_str_arg(sub, sub_kind, line, "rfind");
    return char_find(s, needle, 1);
}

int64_t hyper_rt_str_index(
    int64_t payload, int64_t kind, int64_t sub, int64_t sub_kind, int64_t line, int64_t _lk
) {
    int64_t idx = hyper_rt_str_find(payload, kind, sub, sub_kind, line, _lk);
    if (idx < 0) {
        rt_fatal(line, "substring not found");
    }
    return idx;
}

int64_t hyper_rt_str_rindex(
    int64_t payload, int64_t kind, int64_t sub, int64_t sub_kind, int64_t line, int64_t _lk
) {
    int64_t idx = hyper_rt_str_rfind(payload, kind, sub, sub_kind, line, _lk);
    if (idx < 0) {
        rt_fatal(line, "substring not found");
    }
    return idx;
}

int64_t hyper_rt_str_count(
    int64_t payload, int64_t kind, int64_t sub, int64_t sub_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *needle = require_str_arg(sub, sub_kind, line, "count");
    return count_sub(s, needle);
}

int64_t hyper_rt_str_center(
    int64_t payload, int64_t kind, int64_t width, int64_t width_kind, int64_t fill, int64_t fill_kind,
    int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    int64_t w = require_i64(width, width_kind, line, "center");
    char fill_ch = optional_fill(fill, fill_kind, line, "center");
    if (w < 0) {
        w = 0;
    }
    return ret_str(pad_center(s, (size_t)w, fill_ch));
}

int64_t hyper_rt_str_ljust(
    int64_t payload, int64_t kind, int64_t width, int64_t width_kind, int64_t fill, int64_t fill_kind,
    int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    int64_t w = require_i64(width, width_kind, line, "ljust");
    char fill_ch = optional_fill(fill, fill_kind, line, "ljust");
    if (w < 0) {
        w = 0;
    }
    return ret_str(pad_right(s, (size_t)w, fill_ch));
}

int64_t hyper_rt_str_rjust(
    int64_t payload, int64_t kind, int64_t width, int64_t width_kind, int64_t fill, int64_t fill_kind,
    int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    int64_t w = require_i64(width, width_kind, line, "rjust");
    char fill_ch = optional_fill(fill, fill_kind, line, "rjust");
    if (w < 0) {
        w = 0;
    }
    return ret_str(pad_left(s, (size_t)w, fill_ch));
}

int64_t hyper_rt_str_zfill(
    int64_t payload, int64_t kind, int64_t width, int64_t width_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    int64_t w = require_i64(width, width_kind, line, "zfill");
    if (w < 0) {
        w = 0;
    }
    return ret_str(zfill_s(s, (size_t)w));
}

int64_t hyper_rt_str_removeprefix(
    int64_t payload, int64_t kind, int64_t prefix, int64_t prefix_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *p = require_str_arg(prefix, prefix_kind, line, "removeprefix");
    size_t plen = strlen(p);
    if (plen && strncmp(s, p, plen) == 0) {
        return ret_copy(s + plen);
    }
    return ret_copy(s);
}

int64_t hyper_rt_str_removesuffix(
    int64_t payload, int64_t kind, int64_t suffix, int64_t suffix_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *p = require_str_arg(suffix, suffix_kind, line, "removesuffix");
    size_t slen = strlen(s);
    size_t plen = strlen(p);
    if (plen && plen <= slen && strcmp(s + slen - plen, p) == 0) {
        char *out = (char *)malloc(slen - plen + 1);
        if (!out) {
            return 0;
        }
        memcpy(out, s, slen - plen);
        out[slen - plen] = '\0';
        return (int64_t)(intptr_t)out;
    }
    return ret_copy(s);
}

int64_t hyper_rt_str_partition(
    int64_t payload, int64_t kind, int64_t sep, int64_t sep_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *d = require_str_arg(sep, sep_kind, line, "partition");
    const char *found = strstr(s, d);
    char *parts[3];
    if (!found) {
        parts[0] = rt_strdup(s);
        parts[1] = rt_strdup("");
        parts[2] = rt_strdup("");
    } else {
        size_t before = (size_t)(found - s);
        parts[0] = (char *)malloc(before + 1);
        if (parts[0]) {
            memcpy(parts[0], s, before);
            parts[0][before] = '\0';
        }
        parts[1] = rt_strdup(d);
        parts[2] = rt_strdup(found + strlen(d));
    }
    return push_parts(parts, 3);
}

int64_t hyper_rt_str_rpartition(
    int64_t payload, int64_t kind, int64_t sep, int64_t sep_kind, int64_t line, int64_t _lk
) {
    (void)_lk;
    const char *s = require_str(payload, kind, line);
    const char *d = require_str_arg(sep, sep_kind, line, "rpartition");
    size_t dlen = strlen(d);
    const char *found = NULL;
    if (dlen) {
        for (const char *p = s; (p = strstr(p, d)) != NULL; p++) {
            found = p;
        }
    }
    char *parts[3];
    if (!found) {
        parts[0] = rt_strdup("");
        parts[1] = rt_strdup("");
        parts[2] = rt_strdup(s);
    } else {
        size_t before = (size_t)(found - s);
        parts[0] = (char *)malloc(before + 1);
        if (parts[0]) {
            memcpy(parts[0], s, before);
            parts[0][before] = '\0';
        }
        parts[1] = rt_strdup(d);
        parts[2] = rt_strdup(found + dlen);
    }
    return push_parts(parts, 3);
}
