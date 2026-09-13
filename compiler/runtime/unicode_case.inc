/* Locale-independent Unicode case mapping for the AOT string runtime.
 * Covers ASCII, Latin-1, Latin Extended-A pairs, Cyrillic, Greek, and the
 * expansions Rust uses for ß / İ / ı. */
extern void hyper_rt_owned_str_register(void *p);

static char *finish_case(char *out) {
    if (out) {
        hyper_rt_owned_str_register(out);
    }
    return out;
}

static int utf8_next(const char **ps, uint32_t *cp) {
    const unsigned char *s = (const unsigned char *)*ps;
    if (!s || !*s) {
        return 0;
    }
    uint32_t c = s[0];
    int n = 1;
    if (c < 0x80) {
        *cp = c;
    } else if ((c & 0xE0) == 0xC0 && s[1] && (s[1] & 0xC0) == 0x80) {
        *cp = ((c & 0x1F) << 6) | (s[1] & 0x3F);
        n = 2;
    } else if ((c & 0xF0) == 0xE0 && s[1] && s[2] && (s[1] & 0xC0) == 0x80 && (s[2] & 0xC0) == 0x80) {
        *cp = ((c & 0x0F) << 12) | ((s[1] & 0x3F) << 6) | (s[2] & 0x3F);
        n = 3;
    } else if ((c & 0xF8) == 0xF0 && s[1] && s[2] && s[3]) {
        *cp = ((c & 0x07) << 18) | ((s[1] & 0x3F) << 12) | ((s[2] & 0x3F) << 6) | (s[3] & 0x3F);
        n = 4;
    } else {
        *cp = 0xFFFD;
    }
    *ps += n;
    return 1;
}

static int utf8_put(char *dst, uint32_t cp) {
    if (cp < 0x80) {
        dst[0] = (char)cp;
        return 1;
    }
    if (cp < 0x800) {
        dst[0] = (char)(0xC0 | (cp >> 6));
        dst[1] = (char)(0x80 | (cp & 0x3F));
        return 2;
    }
    if (cp < 0x10000) {
        dst[0] = (char)(0xE0 | (cp >> 12));
        dst[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        dst[2] = (char)(0x80 | (cp & 0x3F));
        return 3;
    }
    dst[0] = (char)(0xF0 | (cp >> 18));
    dst[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
    dst[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
    dst[3] = (char)(0x80 | (cp & 0x3F));
    return 4;
}

static int is_lower_cp(uint32_t cp) {
    if (cp >= 'a' && cp <= 'z') return 1;
    if (cp == 0x00DF || cp == 0x0131 || cp == 0x0138) return 1;
    if (cp >= 0x00E0 && cp <= 0x00FE && cp != 0x00F7) return 1;
    if (cp >= 0x0101 && cp <= 0x0177 && (cp & 1) == 1 && cp != 0x0149) return 1;
    if (cp >= 0x0430 && cp <= 0x044F) return 1;
    if (cp >= 0x0450 && cp <= 0x045F) return 1;
    if (cp >= 0x03B1 && cp <= 0x03C9) return 1;
    return 0;
}

static int is_upper_cp(uint32_t cp) {
    if (cp >= 'A' && cp <= 'Z') return 1;
    if (cp == 0x0130 || cp == 0x0178) return 1;
    if (cp >= 0x00C0 && cp <= 0x00DE && cp != 0x00D7) return 1;
    if (cp >= 0x0100 && cp <= 0x0176 && (cp & 1) == 0) return 1;
    if (cp >= 0x0410 && cp <= 0x042F) return 1;
    if (cp >= 0x0400 && cp <= 0x040F) return 1;
    if ((cp >= 0x0391 && cp <= 0x03A1) || (cp >= 0x03A3 && cp <= 0x03A9)) return 1;
    return 0;
}

static int is_cased_cp(uint32_t cp) {
    return is_lower_cp(cp) || is_upper_cp(cp);
}

/* Write 1–2 mapped code points. `upper` selects the direction. */
static int map_case_cp(uint32_t cp, int upper, uint32_t out[2]) {
    if (upper && cp == 0x00DF) {
        out[0] = 'S';
        out[1] = 'S';
        return 2;
    }
    if (!upper && cp == 0x0130) {
        out[0] = 'i';
        out[1] = 0x0307;
        return 2;
    }
    if (upper && cp == 0x0131) {
        out[0] = 'I';
        return 1;
    }
    if (upper && cp == 0x00FF) {
        out[0] = 0x0178;
        return 1;
    }
    if (!upper && cp == 0x0178) {
        out[0] = 0x00FF;
        return 1;
    }
    if (upper && cp == 0x03C2) {
        out[0] = 0x03A3;
        return 1;
    }
    if (cp >= 'a' && cp <= 'z') {
        out[0] = upper ? cp - 32 : cp;
        return 1;
    }
    if (cp >= 'A' && cp <= 'Z') {
        out[0] = upper ? cp : cp + 32;
        return 1;
    }
    if (cp >= 0x00C0 && cp <= 0x00DE && cp != 0x00D7) {
        out[0] = upper ? cp : cp + 32;
        return 1;
    }
    if (cp >= 0x00E0 && cp <= 0x00FE && cp != 0x00F7) {
        out[0] = upper ? cp - 32 : cp;
        return 1;
    }
    if (cp >= 0x0100 && cp <= 0x0177 && cp != 0x0138 && cp != 0x0149) {
        if ((cp & 1) == 0) {
            out[0] = upper ? cp : cp + 1;
        } else {
            out[0] = upper ? cp - 1 : cp;
        }
        return 1;
    }
    if (cp >= 0x0410 && cp <= 0x042F) {
        out[0] = upper ? cp : cp + 0x20;
        return 1;
    }
    if (cp >= 0x0430 && cp <= 0x044F) {
        out[0] = upper ? cp - 0x20 : cp;
        return 1;
    }
    if (cp >= 0x0400 && cp <= 0x040F) {
        out[0] = upper ? cp : cp + 0x50;
        return 1;
    }
    if (cp >= 0x0450 && cp <= 0x045F) {
        out[0] = upper ? cp - 0x50 : cp;
        return 1;
    }
    if ((cp >= 0x0391 && cp <= 0x03A1) || (cp >= 0x03A3 && cp <= 0x03A9)) {
        out[0] = upper ? cp : cp + 0x20;
        return 1;
    }
    if (cp >= 0x03B1 && cp <= 0x03C1) {
        out[0] = upper ? cp - 0x20 : cp;
        return 1;
    }
    if (cp >= 0x03C3 && cp <= 0x03C9) {
        out[0] = upper ? cp - 0x20 : cp;
        return 1;
    }
    out[0] = cp;
    return 1;
}

static int buf_grow(char **data, size_t *cap, size_t need) {
    if (need <= *cap) {
        return 1;
    }
    size_t ncap = *cap ? *cap : 16;
    while (ncap < need) {
        ncap *= 2;
    }
    char *n = (char *)realloc(*data, ncap);
    if (!n) {
        return 0;
    }
    *data = n;
    *cap = ncap;
    return 1;
}

static int buf_append_cp(char **data, size_t *len, size_t *cap, uint32_t cp) {
    char tmp[4];
    int n = utf8_put(tmp, cp);
    if (!buf_grow(data, cap, *len + (size_t)n + 1)) {
        return 0;
    }
    memcpy(*data + *len, tmp, (size_t)n);
    *len += (size_t)n;
    (*data)[*len] = '\0';
    return 1;
}

static char *map_string_case(const char *s, int upper) {
    char *out = NULL;
    size_t len = 0;
    size_t cap = 0;
    const char *p = s ? s : "";
    uint32_t cp;
    if (!buf_grow(&out, &cap, 1)) {
        return NULL;
    }
    out[0] = '\0';
    while (utf8_next(&p, &cp)) {
        uint32_t mapped[2];
        int n = map_case_cp(cp, upper, mapped);
        for (int i = 0; i < n; i++) {
            if (!buf_append_cp(&out, &len, &cap, mapped[i])) {
                free(out);
                return NULL;
            }
        }
    }
    return finish_case(out);
}

static char *capitalize_utf8(const char *s) {
    char *out = NULL;
    size_t len = 0;
    size_t cap = 0;
    const char *p = s ? s : "";
    uint32_t cp;
    int first = 1;
    if (!buf_grow(&out, &cap, 1)) {
        return NULL;
    }
    out[0] = '\0';
    while (utf8_next(&p, &cp)) {
        uint32_t mapped[2];
        int n = map_case_cp(cp, first ? 1 : 0, mapped);
        first = 0;
        for (int i = 0; i < n; i++) {
            if (!buf_append_cp(&out, &len, &cap, mapped[i])) {
                free(out);
                return NULL;
            }
        }
    }
    return finish_case(out);
}

static char *title_utf8(const char *s) {
    char *out = NULL;
    size_t len = 0;
    size_t cap = 0;
    const char *p = s ? s : "";
    uint32_t cp;
    int cap_next = 1;
    if (!buf_grow(&out, &cap, 1)) {
        return NULL;
    }
    out[0] = '\0';
    while (utf8_next(&p, &cp)) {
        if (!is_cased_cp(cp)) {
            if (!buf_append_cp(&out, &len, &cap, cp)) {
                free(out);
                return NULL;
            }
            cap_next = 1;
            continue;
        }
        uint32_t mapped[2];
        int n = map_case_cp(cp, cap_next, mapped);
        cap_next = 0;
        for (int i = 0; i < n; i++) {
            if (!buf_append_cp(&out, &len, &cap, mapped[i])) {
                free(out);
                return NULL;
            }
        }
    }
    return finish_case(out);
}

static char *swapcase_utf8(const char *s) {
    char *out = NULL;
    size_t len = 0;
    size_t cap = 0;
    const char *p = s ? s : "";
    uint32_t cp;
    if (!buf_grow(&out, &cap, 1)) {
        return NULL;
    }
    out[0] = '\0';
    while (utf8_next(&p, &cp)) {
        int upper = is_lower_cp(cp) ? 1 : 0;
        int lower = is_upper_cp(cp) ? 1 : 0;
        uint32_t mapped[2];
        int n = 1;
        mapped[0] = cp;
        if (upper) {
            n = map_case_cp(cp, 1, mapped);
        } else if (lower) {
            n = map_case_cp(cp, 0, mapped);
        }
        for (int i = 0; i < n; i++) {
            if (!buf_append_cp(&out, &len, &cap, mapped[i])) {
                free(out);
                return NULL;
            }
        }
    }
    return finish_case(out);
}
