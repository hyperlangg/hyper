//! Hyper-IR → LLVM IR text emitter and clang-driven AOT link (production path).
//!
//! Mirrors Cranelift AOT ABI: payloads are `i64`; calls pass `(payload, kind)` pairs;
//! user functions return `{i64, i64}`; `__main__` returns `i64`.

use super::codegen::{
    concat_consume_plan, names_needing_kind_vars, needs_runtime_eq, normalize_exe_path,
    runtime_builtins_c_path, runtime_c_path, runtime_file_c_path, runtime_io_c_path,
    runtime_json_c_path, runtime_mmap_c_path, runtime_str_c_path, ValueKind,
};
use super::ir::{BlockId, IrInstr, IrModule, IrOp, ValueId};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

fn host_triple() -> &'static str {
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        "arm64-apple-macosx"
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        "x86_64-apple-macosx"
    } else if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

fn escape_llvm_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'\\' => out.push_str("\\5C"),
            b'"' => out.push_str("\\22"),
            b'\n' => out.push_str("\\0A"),
            b'\r' => out.push_str("\\0D"),
            b'\t' => out.push_str("\\09"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:02X}")),
        }
    }
    out.push_str("\\00");
    out
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("n");
    }
    out
}

/// Runtime call: (result kind, uses out_kind pointer). Name is `func` itself.
fn classify_runtime(func: &str) -> Option<(ValueKind, bool)> {
    match func {
        "hyper_rt_file_open" => Some((ValueKind::File, false)),
        "hyper_rt_file_read_all" | "hyper_rt_file_path" | "hyper_rt_file_mode" => {
            Some((ValueKind::Str, false))
        }
        "hyper_rt_file_read_n" => Some((ValueKind::Str, false)),
        "hyper_rt_file_readline" => Some((ValueKind::Dynamic, true)),
        "hyper_rt_file_readlines" => Some((ValueKind::List, false)),
        "hyper_rt_file_write" | "hyper_rt_file_writelines" | "hyper_rt_file_seek"
        | "hyper_rt_file_tell" | "hyper_rt_file_size" => Some((ValueKind::I64, false)),
        "hyper_rt_file_is_closed" => Some((ValueKind::Bool, false)),
        "hyper_rt_file_close" | "hyper_rt_file_flush" => Some((ValueKind::None_, false)),
        "hyper_rt_mmap_open" => Some((ValueKind::Mmap, false)),
        "hyper_rt_mmap_read_chunk" => Some((ValueKind::Str, false)),
        "hyper_rt_mmap_close" => Some((ValueKind::None_, false)),
        "hyper_rt_clock" => Some((ValueKind::F64, false)),
        "hyper_rt_coll_len" => Some((ValueKind::I64, false)),
        "hyper_rt_coll_append" => Some((ValueKind::None_, false)),
        "hyper_rt_coll_keys" => Some((ValueKind::List, false)),
        "hyper_rt_builtin_len" => Some((ValueKind::I64, false)),
        "hyper_rt_builtin_abs" | "hyper_rt_builtin_min" | "hyper_rt_builtin_max"
        | "hyper_rt_builtin_sum" | "hyper_rt_builtin_round" | "hyper_rt_builtin_pow"
        | "hyper_rt_builtin_int" | "hyper_rt_builtin_float" => Some((ValueKind::Dynamic, true)),
        "hyper_rt_builtin_divmod" => Some((ValueKind::List, false)),
        "hyper_rt_builtin_chr" | "hyper_rt_builtin_bin" | "hyper_rt_builtin_hex"
        | "hyper_rt_builtin_oct" | "hyper_rt_builtin_str" | "hyper_rt_builtin_repr" => {
            Some((ValueKind::Str, false))
        }
        "hyper_rt_builtin_ord" => Some((ValueKind::I64, false)),
        "hyper_rt_builtin_bool" | "hyper_rt_builtin_all" | "hyper_rt_builtin_any" => {
            Some((ValueKind::Bool, false))
        }
        "hyper_rt_builtin_sorted" | "hyper_rt_builtin_reversed" | "hyper_rt_builtin_enumerate"
        | "hyper_rt_builtin_zip" | "hyper_rt_builtin_list" | "hyper_rt_builtin_range" => {
            Some((ValueKind::List, false))
        }
        "hyper_rt_str_upper" | "hyper_rt_str_lower" | "hyper_rt_str_capitalize"
        | "hyper_rt_str_title" | "hyper_rt_str_swapcase" | "hyper_rt_str_strip"
        | "hyper_rt_str_lstrip" | "hyper_rt_str_rstrip" | "hyper_rt_str_replace"
        | "hyper_rt_str_join" | "hyper_rt_str_center" | "hyper_rt_str_ljust"
        | "hyper_rt_str_rjust" | "hyper_rt_str_zfill" | "hyper_rt_str_removeprefix"
        | "hyper_rt_str_removesuffix" => Some((ValueKind::Str, false)),
        "hyper_rt_str_startswith" | "hyper_rt_str_endswith" | "hyper_rt_str_isdigit"
        | "hyper_rt_str_isalpha" | "hyper_rt_str_isalnum" | "hyper_rt_str_isspace"
        | "hyper_rt_str_islower" | "hyper_rt_str_isupper" | "hyper_rt_str_istitle"
        | "hyper_rt_str_isascii" => Some((ValueKind::Bool, false)),
        "hyper_rt_str_find" | "hyper_rt_str_rfind" | "hyper_rt_str_index"
        | "hyper_rt_str_rindex" | "hyper_rt_str_count" => Some((ValueKind::I64, false)),
        "hyper_rt_str_split" | "hyper_rt_str_rsplit" | "hyper_rt_str_partition"
        | "hyper_rt_str_rpartition" => Some((ValueKind::List, false)),
        "hyper_rt_input" => Some((ValueKind::Str, false)),
        "hyper_rt_json_loads" | "hyper_rt_json_load" => Some((ValueKind::Dynamic, true)),
        "hyper_rt_json_dumps" => Some((ValueKind::Str, false)),
        "hyper_rt_json_dump" => Some((ValueKind::I64, false)),
        "hyper_rt_handle_enter" => Some((ValueKind::I64, false)),
        "hyper_rt_handle_leave" => Some((ValueKind::Bool, false)),
        "hyper_rt_raise" => Some((ValueKind::I64, false)),
        _ => None,
    }
}

fn runtime_extern_decls() -> &'static str {
    r#"declare void @hyper_rt_print_i64(i64)
declare void @hyper_rt_print_f64(double)
declare void @hyper_rt_print_str(i64)
declare void @hyper_rt_print_newline()
declare void @hyper_rt_print_separator()
declare void @hyper_rt_print_list(i64)
declare void @hyper_rt_print_dict(i64)
declare void @hyper_rt_print_value(i64, i64)
declare void @hyper_rt_print_struct(i64)
declare i64 @hyper_rt_pow_i64(i64, i64)
declare double @hyper_rt_pow_f64(double, double)
declare i64 @hyper_rt_floor_div_i64(i64, i64)
declare double @hyper_rt_floor_div_f64(double, double)
declare i64 @hyper_rt_list_new()
declare void @hyper_rt_list_push(i64, i64, i64)
declare i64 @hyper_rt_list_get(i64, i64, i64)
declare void @hyper_rt_list_set(i64, i64, i64, i64)
declare i64 @hyper_rt_list_len(i64)
declare i64 @hyper_rt_dict_new()
declare void @hyper_rt_dict_push(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_dict_get(i64, i64, i64, i64)
declare void @hyper_rt_dict_set(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_index_get(i64, i64, i64, i64, i64)
declare void @hyper_rt_index_set(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_value_to_str(i64, i64)
declare i64 @hyper_rt_value_eq(i64, i64, i64, i64)
declare void @hyper_rt_div_by_zero(i64)
declare i64 @hyper_rt_str_concat(i64, i64, i64, i64)
declare i64 @hyper_rt_struct_new(i64)
declare i64 @hyper_rt_struct_get(i64, i64, i64)
declare void @hyper_rt_struct_set(i64, i64, i64, i64)
declare i64 @hyper_rt_file_open(i64, i64, i64, i64, i64, i64)
declare void @hyper_rt_file_close(i64, i64, i64, i64)
declare i64 @hyper_rt_file_read_all(i64, i64, i64, i64)
declare i64 @hyper_rt_file_read_n(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_file_readline(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_file_readlines(i64, i64, i64, i64)
declare i64 @hyper_rt_file_write(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_file_writelines(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_file_seek(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_file_tell(i64, i64, i64, i64)
declare i64 @hyper_rt_file_size(i64, i64, i64, i64)
declare void @hyper_rt_file_flush(i64, i64, i64, i64)
declare i64 @hyper_rt_file_is_closed(i64, i64)
declare i64 @hyper_rt_file_path(i64, i64, i64, i64)
declare i64 @hyper_rt_file_mode(i64, i64, i64, i64)
declare i64 @hyper_rt_mmap_open(i64, i64, i64, i64)
declare void @hyper_rt_mmap_close(i64, i64, i64, i64)
declare i64 @hyper_rt_mmap_read_chunk(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_input(i64, i64, i64, i64)
declare i64 @hyper_rt_clock()
declare i64 @hyper_rt_coll_len(i64, i64, i64, i64)
declare void @hyper_rt_coll_append(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_coll_keys(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_len(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_abs(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_min(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_max(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_sum(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_round(i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_pow(i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_divmod(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_chr(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_ord(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_bin(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_hex(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_oct(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_int(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_float(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_str(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_bool(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_all(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_any(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_sorted(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_reversed(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_enumerate(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_zip(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_list(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_range(i64, i64, i64, i64)
declare i64 @hyper_rt_builtin_repr(i64, i64, i64, i64)
declare i64 @hyper_rt_str_upper(i64, i64, i64, i64)
declare i64 @hyper_rt_str_lower(i64, i64, i64, i64)
declare i64 @hyper_rt_str_capitalize(i64, i64, i64, i64)
declare i64 @hyper_rt_str_title(i64, i64, i64, i64)
declare i64 @hyper_rt_str_swapcase(i64, i64, i64, i64)
declare i64 @hyper_rt_str_strip(i64, i64, i64, i64)
declare i64 @hyper_rt_str_lstrip(i64, i64, i64, i64)
declare i64 @hyper_rt_str_rstrip(i64, i64, i64, i64)
declare i64 @hyper_rt_str_startswith(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_endswith(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_split(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_rsplit(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_replace(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_join(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_find(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_rfind(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_index(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_rindex(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_count(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_isdigit(i64, i64, i64, i64)
declare i64 @hyper_rt_str_isalpha(i64, i64, i64, i64)
declare i64 @hyper_rt_str_isalnum(i64, i64, i64, i64)
declare i64 @hyper_rt_str_isspace(i64, i64, i64, i64)
declare i64 @hyper_rt_str_islower(i64, i64, i64, i64)
declare i64 @hyper_rt_str_isupper(i64, i64, i64, i64)
declare i64 @hyper_rt_str_istitle(i64, i64, i64, i64)
declare i64 @hyper_rt_str_isascii(i64, i64, i64, i64)
declare i64 @hyper_rt_str_center(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_ljust(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_rjust(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_zfill(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_removeprefix(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_removesuffix(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_partition(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_str_rpartition(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_json_loads(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_json_dumps(i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_json_load(i64, i64, i64, i64, i64)
declare i64 @hyper_rt_json_dump(i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @hyper_rt_handle_enter()
declare i64 @hyper_rt_handle_leave()
declare i64 @hyper_rt_raise(i64, i64, i64, i64)
declare void @hyper_rt_parallel_for(i64, i64, {i64, i64} (i64, i64)*)
"#
}

struct FuncEmitter {
    body: String,
    next_tmp: usize,
    value_slots: HashMap<ValueId, String>,
    kind_slots: HashMap<ValueId, String>,
    named_slots: HashMap<String, String>,
    named_kind_slots: HashMap<String, String>,
    value_kinds: HashMap<ValueId, ValueKind>,
    named_kinds: HashMap<String, ValueKind>,
    blocks: HashMap<BlockId, String>,
    terminated: bool,
    returns_kind: bool,
    user_funcs: HashSet<String>,
    out_kind_n: usize,
}

impl FuncEmitter {
    fn new(returns_kind: bool, user_funcs: HashSet<String>) -> Self {
        Self {
            body: String::new(),
            next_tmp: 0,
            value_slots: HashMap::new(),
            kind_slots: HashMap::new(),
            named_slots: HashMap::new(),
            named_kind_slots: HashMap::new(),
            value_kinds: HashMap::new(),
            named_kinds: HashMap::new(),
            blocks: HashMap::new(),
            terminated: false,
            returns_kind,
            user_funcs,
            out_kind_n: 0,
        }
    }

    fn tmp(&mut self) -> String {
        let t = format!("%t{}", self.next_tmp);
        self.next_tmp += 1;
        t
    }

    fn emit(&mut self, line: &str) {
        self.body.push_str("  ");
        self.body.push_str(line);
        self.body.push('\n');
    }

    fn emit_raw(&mut self, line: &str) {
        self.body.push_str(line);
        self.body.push('\n');
    }

    fn kind_of(&self, id: ValueId) -> ValueKind {
        self.value_kinds.get(&id).copied().unwrap_or(ValueKind::I64)
    }

    fn named_kind(&self, name: &str) -> ValueKind {
        self.named_kinds
            .get(name)
            .copied()
            .unwrap_or(ValueKind::I64)
    }

    fn load_val(&mut self, id: ValueId) -> String {
        let slot = self.value_slots[&id].clone();
        let t = self.tmp();
        self.emit(&format!("{t} = load i64, ptr {slot}, align 8"));
        t
    }

    fn store_val(&mut self, id: ValueId, val: &str) {
        let slot = self.value_slots[&id].clone();
        self.emit(&format!("store i64 {val}, ptr {slot}, align 8"));
    }

    fn load_named(&mut self, name: &str) -> String {
        let slot = self.named_slots[name].clone();
        let t = self.tmp();
        self.emit(&format!("{t} = load i64, ptr {slot}, align 8"));
        t
    }

    fn store_named(&mut self, name: &str, val: &str) {
        let slot = self.named_slots[name].clone();
        self.emit(&format!("store i64 {val}, ptr {slot}, align 8"));
    }

    fn ensure_kind_slot(&mut self, id: ValueId) -> String {
        if let Some(s) = self.kind_slots.get(&id) {
            return s.clone();
        }
        let s = format!("%kv{}", id);
        self.emit(&format!("{s} = alloca i64, align 8"));
        self.kind_slots.insert(id, s.clone());
        s
    }

    fn store_kind_dyn(&mut self, id: ValueId, kind_val: &str) {
        let slot = self.ensure_kind_slot(id);
        self.emit(&format!("store i64 {kind_val}, ptr {slot}, align 8"));
    }

    fn load_kind_dyn(&mut self, id: ValueId) -> String {
        let slot = self.kind_slots.get(&id).cloned().unwrap_or_else(|| {
            let s = format!("%kv{}", id);
            self.emit(&format!("{s} = alloca i64, align 8"));
            self.emit(&format!("store i64 0, ptr {s}, align 8"));
            self.kind_slots.insert(id, s.clone());
            s
        });
        let t = self.tmp();
        self.emit(&format!("{t} = load i64, ptr {slot}, align 8"));
        t
    }

    fn kind_operand(&mut self, kind: ValueKind, id: ValueId) -> String {
        match kind {
            ValueKind::Dynamic => self.load_kind_dyn(id),
            other => format!("{}", other.as_i64()),
        }
    }

    fn i64_to_f64(&mut self, v: &str) -> String {
        let t = self.tmp();
        self.emit(&format!("{t} = bitcast i64 {v} to double"));
        t
    }

    fn f64_to_i64(&mut self, v: &str) -> String {
        let t = self.tmp();
        self.emit(&format!("{t} = bitcast double {v} to i64"));
        t
    }

    fn sitofp(&mut self, v: &str) -> String {
        let t = self.tmp();
        self.emit(&format!("{t} = sitofp i64 {v} to double"));
        t
    }

    fn bool_ext(&mut self, cond: &str) -> String {
        let t = self.tmp();
        self.emit(&format!("{t} = zext i1 {cond} to i64"));
        t
    }

    fn emit_ret(&mut self, payload: &str, kind: Option<&str>) {
        if self.returns_kind {
            let k = kind.unwrap_or("4"); // None
            let a = self.tmp();
            self.emit(&format!("{a} = insertvalue {{ i64, i64 }} undef, i64 {payload}, 0"));
            let b = self.tmp();
            self.emit(&format!("{b} = insertvalue {{ i64, i64 }} {a}, i64 {k}, 1"));
            self.emit(&format!("ret {{ i64, i64 }} {b}"));
        } else {
            self.emit(&format!("ret i64 {payload}"));
        }
        self.terminated = true;
    }

    fn out_kind_alloca(&mut self) -> String {
        let s = format!("%outk{}", self.out_kind_n);
        self.out_kind_n += 1;
        self.emit(&format!("{s} = alloca i64, align 8"));
        s
    }

    fn emit_function(
        &mut self,
        name: &str,
        params: &[String],
        body: &[IrInstr],
        string_globals: &HashMap<String, (String, usize)>,
    ) -> Result<(), String> {
        // Collect ValueIds / names needing slots.
        let mut value_ids: HashSet<ValueId> = HashSet::new();
        let mut names: HashSet<String> = HashSet::new();
        for p in params {
            names.insert(p.clone());
        }
        for instr in body {
            match instr {
                IrInstr::ConstI64 { dest, .. }
                | IrInstr::ConstF64 { dest, .. }
                | IrInstr::ConstBool { dest, .. }
                | IrInstr::ConstNone { dest }
                | IrInstr::ConstStr { dest, .. }
                | IrInstr::Load { dest, .. }
                | IrInstr::Unary { dest, .. }
                | IrInstr::Binary { dest, .. }
                | IrInstr::IntWrap { dest, .. }
                | IrInstr::Call { dest, .. }
                | IrInstr::MakeList { dest, .. }
                | IrInstr::MakeDict { dest, .. }
                | IrInstr::IndexGet { dest, .. }
                | IrInstr::ListLen { dest, .. }
                | IrInstr::ValueToStr { dest, .. }
                | IrInstr::StrConcat { dest, .. }
                | IrInstr::MakeStruct { dest, .. }
                | IrInstr::StructGet { dest, .. } => {
                    value_ids.insert(*dest);
                }
                _ => {}
            }
            match instr {
                IrInstr::Load { name, .. } => {
                    names.insert(name.clone());
                }
                IrInstr::Store { name, value } => {
                    names.insert(name.clone());
                    value_ids.insert(*value);
                }
                IrInstr::Unary { src, .. }
                | IrInstr::IntWrap { src, .. }
                | IrInstr::GuardDivisor { value: src, .. }
                | IrInstr::ValueToStr { src, .. }
                | IrInstr::Return { value: Some(src) }
                | IrInstr::Branch { cond: src, .. } => {
                    value_ids.insert(*src);
                }
                IrInstr::Binary { left, right, .. } | IrInstr::StrConcat { left, right, .. } => {
                    value_ids.insert(*left);
                    value_ids.insert(*right);
                }
                IrInstr::Call { args, .. }
                | IrInstr::Print { args }
                | IrInstr::MakeList { items: args, .. } => {
                    for a in args {
                        value_ids.insert(*a);
                    }
                }
                IrInstr::MakeDict { entries, .. } => {
                    for (k, v) in entries {
                        value_ids.insert(*k);
                        value_ids.insert(*v);
                    }
                }
                IrInstr::IndexGet { object, index, .. } => {
                    value_ids.insert(*object);
                    value_ids.insert(*index);
                }
                IrInstr::IndexSet {
                    object,
                    index,
                    value,
                } => {
                    value_ids.insert(*object);
                    value_ids.insert(*index);
                    value_ids.insert(*value);
                }
                IrInstr::ListLen { list, .. } => {
                    value_ids.insert(*list);
                }
                IrInstr::StructGet { object, .. } => {
                    value_ids.insert(*object);
                }
                IrInstr::StructSet { object, value, .. } => {
                    value_ids.insert(*object);
                    value_ids.insert(*value);
                }
                _ => {}
            }
            if let IrInstr::Label { block } = instr {
                self.blocks
                    .entry(*block)
                    .or_insert_with(|| format!("bb{}", block));
            }
        }

        let kind_needed = names_needing_kind_vars(body, params);
        let concat_consume = concat_consume_plan(body);

        // Signature
        let mut sig_params = String::new();
        for (i, _) in params.iter().enumerate() {
            if i > 0 {
                sig_params.push_str(", ");
            }
            sig_params.push_str(&format!("i64 %arg_p{i}, i64 %arg_k{i}"));
        }
        let ret_ty = if self.returns_kind {
            "{ i64, i64 }"
        } else {
            "i64"
        };
        self.emit_raw(&format!("define {ret_ty} @{name}({sig_params}) {{"));
        self.emit_raw("entry:");

        for id in &value_ids {
            let slot = format!("%v{id}");
            self.emit(&format!("{slot} = alloca i64, align 8"));
            self.value_slots.insert(*id, slot);
        }
        for name in &names {
            let slot = format!("%n_{}", sanitize_name(name));
            self.emit(&format!("{slot} = alloca i64, align 8"));
            self.emit(&format!("store i64 0, ptr {slot}, align 8"));
            self.named_slots.insert(name.clone(), slot);
            if kind_needed.contains(name) {
                let ks = format!("%nk_{}", sanitize_name(name));
                self.emit(&format!("{ks} = alloca i64, align 8"));
                self.emit(&format!(
                    "store i64 {}, ptr {ks}, align 8",
                    ValueKind::I64.as_i64()
                ));
                self.named_kind_slots.insert(name.clone(), ks);
            }
        }

        for (i, name) in params.iter().enumerate() {
            self.store_named(name, &format!("%arg_p{i}"));
            if let Some(ks) = self.named_kind_slots.get(name).cloned() {
                self.emit(&format!("store i64 %arg_k{i}, ptr {ks}, align 8"));
            }
            self.named_kinds.insert(name.clone(), ValueKind::Dynamic);
        }

        for (idx, instr) in body.iter().enumerate() {
            match instr {
                IrInstr::Label { block } => {
                    let bb = self.blocks[block].clone();
                    if !self.terminated {
                        self.emit(&format!("br label %{bb}"));
                    }
                    self.emit_raw(&format!("{bb}:"));
                    self.terminated = false;
                }
                _ if self.terminated => {}
                IrInstr::ConstI64 { dest, value } => {
                    self.store_val(*dest, &format!("{value}"));
                    self.value_kinds.insert(*dest, ValueKind::I64);
                }
                IrInstr::ConstF64 { dest, value } => {
                    let bits = value.to_bits() as i64;
                    self.store_val(*dest, &format!("{bits}"));
                    self.value_kinds.insert(*dest, ValueKind::F64);
                }
                IrInstr::ConstBool { dest, value } => {
                    self.store_val(*dest, if *value { "1" } else { "0" });
                    self.value_kinds.insert(*dest, ValueKind::Bool);
                }
                IrInstr::ConstNone { dest } => {
                    self.store_val(*dest, "0");
                    self.value_kinds.insert(*dest, ValueKind::None_);
                }
                IrInstr::ConstStr { dest, value } => {
                    let (gname, len) = string_globals
                        .get(value)
                        .cloned()
                        .ok_or_else(|| format!("llvm: missing string global for {value:?}"))?;
                    let p = self.tmp();
                    self.emit(&format!(
                        "{p} = getelementptr inbounds [{len} x i8], ptr {gname}, i64 0, i64 0"
                    ));
                    let v = self.tmp();
                    self.emit(&format!("{v} = ptrtoint ptr {p} to i64"));
                    self.store_val(*dest, &v);
                    self.value_kinds.insert(*dest, ValueKind::Str);
                }
                IrInstr::Load { dest, name } => {
                    let val = self.load_named(name);
                    self.store_val(*dest, &val);
                    let nk = self.named_kind(name);
                    self.value_kinds.insert(*dest, nk);
                    if nk == ValueKind::Dynamic {
                        if let Some(ks) = self.named_kind_slots.get(name).cloned() {
                            let k = self.tmp();
                            self.emit(&format!("{k} = load i64, ptr {ks}, align 8"));
                            self.store_kind_dyn(*dest, &k);
                        }
                    }
                }
                IrInstr::Store { name, value } => {
                    let val = self.load_val(*value);
                    self.store_named(name, &val);
                    let vk = self.kind_of(*value);
                    let merged = match self.named_kinds.get(name) {
                        Some(prev) if *prev != vk => ValueKind::Dynamic,
                        _ => vk,
                    };
                    self.named_kinds.insert(name.clone(), merged);
                    if let Some(ks) = self.named_kind_slots.get(name).cloned() {
                        let runtime_kind = if vk == ValueKind::Dynamic {
                            self.kind_operand(vk, *value)
                        } else {
                            format!("{}", vk.as_i64())
                        };
                        self.emit(&format!("store i64 {runtime_kind}, ptr {ks}, align 8"));
                    }
                }
                IrInstr::Unary { dest, op, src } => {
                    let s = self.load_val(*src);
                    let src_kind = self.kind_of(*src);
                    let (v, out_kind) = match op {
                        IrOp::Neg if src_kind == ValueKind::F64 => {
                            let f = self.i64_to_f64(&s);
                            let n = self.tmp();
                            self.emit(&format!("{n} = fneg double {f}"));
                            (self.f64_to_i64(&n), ValueKind::F64)
                        }
                        IrOp::Neg => {
                            let n = self.tmp();
                            self.emit(&format!("{n} = sub i64 0, {s}"));
                            (n, ValueKind::I64)
                        }
                        IrOp::Not => {
                            let c = self.tmp();
                            self.emit(&format!("{c} = icmp ne i64 {s}, 0"));
                            let x = self.tmp();
                            self.emit(&format!("{x} = xor i1 {c}, true"));
                            (self.bool_ext(&x), ValueKind::Bool)
                        }
                        other => {
                            return Err(format!("llvm codegen: unsupported unary op {other}"));
                        }
                    };
                    self.store_val(*dest, &v);
                    self.value_kinds.insert(*dest, out_kind);
                }
                IrInstr::IntWrap {
                    dest,
                    src,
                    bits,
                    signed,
                } => {
                    let s = self.load_val(*src);
                    let v = if *bits >= 64 {
                        s
                    } else if *signed {
                        let shift = 64 - *bits;
                        let left = self.tmp();
                        self.emit(&format!("{left} = shl i64 {s}, {shift}"));
                        let right = self.tmp();
                        self.emit(&format!("{right} = ashr i64 {left}, {shift}"));
                        right
                    } else {
                        let mask_bits = if *bits == 0 {
                            0i64
                        } else {
                            (1i64 << *bits).wrapping_sub(1)
                        };
                        let t = self.tmp();
                        self.emit(&format!("{t} = and i64 {s}, {mask_bits}"));
                        t
                    };
                    self.store_val(*dest, &v);
                    let out_kind = if !*signed && *bits == 64 {
                        ValueKind::U64
                    } else {
                        ValueKind::I64
                    };
                    self.value_kinds.insert(*dest, out_kind);
                }
                IrInstr::GuardDivisor { value, line } => {
                    let kind = self.kind_of(*value);
                    if kind != ValueKind::F64 {
                        let v = self.load_val(*value);
                        let is_zero = self.tmp();
                        self.emit(&format!("{is_zero} = icmp eq i64 {v}, 0"));
                        let mut cond = is_zero;
                        if kind == ValueKind::Dynamic {
                            let vk = self.kind_operand(kind, *value);
                            let not_float = self.tmp();
                            self.emit(&format!(
                                "{not_float} = icmp ne i64 {vk}, {}",
                                ValueKind::F64.as_i64()
                            ));
                            let anded = self.tmp();
                            self.emit(&format!("{anded} = and i1 {cond}, {not_float}"));
                            cond = anded;
                        }
                        let err_bb = format!("divz_err_{}", self.next_tmp);
                        let ok_bb = format!("divz_ok_{}", self.next_tmp);
                        self.next_tmp += 1;
                        self.emit(&format!("br i1 {cond}, label %{err_bb}, label %{ok_bb}"));
                        self.emit_raw(&format!("{err_bb}:"));
                        self.emit(&format!("call void @hyper_rt_div_by_zero(i64 {line})"));
                        self.emit(&format!("br label %{ok_bb}"));
                        self.emit_raw(&format!("{ok_bb}:"));
                    }
                }
                IrInstr::Binary {
                    dest,
                    op,
                    left,
                    right,
                } => {
                    let l = self.load_val(*left);
                    let r = self.load_val(*right);
                    let lk = self.kind_of(*left);
                    let rk = self.kind_of(*right);
                    let is_float = lk == ValueKind::F64 || rk == ValueKind::F64;

                    let (v, out_kind) = if lk == ValueKind::Str
                        && rk == ValueKind::Str
                        && matches!(op, IrOp::Add)
                    {
                        let (c_l, c_r) = concat_consume[idx];
                        let call = self.tmp();
                        self.emit(&format!(
                            "{call} = call i64 @hyper_rt_str_concat(i64 {l}, i64 {r}, i64 {}, i64 {})",
                            c_l as i64, c_r as i64
                        ));
                        (call, ValueKind::Str)
                    } else if matches!(op, IrOp::Eq | IrOp::Ne)
                        && (needs_runtime_eq(lk) || needs_runtime_eq(rk))
                    {
                        let lkind = self.kind_operand(lk, *left);
                        let rkind = self.kind_operand(rk, *right);
                        let eq = self.tmp();
                        self.emit(&format!(
                            "{eq} = call i64 @hyper_rt_value_eq(i64 {l}, i64 {lkind}, i64 {r}, i64 {rkind})"
                        ));
                        let v = if matches!(op, IrOp::Ne) {
                            let x = self.tmp();
                            self.emit(&format!("{x} = xor i64 {eq}, 1"));
                            x
                        } else {
                            eq
                        };
                        (v, ValueKind::Bool)
                    } else if is_float
                        && matches!(
                            op,
                            IrOp::Add
                                | IrOp::Sub
                                | IrOp::Mul
                                | IrOp::Div
                                | IrOp::FloorDiv
                                | IrOp::Pow
                        )
                    {
                        let lf = if lk == ValueKind::F64 {
                            self.i64_to_f64(&l)
                        } else {
                            self.sitofp(&l)
                        };
                        let rf = if rk == ValueKind::F64 {
                            self.i64_to_f64(&r)
                        } else {
                            self.sitofp(&r)
                        };
                        let fv = match op {
                            IrOp::Add => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = fadd double {lf}, {rf}"));
                                t
                            }
                            IrOp::Sub => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = fsub double {lf}, {rf}"));
                                t
                            }
                            IrOp::Mul => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = fmul double {lf}, {rf}"));
                                t
                            }
                            IrOp::Div => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = fdiv double {lf}, {rf}"));
                                t
                            }
                            IrOp::FloorDiv => {
                                let t = self.tmp();
                                self.emit(&format!(
                                    "{t} = call double @hyper_rt_floor_div_f64(double {lf}, double {rf})"
                                ));
                                t
                            }
                            IrOp::Pow => {
                                let t = self.tmp();
                                self.emit(&format!(
                                    "{t} = call double @hyper_rt_pow_f64(double {lf}, double {rf})"
                                ));
                                t
                            }
                            _ => unreachable!(),
                        };
                        (self.f64_to_i64(&fv), ValueKind::F64)
                    } else {
                        let v = match op {
                            IrOp::Add => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = add i64 {l}, {r}"));
                                t
                            }
                            IrOp::Sub => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = sub i64 {l}, {r}"));
                                t
                            }
                            IrOp::Mul => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = mul i64 {l}, {r}"));
                                t
                            }
                            IrOp::Div => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = sdiv i64 {l}, {r}"));
                                t
                            }
                            IrOp::FloorDiv => {
                                let t = self.tmp();
                                self.emit(&format!(
                                    "{t} = call i64 @hyper_rt_floor_div_i64(i64 {l}, i64 {r})"
                                ));
                                t
                            }
                            IrOp::Rem => {
                                let t = self.tmp();
                                self.emit(&format!("{t} = srem i64 {l}, {r}"));
                                t
                            }
                            IrOp::Pow => {
                                let t = self.tmp();
                                self.emit(&format!(
                                    "{t} = call i64 @hyper_rt_pow_i64(i64 {l}, i64 {r})"
                                ));
                                t
                            }
                            IrOp::Eq => {
                                let b = if is_float {
                                    let lf = self.i64_to_f64(&l);
                                    let rf = self.i64_to_f64(&r);
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = fcmp oeq double {lf}, {rf}"));
                                    c
                                } else {
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = icmp eq i64 {l}, {r}"));
                                    c
                                };
                                self.bool_ext(&b)
                            }
                            IrOp::Ne => {
                                let b = if is_float {
                                    let lf = self.i64_to_f64(&l);
                                    let rf = self.i64_to_f64(&r);
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = fcmp one double {lf}, {rf}"));
                                    c
                                } else {
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = icmp ne i64 {l}, {r}"));
                                    c
                                };
                                self.bool_ext(&b)
                            }
                            IrOp::Lt => {
                                let b = if is_float {
                                    let lf = self.i64_to_f64(&l);
                                    let rf = self.i64_to_f64(&r);
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = fcmp olt double {lf}, {rf}"));
                                    c
                                } else {
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = icmp slt i64 {l}, {r}"));
                                    c
                                };
                                self.bool_ext(&b)
                            }
                            IrOp::Le => {
                                let b = if is_float {
                                    let lf = self.i64_to_f64(&l);
                                    let rf = self.i64_to_f64(&r);
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = fcmp ole double {lf}, {rf}"));
                                    c
                                } else {
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = icmp sle i64 {l}, {r}"));
                                    c
                                };
                                self.bool_ext(&b)
                            }
                            IrOp::Gt => {
                                let b = if is_float {
                                    let lf = self.i64_to_f64(&l);
                                    let rf = self.i64_to_f64(&r);
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = fcmp ogt double {lf}, {rf}"));
                                    c
                                } else {
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = icmp sgt i64 {l}, {r}"));
                                    c
                                };
                                self.bool_ext(&b)
                            }
                            IrOp::Ge => {
                                let b = if is_float {
                                    let lf = self.i64_to_f64(&l);
                                    let rf = self.i64_to_f64(&r);
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = fcmp oge double {lf}, {rf}"));
                                    c
                                } else {
                                    let c = self.tmp();
                                    self.emit(&format!("{c} = icmp sge i64 {l}, {r}"));
                                    c
                                };
                                self.bool_ext(&b)
                            }
                            IrOp::Neg | IrOp::Not => {
                                return Err(format!("llvm codegen: {op} is unary, not binary"));
                            }
                        };
                        let out_kind = match op {
                            IrOp::Eq | IrOp::Ne | IrOp::Lt | IrOp::Le | IrOp::Gt | IrOp::Ge => {
                                ValueKind::Bool
                            }
                            _ => ValueKind::I64,
                        };
                        (v, out_kind)
                    };
                    self.store_val(*dest, &v);
                    self.value_kinds.insert(*dest, out_kind);
                }
                IrInstr::Call { dest, func, args } => {
                    self.emit_call(*dest, func, args)?;
                }
                IrInstr::MakeList { dest, items } => {
                    let list = self.tmp();
                    self.emit(&format!("{list} = call i64 @hyper_rt_list_new()"));
                    self.store_val(*dest, &list);
                    self.value_kinds.insert(*dest, ValueKind::List);
                    for item in items {
                        let val = self.load_val(*item);
                        let kind = self.kind_operand(self.kind_of(*item), *item);
                        self.emit(&format!(
                            "call void @hyper_rt_list_push(i64 {list}, i64 {val}, i64 {kind})"
                        ));
                    }
                }
                IrInstr::MakeDict { dest, entries } => {
                    let dict = self.tmp();
                    self.emit(&format!("{dict} = call i64 @hyper_rt_dict_new()"));
                    self.store_val(*dest, &dict);
                    self.value_kinds.insert(*dest, ValueKind::Dict);
                    for (k, v) in entries {
                        let key = self.load_val(*k);
                        let key_kind = self.kind_operand(self.kind_of(*k), *k);
                        let val = self.load_val(*v);
                        let val_kind = self.kind_operand(self.kind_of(*v), *v);
                        self.emit(&format!(
                            "call void @hyper_rt_dict_push(i64 {dict}, i64 {key}, i64 {key_kind}, i64 {val}, i64 {val_kind})"
                        ));
                    }
                }
                IrInstr::IndexGet {
                    dest,
                    object,
                    index,
                } => {
                    let obj = self.load_val(*object);
                    let idx = self.load_val(*index);
                    let kind_ptr = self.out_kind_alloca();
                    let kp = self.tmp();
                    self.emit(&format!("{kp} = ptrtoint ptr {kind_ptr} to i64"));
                    let payload = match self.kind_of(*object) {
                        ValueKind::Dict => {
                            let key_kind = self.kind_operand(self.kind_of(*index), *index);
                            let t = self.tmp();
                            self.emit(&format!(
                                "{t} = call i64 @hyper_rt_dict_get(i64 {obj}, i64 {idx}, i64 {key_kind}, i64 {kp})"
                            ));
                            t
                        }
                        ValueKind::Dynamic => {
                            let obj_kind = self.kind_operand(ValueKind::Dynamic, *object);
                            let key_kind = self.kind_operand(self.kind_of(*index), *index);
                            let t = self.tmp();
                            self.emit(&format!(
                                "{t} = call i64 @hyper_rt_index_get(i64 {obj}, i64 {obj_kind}, i64 {idx}, i64 {key_kind}, i64 {kp})"
                            ));
                            t
                        }
                        _ => {
                            let t = self.tmp();
                            self.emit(&format!(
                                "{t} = call i64 @hyper_rt_list_get(i64 {obj}, i64 {idx}, i64 {kp})"
                            ));
                            t
                        }
                    };
                    self.store_val(*dest, &payload);
                    let kind_val = self.tmp();
                    self.emit(&format!("{kind_val} = load i64, ptr {kind_ptr}, align 8"));
                    self.store_kind_dyn(*dest, &kind_val);
                    self.value_kinds.insert(*dest, ValueKind::Dynamic);
                }
                IrInstr::IndexSet {
                    object,
                    index,
                    value,
                } => {
                    let obj = self.load_val(*object);
                    let idx = self.load_val(*index);
                    let val = self.load_val(*value);
                    let val_kind = self.kind_operand(self.kind_of(*value), *value);
                    match self.kind_of(*object) {
                        ValueKind::Dict => {
                            let key_kind = self.kind_operand(self.kind_of(*index), *index);
                            self.emit(&format!(
                                "call void @hyper_rt_dict_set(i64 {obj}, i64 {idx}, i64 {key_kind}, i64 {val}, i64 {val_kind})"
                            ));
                        }
                        ValueKind::Dynamic => {
                            let obj_kind = self.kind_operand(ValueKind::Dynamic, *object);
                            let key_kind = self.kind_operand(self.kind_of(*index), *index);
                            self.emit(&format!(
                                "call void @hyper_rt_index_set(i64 {obj}, i64 {obj_kind}, i64 {idx}, i64 {key_kind}, i64 {val}, i64 {val_kind})"
                            ));
                        }
                        _ => {
                            self.emit(&format!(
                                "call void @hyper_rt_list_set(i64 {obj}, i64 {idx}, i64 {val}, i64 {val_kind})"
                            ));
                        }
                    }
                }
                IrInstr::ListLen { dest, list } => {
                    let l = self.load_val(*list);
                    let t = self.tmp();
                    self.emit(&format!("{t} = call i64 @hyper_rt_list_len(i64 {l})"));
                    self.store_val(*dest, &t);
                    self.value_kinds.insert(*dest, ValueKind::I64);
                }
                IrInstr::ValueToStr { dest, src } => {
                    let src_kind = self.kind_of(*src);
                    if src_kind == ValueKind::Str {
                        let v = self.load_val(*src);
                        self.store_val(*dest, &v);
                        self.value_kinds.insert(*dest, ValueKind::Str);
                    } else {
                        let v = self.load_val(*src);
                        let kind = self.kind_operand(src_kind, *src);
                        let t = self.tmp();
                        self.emit(&format!(
                            "{t} = call i64 @hyper_rt_value_to_str(i64 {v}, i64 {kind})"
                        ));
                        self.store_val(*dest, &t);
                        self.value_kinds.insert(*dest, ValueKind::Str);
                    }
                }
                IrInstr::StrConcat { dest, left, right } => {
                    let l = self.load_val(*left);
                    let r = self.load_val(*right);
                    let (c_l, c_r) = concat_consume[idx];
                    let t = self.tmp();
                    self.emit(&format!(
                        "{t} = call i64 @hyper_rt_str_concat(i64 {l}, i64 {r}, i64 {}, i64 {})",
                        c_l as i64, c_r as i64
                    ));
                    self.store_val(*dest, &t);
                    self.value_kinds.insert(*dest, ValueKind::Str);
                }
                IrInstr::MakeStruct { dest, nfields } => {
                    let t = self.tmp();
                    self.emit(&format!(
                        "{t} = call i64 @hyper_rt_struct_new(i64 {nfields})"
                    ));
                    self.store_val(*dest, &t);
                    self.value_kinds.insert(*dest, ValueKind::Struct);
                }
                IrInstr::StructGet {
                    dest,
                    object,
                    field,
                } => {
                    let obj = self.load_val(*object);
                    let kind_ptr = self.out_kind_alloca();
                    let kp = self.tmp();
                    self.emit(&format!("{kp} = ptrtoint ptr {kind_ptr} to i64"));
                    let t = self.tmp();
                    self.emit(&format!(
                        "{t} = call i64 @hyper_rt_struct_get(i64 {obj}, i64 {field}, i64 {kp})"
                    ));
                    self.store_val(*dest, &t);
                    let kind_val = self.tmp();
                    self.emit(&format!("{kind_val} = load i64, ptr {kind_ptr}, align 8"));
                    self.store_kind_dyn(*dest, &kind_val);
                    self.value_kinds.insert(*dest, ValueKind::Dynamic);
                }
                IrInstr::StructSet {
                    object,
                    field,
                    value,
                } => {
                    let obj = self.load_val(*object);
                    let val = self.load_val(*value);
                    let val_kind = self.kind_operand(self.kind_of(*value), *value);
                    self.emit(&format!(
                        "call void @hyper_rt_struct_set(i64 {obj}, i64 {field}, i64 {val}, i64 {val_kind})"
                    ));
                }
                IrInstr::Print { args } => {
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            self.emit("call void @hyper_rt_print_separator()");
                        }
                        let v = self.load_val(*a);
                        match self.kind_of(*a) {
                            ValueKind::Dynamic => {
                                let k = self.kind_operand(ValueKind::Dynamic, *a);
                                self.emit(&format!(
                                    "call void @hyper_rt_print_value(i64 {v}, i64 {k})"
                                ));
                            }
                            ValueKind::F64 => {
                                let f = self.i64_to_f64(&v);
                                self.emit(&format!("call void @hyper_rt_print_f64(double {f})"));
                            }
                            ValueKind::Str => {
                                self.emit(&format!("call void @hyper_rt_print_str(i64 {v})"));
                            }
                            ValueKind::List => {
                                self.emit(&format!("call void @hyper_rt_print_list(i64 {v})"));
                            }
                            ValueKind::Dict => {
                                self.emit(&format!("call void @hyper_rt_print_dict(i64 {v})"));
                            }
                            ValueKind::Struct => {
                                self.emit(&format!("call void @hyper_rt_print_struct(i64 {v})"));
                            }
                            ValueKind::I64 => {
                                self.emit(&format!("call void @hyper_rt_print_i64(i64 {v})"));
                            }
                            kind => {
                                self.emit(&format!(
                                    "call void @hyper_rt_print_value(i64 {v}, i64 {})",
                                    kind.as_i64()
                                ));
                            }
                        }
                    }
                    self.emit("call void @hyper_rt_print_newline()");
                }
                IrInstr::Return { value } => {
                    let v = match value {
                        Some(id) => self.load_val(*id),
                        None => "0".to_string(),
                    };
                    if self.returns_kind {
                        let k = match value {
                            Some(id) => self.kind_operand(self.kind_of(*id), *id),
                            None => format!("{}", ValueKind::None_.as_i64()),
                        };
                        self.emit_ret(&v, Some(&k));
                    } else {
                        self.emit_ret(&v, None);
                    }
                }
                IrInstr::Jump { target } => {
                    let bb = self.blocks[target].clone();
                    self.emit(&format!("br label %{bb}"));
                    self.terminated = true;
                }
                IrInstr::Branch {
                    cond,
                    then_block,
                    else_block,
                } => {
                    let c = self.load_val(*cond);
                    let is_true = self.tmp();
                    self.emit(&format!("{is_true} = icmp ne i64 {c}, 0"));
                    let t = self.blocks[then_block].clone();
                    let e = self.blocks[else_block].clone();
                    self.emit(&format!("br i1 {is_true}, label %{t}, label %{e}"));
                    self.terminated = true;
                }
                IrInstr::ParallelRange {
                    start,
                    end,
                    worker,
                } => {
                    let s = self.load_val(*start);
                    let e = self.load_val(*end);
                    let w = sanitize_name(worker);
                    self.emit(&format!(
                        "call void @hyper_rt_parallel_for(i64 {s}, i64 {e}, {{i64, i64}} (i64, i64)* @{w})"
                    ));
                }
            }
        }

        if !self.terminated {
            if self.returns_kind {
                self.emit_ret("0", Some(&format!("{}", ValueKind::None_.as_i64())));
            } else {
                self.emit_ret("0", None);
            }
        }

        self.emit_raw("}");
        self.emit_raw("");
        Ok(())
    }

    fn emit_call(&mut self, dest: ValueId, func: &str, args: &[ValueId]) -> Result<(), String> {
        let mut arg_parts: Vec<String> = Vec::new();
        for a in args {
            let v = self.load_val(*a);
            let k = self.kind_operand(self.kind_of(*a), *a);
            arg_parts.push(format!("i64 {v}"));
            arg_parts.push(format!("i64 {k}"));
        }

        if let Some((out_kind, uses_out_kind)) = classify_runtime(func) {
            if uses_out_kind {
                let kind_ptr = self.out_kind_alloca();
                let kp = self.tmp();
                self.emit(&format!("{kp} = ptrtoint ptr {kind_ptr} to i64"));
                arg_parts.push(format!("i64 {kp}"));
                let call_args = arg_parts.join(", ");
                let t = self.tmp();
                self.emit(&format!("{t} = call i64 @{func}({call_args})"));
                self.store_val(dest, &t);
                let kind_val = self.tmp();
                self.emit(&format!("{kind_val} = load i64, ptr {kind_ptr}, align 8"));
                self.store_kind_dyn(dest, &kind_val);
                self.value_kinds.insert(dest, ValueKind::Dynamic);
            } else if out_kind == ValueKind::None_ {
                let call_args = arg_parts.join(", ");
                self.emit(&format!("call void @{func}({call_args})"));
                self.store_val(dest, "0");
                self.value_kinds.insert(dest, ValueKind::None_);
            } else {
                let call_args = arg_parts.join(", ");
                let t = self.tmp();
                self.emit(&format!("{t} = call i64 @{func}({call_args})"));
                self.store_val(dest, &t);
                self.value_kinds.insert(dest, out_kind);
            }
        } else if self.user_funcs.contains(func) {
            let call_args = arg_parts.join(", ");
            let t = self.tmp();
            self.emit(&format!("{t} = call {{ i64, i64 }} @{func}({call_args})"));
            let payload = self.tmp();
            self.emit(&format!("{payload} = extractvalue {{ i64, i64 }} {t}, 0"));
            let kind = self.tmp();
            self.emit(&format!("{kind} = extractvalue {{ i64, i64 }} {t}, 1"));
            self.store_val(dest, &payload);
            self.store_kind_dyn(dest, &kind);
            self.value_kinds.insert(dest, ValueKind::Dynamic);
        } else {
            return Err(format!("llvm codegen: undefined function '{func}'"));
        }
        Ok(())
    }
}

fn collect_strings(module: &IrModule) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut visit = |instrs: &[IrInstr]| {
        for instr in instrs {
            if let IrInstr::ConstStr { value, .. } = instr {
                if seen.insert(value.clone()) {
                    out.push(value.clone());
                }
            }
        }
    };
    for f in &module.functions {
        visit(&f.body);
    }
    visit(&module.main);
    out
}

/// Emit LLVM IR text for a Hyper IR module.
pub fn emit_llvm_ir(module: &IrModule) -> Result<String, String> {
    let mut ir = String::new();
    ir.push_str(&format!("target triple = \"{}\"\n\n", host_triple()));
    ir.push_str(runtime_extern_decls());
    ir.push('\n');

    let strings = collect_strings(module);
    let mut string_globals: HashMap<String, (String, usize)> = HashMap::new();
    for (i, s) in strings.iter().enumerate() {
        let gname = format!("@.hyper_str.{i}");
        let len = s.len() + 1;
        let escaped = escape_llvm_string(s);
        ir.push_str(&format!(
            "{gname} = private unnamed_addr constant [{len} x i8] c\"{escaped}\", align 1\n"
        ));
        string_globals.insert(s.clone(), (gname, len));
    }
    if !strings.is_empty() {
        ir.push('\n');
    }

    let user_funcs: HashSet<String> = module.functions.iter().map(|f| f.name.clone()).collect();

    for func in &module.functions {
        let mut fe = FuncEmitter::new(true, user_funcs.clone());
        fe.emit_function(&func.name, &func.params, &func.body, &string_globals)?;
        ir.push_str(&fe.body);
    }

    let mut main_fe = FuncEmitter::new(false, user_funcs);
    main_fe.emit_function("__main__", &[], &module.main, &string_globals)?;
    ir.push_str(&main_fe.body);

    Ok(ir)
}

fn find_clang() -> Result<String, String> {
    if let Ok(cc) = std::env::var("CC") {
        let trimmed = cc.trim();
        if !trimmed.is_empty() {
            let name = Path::new(trimmed)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(trimmed)
                .to_ascii_lowercase();
            if name.contains("clang") {
                if Command::new(trimmed)
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success() || !o.stdout.is_empty())
                    .unwrap_or(false)
                {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }
    for cand in ["clang", "clang-cl"] {
        if Command::new(cand)
            .arg("--version")
            .output()
            .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
            .unwrap_or(false)
        {
            return Ok(cand.to_string());
        }
    }
    Err(
        "clang is required for the LLVM codegen backend (HYPER_CODEGEN=llvm). \
         Install LLVM/clang or set CC to a clang driver, or use HYPER_CODEGEN=cranelift / --backend cranelift"
            .to_string(),
    )
}

/// Write LLVM IR to a temp file and link with clang + Hyper C runtime into an executable.
pub fn emit_exe_llvm(module: &IrModule, out_path: &str) -> Result<(), String> {
    let ll = emit_llvm_ir(module)?;
    let tmp_dir = std::env::temp_dir();
    let ll_path = tmp_dir.join(format!("hyper_{}.ll", std::process::id()));
    std::fs::write(&ll_path, &ll).map_err(|e| format!("failed to write LLVM IR: {e}"))?;

    let clang = find_clang()?;
    let out = normalize_exe_path(out_path);
    let rt = runtime_c_path()?;
    let rt_file = runtime_file_c_path()?;
    let rt_json = runtime_json_c_path()?;
    let rt_mmap = runtime_mmap_c_path()?;
    let rt_io = runtime_io_c_path()?;
    let rt_str = runtime_str_c_path()?;
    let rt_builtins = runtime_builtins_c_path()?;

    let is_clang_cl = Path::new(&clang)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .eq_ignore_ascii_case("clang-cl");

    let status = if is_clang_cl {
        let fo_dir = tmp_dir.join(format!("hyper_ll_link_{}", std::process::id()));
        std::fs::create_dir_all(&fo_dir)
            .map_err(|e| format!("failed to create temp link dir: {e}"))?;
        let mut fo = fo_dir.to_string_lossy().into_owned();
        if !fo.ends_with('\\') && !fo.ends_with('/') {
            fo.push('\\');
        }
        let status = Command::new(&clang)
            .arg("/nologo")
            .arg("/O2")
            .arg("/D_CRT_SECURE_NO_WARNINGS")
            .arg(format!("/Fo{fo}"))
            .arg(format!("/Fe:{out}"))
            .arg(ll_path.as_os_str())
            .arg(rt.as_os_str())
            .arg(rt_file.as_os_str())
            .arg(rt_json.as_os_str())
            .arg(rt_mmap.as_os_str())
            .arg(rt_io.as_os_str())
            .arg(rt_str.as_os_str())
            .arg(rt_builtins.as_os_str())
            .status()
            .map_err(|e| format!("failed to invoke {clang}: {e}"))?;
        let _ = std::fs::remove_dir_all(&fo_dir);
        status
    } else {
        let mut cmd = Command::new(&clang);
        cmd.arg("-O2")
            .arg(ll_path.as_os_str())
            .arg(rt.as_os_str())
            .arg(rt_file.as_os_str())
            .arg(rt_json.as_os_str())
            .arg(rt_mmap.as_os_str())
            .arg(rt_io.as_os_str())
            .arg(rt_str.as_os_str())
            .arg(rt_builtins.as_os_str())
            .arg("-o")
            .arg(&out);
        if !cfg!(windows) {
            cmd.arg("-lm");
            cmd.arg("-pthread");
        }
        cmd.status()
            .map_err(|e| format!("failed to invoke {clang}: {e}"))?
    };

    let _ = std::fs::remove_file(&ll_path);

    if !status.success() {
        return Err(format!("{clang} failed with status {status}"));
    }
    Ok(())
}
