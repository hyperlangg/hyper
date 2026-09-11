use super::ir::{BlockId, IrInstr, IrModule, IrOp, ValueId};
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, MemFlags, StackSlotData, StackSlotKind, UserFuncName, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{default_libcall_names, DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Production AOT uses LLVM IR + clang; Cranelift remains for `--emit-obj` and opt-in AOT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodegenBackend {
    Llvm,
    Cranelift,
}

/// Resolve backend from `HYPER_CODEGEN` (`llvm` | `cranelift`). Default: **llvm**.
pub fn default_backend() -> CodegenBackend {
    match std::env::var("HYPER_CODEGEN") {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "cranelift" | "clif" => CodegenBackend::Cranelift,
            "llvm" | "" => CodegenBackend::Llvm,
            other => {
                eprintln!(
                    "warning: unknown HYPER_CODEGEN={other:?}, using llvm (try llvm|cranelift)"
                );
                CodegenBackend::Llvm
            }
        },
        Err(_) => CodegenBackend::Llvm,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    I64,
    U64,
    F64,
    Str,
    Bool,
    None_,
    List,
    Dict,
    Struct,
    File,
    Mmap,
    /// Element kind known only at runtime (e.g. after index_get).
    Dynamic,
}

impl ValueKind {
    pub(crate) fn as_i64(self) -> i64 {
        match self {
            ValueKind::I64 => 0,
            ValueKind::F64 => 1,
            ValueKind::Str => 2,
            ValueKind::Bool => 3,
            ValueKind::None_ => 4,
            ValueKind::List => 5,
            ValueKind::Dict => 6,
            ValueKind::Struct => 7,
            ValueKind::File => 8,
            ValueKind::Mmap => 9,
            ValueKind::U64 => 10,
            ValueKind::Dynamic => 0,
        }
    }
}

struct StringData {
    next: usize,
}

impl StringData {
    fn new() -> Self {
        StringData { next: 0 }
    }

    fn define<M: Module>(&mut self, module: &mut M, s: &str) -> Result<DataId, String> {
        let name = format!(".hyper_str.{}", self.next);
        self.next += 1;
        let id = module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| e.to_string())?;
        let mut desc = DataDescription::new();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        desc.define(bytes.into_boxed_slice());
        module.define_data(id, &desc).map_err(|e| e.to_string())?;
        Ok(id)
    }
}

struct RuntimeIds {
    print_i64: FuncId,
    print_f64: FuncId,
    print_str: FuncId,
    print_nl: FuncId,
    print_sep: FuncId,
    print_list: FuncId,
    print_dict: FuncId,
    print_value: FuncId,
    pow_i64: FuncId,
    pow_f64: FuncId,
    floor_div_i64: FuncId,
    floor_div_f64: FuncId,
    list_new: FuncId,
    list_push: FuncId,
    list_get: FuncId,
    list_set: FuncId,
    list_len: FuncId,
    dict_new: FuncId,
    dict_push: FuncId,
    dict_get: FuncId,
    dict_set: FuncId,
    index_get: FuncId,
    index_set: FuncId,
    value_to_str: FuncId,
    value_eq: FuncId,
    div_by_zero: FuncId,
    str_concat: FuncId,
    struct_new: FuncId,
    struct_get: FuncId,
    struct_set: FuncId,
    print_struct: FuncId,
    file_open: FuncId,
    file_close: FuncId,
    file_read_all: FuncId,
    file_read_n: FuncId,
    file_readline: FuncId,
    file_readlines: FuncId,
    file_write: FuncId,
    file_writelines: FuncId,
    file_seek: FuncId,
    file_tell: FuncId,
    file_size: FuncId,
    file_flush: FuncId,
    file_is_closed: FuncId,
    file_path: FuncId,
    file_mode: FuncId,
    mmap_open: FuncId,
    mmap_close: FuncId,
    mmap_read_chunk: FuncId,
    input_fn: FuncId,
    clock_fn: FuncId,
    coll_len: FuncId,
    coll_append: FuncId,
    coll_keys: FuncId,
    builtin_len: FuncId,
    builtin_abs: FuncId,
    builtin_min: FuncId,
    builtin_max: FuncId,
    builtin_sum: FuncId,
    builtin_round: FuncId,
    builtin_pow: FuncId,
    builtin_divmod: FuncId,
    builtin_chr: FuncId,
    builtin_ord: FuncId,
    builtin_bin: FuncId,
    builtin_hex: FuncId,
    builtin_oct: FuncId,
    builtin_int: FuncId,
    builtin_float: FuncId,
    builtin_str: FuncId,
    builtin_bool: FuncId,
    builtin_all: FuncId,
    builtin_any: FuncId,
    builtin_sorted: FuncId,
    builtin_reversed: FuncId,
    builtin_enumerate: FuncId,
    builtin_zip: FuncId,
    builtin_list: FuncId,
    builtin_range: FuncId,
    builtin_repr: FuncId,
    str_upper: FuncId,
    str_lower: FuncId,
    str_capitalize: FuncId,
    str_title: FuncId,
    str_swapcase: FuncId,
    str_strip: FuncId,
    str_lstrip: FuncId,
    str_rstrip: FuncId,
    str_startswith: FuncId,
    str_endswith: FuncId,
    str_split: FuncId,
    str_rsplit: FuncId,
    str_replace: FuncId,
    str_join: FuncId,
    str_find: FuncId,
    str_rfind: FuncId,
    str_index: FuncId,
    str_rindex: FuncId,
    str_count: FuncId,
    str_isdigit: FuncId,
    str_isalpha: FuncId,
    str_isalnum: FuncId,
    str_isspace: FuncId,
    str_islower: FuncId,
    str_isupper: FuncId,
    str_istitle: FuncId,
    str_isascii: FuncId,
    str_center: FuncId,
    str_ljust: FuncId,
    str_rjust: FuncId,
    str_zfill: FuncId,
    str_removeprefix: FuncId,
    str_removesuffix: FuncId,
    str_partition: FuncId,
    str_rpartition: FuncId,
    json_loads: FuncId,
    json_dumps: FuncId,
    json_load: FuncId,
    json_dump: FuncId,
    handle_enter: FuncId,
    handle_leave: FuncId,
    raise_fn: FuncId,
    parallel_for: FuncId,
}

fn make_flags(is_pic: bool) -> Result<settings::Flags, String> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("use_colocated_libcalls", "false")
        .map_err(|e| e.to_string())?;
    flag_builder
        .set("is_pic", if is_pic { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    Ok(settings::Flags::new(flag_builder))
}

fn declare_runtime<M: Module>(module: &mut M) -> Result<RuntimeIds, String> {
    let print_i64 = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_print_i64", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_f64 = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        module
            .declare_function("hyper_rt_print_f64", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_str = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_print_str", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_nl = {
        let sig = module.make_signature();
        module
            .declare_function("hyper_rt_print_newline", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_sep = {
        let sig = module.make_signature();
        module
            .declare_function("hyper_rt_print_separator", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_list = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_print_list", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_dict = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_print_dict", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let pow_i64 = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_pow_i64", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let pow_f64 = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        module
            .declare_function("hyper_rt_pow_f64", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let floor_div_i64 = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_floor_div_i64", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let floor_div_f64 = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        module
            .declare_function("hyper_rt_floor_div_f64", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let list_new = {
        let mut sig = module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_list_new", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let list_push = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_list_push", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let dict_new = {
        let mut sig = module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_dict_new", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let dict_push = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_dict_push", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_value = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_print_value", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let list_get = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_list_get", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let list_set = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_list_set", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let dict_get = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_dict_get", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let dict_set = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_dict_set", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let index_get = {
        let mut sig = module.make_signature();
        for _ in 0..5 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_index_get", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let index_set = {
        let mut sig = module.make_signature();
        for _ in 0..6 {
            sig.params.push(AbiParam::new(types::I64));
        }
        module
            .declare_function("hyper_rt_index_set", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let list_len = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_list_len", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let value_to_str = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_value_to_str", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let value_eq = {
        let mut sig = module.make_signature();
        for _ in 0..4 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_value_eq", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let div_by_zero = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_div_by_zero", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let str_concat = {
        let mut sig = module.make_signature();
        for _ in 0..4 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_str_concat", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let struct_new = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_struct_new", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let struct_get = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_struct_get", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let struct_set = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_struct_set", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let print_struct = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        module
            .declare_function("hyper_rt_print_struct", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    let mut declare_file = |name: &str, params: usize, returns: usize| -> Result<FuncId, String> {
        let mut sig = module.make_signature();
        for _ in 0..params {
            sig.params.push(AbiParam::new(types::I64));
        }
        for _ in 0..returns {
            sig.returns.push(AbiParam::new(types::I64));
        }
        module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| e.to_string())
    };
    let file_open = declare_file("hyper_rt_file_open", 6, 1)?;
    let file_close = declare_file("hyper_rt_file_close", 4, 0)?;
    let file_read_all = declare_file("hyper_rt_file_read_all", 4, 1)?;
    let file_read_n = declare_file("hyper_rt_file_read_n", 6, 1)?;
    let file_readline = declare_file("hyper_rt_file_readline", 5, 1)?;
    let file_readlines = declare_file("hyper_rt_file_readlines", 4, 1)?;
    let file_write = declare_file("hyper_rt_file_write", 6, 1)?;
    let file_writelines = declare_file("hyper_rt_file_writelines", 6, 1)?;
    let file_seek = declare_file("hyper_rt_file_seek", 8, 1)?;
    let file_tell = declare_file("hyper_rt_file_tell", 4, 1)?;
    let file_size = declare_file("hyper_rt_file_size", 4, 1)?;
    let file_flush = declare_file("hyper_rt_file_flush", 4, 0)?;
    let file_is_closed = declare_file("hyper_rt_file_is_closed", 2, 1)?;
    let file_path = declare_file("hyper_rt_file_path", 4, 1)?;
    let file_mode = declare_file("hyper_rt_file_mode", 4, 1)?;
    let mmap_open = declare_file("hyper_rt_mmap_open", 4, 1)?;
    let mmap_close = declare_file("hyper_rt_mmap_close", 4, 0)?;
    let mmap_read_chunk = declare_file("hyper_rt_mmap_read_chunk", 8, 1)?;
    let input_fn = declare_file("hyper_rt_input", 4, 1)?;
    let clock_fn = declare_file("hyper_rt_clock", 0, 1)?;
    let coll_len = declare_file("hyper_rt_coll_len", 4, 1)?;
    let coll_append = declare_file("hyper_rt_coll_append", 6, 0)?;
    let coll_keys = declare_file("hyper_rt_coll_keys", 4, 1)?;
    let builtin_len = declare_file("hyper_rt_builtin_len", 4, 1)?;
    let builtin_abs = declare_file("hyper_rt_builtin_abs", 5, 1)?;
    let builtin_min = declare_file("hyper_rt_builtin_min", 5, 1)?;
    let builtin_max = declare_file("hyper_rt_builtin_max", 5, 1)?;
    let builtin_sum = declare_file("hyper_rt_builtin_sum", 5, 1)?;
    let builtin_round = declare_file("hyper_rt_builtin_round", 7, 1)?;
    let builtin_pow = declare_file("hyper_rt_builtin_pow", 7, 1)?;
    let builtin_divmod = declare_file("hyper_rt_builtin_divmod", 6, 1)?;
    let builtin_chr = declare_file("hyper_rt_builtin_chr", 4, 1)?;
    let builtin_ord = declare_file("hyper_rt_builtin_ord", 4, 1)?;
    let builtin_bin = declare_file("hyper_rt_builtin_bin", 4, 1)?;
    let builtin_hex = declare_file("hyper_rt_builtin_hex", 4, 1)?;
    let builtin_oct = declare_file("hyper_rt_builtin_oct", 4, 1)?;
    let builtin_int = declare_file("hyper_rt_builtin_int", 5, 1)?;
    let builtin_float = declare_file("hyper_rt_builtin_float", 5, 1)?;
    let builtin_str = declare_file("hyper_rt_builtin_str", 4, 1)?;
    let builtin_bool = declare_file("hyper_rt_builtin_bool", 4, 1)?;
    let builtin_all = declare_file("hyper_rt_builtin_all", 4, 1)?;
    let builtin_any = declare_file("hyper_rt_builtin_any", 4, 1)?;
    let builtin_sorted = declare_file("hyper_rt_builtin_sorted", 4, 1)?;
    let builtin_reversed = declare_file("hyper_rt_builtin_reversed", 4, 1)?;
    let builtin_enumerate = declare_file("hyper_rt_builtin_enumerate", 6, 1)?;
    let builtin_zip = declare_file("hyper_rt_builtin_zip", 4, 1)?;
    let builtin_list = declare_file("hyper_rt_builtin_list", 4, 1)?;
    let builtin_range = declare_file("hyper_rt_builtin_range", 4, 1)?;
    let builtin_repr = declare_file("hyper_rt_builtin_repr", 4, 1)?;
    let str_upper = declare_file("hyper_rt_str_upper", 4, 1)?;
    let str_lower = declare_file("hyper_rt_str_lower", 4, 1)?;
    let str_capitalize = declare_file("hyper_rt_str_capitalize", 4, 1)?;
    let str_title = declare_file("hyper_rt_str_title", 4, 1)?;
    let str_swapcase = declare_file("hyper_rt_str_swapcase", 4, 1)?;
    let str_strip = declare_file("hyper_rt_str_strip", 4, 1)?;
    let str_lstrip = declare_file("hyper_rt_str_lstrip", 4, 1)?;
    let str_rstrip = declare_file("hyper_rt_str_rstrip", 4, 1)?;
    let str_startswith = declare_file("hyper_rt_str_startswith", 6, 1)?;
    let str_endswith = declare_file("hyper_rt_str_endswith", 6, 1)?;
    let str_split = declare_file("hyper_rt_str_split", 6, 1)?;
    let str_rsplit = declare_file("hyper_rt_str_rsplit", 6, 1)?;
    let str_replace = declare_file("hyper_rt_str_replace", 8, 1)?;
    let str_join = declare_file("hyper_rt_str_join", 6, 1)?;
    let str_find = declare_file("hyper_rt_str_find", 6, 1)?;
    let str_rfind = declare_file("hyper_rt_str_rfind", 6, 1)?;
    let str_index = declare_file("hyper_rt_str_index", 6, 1)?;
    let str_rindex = declare_file("hyper_rt_str_rindex", 6, 1)?;
    let str_count = declare_file("hyper_rt_str_count", 6, 1)?;
    let str_isdigit = declare_file("hyper_rt_str_isdigit", 4, 1)?;
    let str_isalpha = declare_file("hyper_rt_str_isalpha", 4, 1)?;
    let str_isalnum = declare_file("hyper_rt_str_isalnum", 4, 1)?;
    let str_isspace = declare_file("hyper_rt_str_isspace", 4, 1)?;
    let str_islower = declare_file("hyper_rt_str_islower", 4, 1)?;
    let str_isupper = declare_file("hyper_rt_str_isupper", 4, 1)?;
    let str_istitle = declare_file("hyper_rt_str_istitle", 4, 1)?;
    let str_isascii = declare_file("hyper_rt_str_isascii", 4, 1)?;
    let str_center = declare_file("hyper_rt_str_center", 8, 1)?;
    let str_ljust = declare_file("hyper_rt_str_ljust", 8, 1)?;
    let str_rjust = declare_file("hyper_rt_str_rjust", 8, 1)?;
    let str_zfill = declare_file("hyper_rt_str_zfill", 6, 1)?;
    let str_removeprefix = declare_file("hyper_rt_str_removeprefix", 6, 1)?;
    let str_removesuffix = declare_file("hyper_rt_str_removesuffix", 6, 1)?;
    let str_partition = declare_file("hyper_rt_str_partition", 6, 1)?;
    let str_rpartition = declare_file("hyper_rt_str_rpartition", 6, 1)?;
    let json_loads = declare_file("hyper_rt_json_loads", 5, 1)?;
    let json_dumps = declare_file("hyper_rt_json_dumps", 6, 1)?;
    let json_load = declare_file("hyper_rt_json_load", 5, 1)?;
    let json_dump = declare_file("hyper_rt_json_dump", 8, 1)?;
    let handle_enter = declare_file("hyper_rt_handle_enter", 0, 1)?;
    let handle_leave = declare_file("hyper_rt_handle_leave", 0, 1)?;
    let raise_fn = declare_file("hyper_rt_raise", 4, 1)?;
    let parallel_for = {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // start
        sig.params.push(AbiParam::new(types::I64)); // end
        sig.params.push(AbiParam::new(types::I64)); // worker fn ptr
        module
            .declare_function("hyper_rt_parallel_for", Linkage::Import, &sig)
            .map_err(|e| e.to_string())?
    };
    Ok(RuntimeIds {
        print_i64,
        print_f64,
        print_str,
        print_nl,
        print_sep,
        print_list,
        print_dict,
        print_value,
        pow_i64,
        pow_f64,
        floor_div_i64,
        floor_div_f64,
        list_new,
        list_push,
        list_get,
        list_set,
        list_len,
        dict_new,
        dict_push,
        dict_get,
        dict_set,
        index_get,
        index_set,
        value_to_str,
        value_eq,
        div_by_zero,
        str_concat,
        struct_new,
        struct_get,
        struct_set,
        print_struct,
        file_open,
        file_close,
        file_read_all,
        file_read_n,
        file_readline,
        file_readlines,
        file_write,
        file_writelines,
        file_seek,
        file_tell,
        file_size,
        file_flush,
        file_is_closed,
        file_path,
        file_mode,
        mmap_open,
        mmap_close,
        mmap_read_chunk,
        input_fn,
        clock_fn,
        coll_len,
        coll_append,
        coll_keys,
        builtin_len,
        builtin_abs,
        builtin_min,
        builtin_max,
        builtin_sum,
        builtin_round,
        builtin_pow,
        builtin_divmod,
        builtin_chr,
        builtin_ord,
        builtin_bin,
        builtin_hex,
        builtin_oct,
        builtin_int,
        builtin_float,
        builtin_str,
        builtin_bool,
        builtin_all,
        builtin_any,
        builtin_sorted,
        builtin_reversed,
        builtin_enumerate,
        builtin_zip,
        builtin_list,
        builtin_range,
        builtin_repr,
        str_upper,
        str_lower,
        str_capitalize,
        str_title,
        str_swapcase,
        str_strip,
        str_lstrip,
        str_rstrip,
        str_startswith,
        str_endswith,
        str_split,
        str_rsplit,
        str_replace,
        str_join,
        str_find,
        str_rfind,
        str_index,
        str_rindex,
        str_count,
        str_isdigit,
        str_isalpha,
        str_isalnum,
        str_isspace,
        str_islower,
        str_isupper,
        str_istitle,
        str_isascii,
        str_center,
        str_ljust,
        str_rjust,
        str_zfill,
        str_removeprefix,
        str_removesuffix,
        str_partition,
        str_rpartition,
        json_loads,
        json_dumps,
        json_load,
        json_dump,
        handle_enter,
        handle_leave,
        raise_fn,
        parallel_for,
    })
}

fn declare_user_funcs<M: Module>(
    module: &mut M,
    ir: &IrModule,
) -> Result<HashMap<String, FuncId>, String> {
    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    for func in &ir.functions {
        let mut sig = module.make_signature();
        // Arguments and results are passed as (payload, kind) pairs so values
        // keep their type across call boundaries.
        for _ in &func.params {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        let id = module
            .declare_function(&func.name, Linkage::Local, &sig)
            .map_err(|e| e.to_string())?;
        func_ids.insert(func.name.clone(), id);
    }

    let mut sig = module.make_signature();
    sig.returns.push(AbiParam::new(types::I64));
    let main_id = module
        .declare_function("__main__", Linkage::Export, &sig)
        .map_err(|e| e.to_string())?;
    func_ids.insert("__main__".to_string(), main_id);
    Ok(func_ids)
}

fn kind_of(map: &HashMap<ValueId, ValueKind>, id: ValueId) -> ValueKind {
    map.get(&id).copied().unwrap_or(ValueKind::I64)
}

/// Kinds whose `==` cannot be a raw payload comparison.
pub(crate) fn needs_runtime_eq(kind: ValueKind) -> bool {
    matches!(
        kind,
        ValueKind::Str | ValueKind::List | ValueKind::Dict | ValueKind::Struct | ValueKind::Dynamic
    )
}

fn kind_operand(
    builder: &mut FunctionBuilder,
    kind_vars: &HashMap<ValueId, Variable>,
    kind: ValueKind,
    id: ValueId,
) -> Value {
    match kind {
        ValueKind::Dynamic => match kind_vars.get(&id) {
            Some(kv) => builder.use_var(*kv),
            None => builder.ins().iconst(types::I64, 0),
        },
        other => builder.ins().iconst(types::I64, other.as_i64()),
    }
}

fn named_kind(map: &HashMap<String, ValueKind>, name: &str) -> ValueKind {
    map.get(name).copied().unwrap_or(ValueKind::I64)
}

/// Names that may hold more than one runtime kind (or are function params).
/// Only these need a Cranelift kind SSA variable — monomorphic locals (e.g. range
/// induction `i`) skip the per-iteration kind `iconst`/`def_var` tax.
pub(crate) fn names_needing_kind_vars(body: &[IrInstr], params: &[String]) -> HashSet<String> {
    let mut value_kinds: HashMap<ValueId, ValueKind> = HashMap::new();
    let mut named_kinds: HashMap<String, ValueKind> = HashMap::new();
    let mut need: HashSet<String> = params.iter().cloned().collect();

    let set_val = |map: &mut HashMap<ValueId, ValueKind>, id: ValueId, k: ValueKind| {
        map.insert(id, k);
    };
    let vk = |map: &HashMap<ValueId, ValueKind>, id: ValueId| {
        map.get(&id).copied().unwrap_or(ValueKind::Dynamic)
    };

    for instr in body {
        match instr {
            IrInstr::ConstI64 { dest, .. } => set_val(&mut value_kinds, *dest, ValueKind::I64),
            IrInstr::ConstF64 { dest, .. } => set_val(&mut value_kinds, *dest, ValueKind::F64),
            IrInstr::ConstBool { dest, .. } => set_val(&mut value_kinds, *dest, ValueKind::Bool),
            IrInstr::ConstNone { dest } => set_val(&mut value_kinds, *dest, ValueKind::None_),
            IrInstr::ConstStr { dest, .. } => set_val(&mut value_kinds, *dest, ValueKind::Str),
            IrInstr::Load { dest, name } => {
                let k = named_kinds
                    .get(name)
                    .copied()
                    .unwrap_or(if need.contains(name) {
                        ValueKind::Dynamic
                    } else {
                        ValueKind::I64
                    });
                set_val(&mut value_kinds, *dest, k);
            }
            IrInstr::Store { name, value } => {
                let incoming = vk(&value_kinds, *value);
                let merged = match named_kinds.get(name) {
                    Some(prev) if *prev != incoming => ValueKind::Dynamic,
                    Some(prev) => *prev,
                    None => incoming,
                };
                named_kinds.insert(name.clone(), merged);
                if merged == ValueKind::Dynamic || incoming == ValueKind::Dynamic {
                    need.insert(name.clone());
                }
            }
            IrInstr::Unary { dest, op, src } => {
                let sk = vk(&value_kinds, *src);
                let out = match op {
                    IrOp::Not => ValueKind::Bool,
                    IrOp::Neg if sk == ValueKind::F64 => ValueKind::F64,
                    IrOp::Neg => ValueKind::I64,
                    _ => ValueKind::Dynamic,
                };
                set_val(&mut value_kinds, *dest, out);
            }
            IrInstr::Binary {
                dest,
                op,
                left,
                right,
            } => {
                let lk = vk(&value_kinds, *left);
                let rk = vk(&value_kinds, *right);
                let out = if lk == ValueKind::Str && rk == ValueKind::Str && matches!(op, IrOp::Add)
                {
                    ValueKind::Str
                } else if matches!(
                    op,
                    IrOp::Eq | IrOp::Ne | IrOp::Lt | IrOp::Le | IrOp::Gt | IrOp::Ge
                ) {
                    ValueKind::Bool
                } else if lk == ValueKind::F64 || rk == ValueKind::F64 {
                    ValueKind::F64
                } else if lk == ValueKind::Dynamic || rk == ValueKind::Dynamic {
                    ValueKind::Dynamic
                } else {
                    ValueKind::I64
                };
                set_val(&mut value_kinds, *dest, out);
            }
            IrInstr::Call { dest, .. }
            | IrInstr::MakeList { dest, .. }
            | IrInstr::MakeDict { dest, .. }
            | IrInstr::IndexGet { dest, .. }
            | IrInstr::MakeStruct { dest, .. }
            | IrInstr::StructGet { dest, .. } => {
                set_val(&mut value_kinds, *dest, ValueKind::Dynamic);
            }
            IrInstr::ListLen { dest, .. } => set_val(&mut value_kinds, *dest, ValueKind::I64),
            IrInstr::ValueToStr { dest, .. } | IrInstr::StrConcat { dest, .. } => {
                set_val(&mut value_kinds, *dest, ValueKind::Str);
            }
            IrInstr::IntWrap {
                dest,
                bits,
                signed,
                ..
            } => {
                let k = if !*signed && *bits == 64 {
                    ValueKind::U64
                } else {
                    ValueKind::I64
                };
                set_val(&mut value_kinds, *dest, k);
            }
            _ => {}
        }
    }

    for (name, kind) in named_kinds {
        if kind == ValueKind::Dynamic {
            need.insert(name);
        }
    }
    need
}

fn i64_to_f64(builder: &mut FunctionBuilder, v: Value) -> Value {
    builder.ins().bitcast(types::F64, MemFlags::new(), v)
}

fn f64_to_i64(builder: &mut FunctionBuilder, v: Value) -> Value {
    builder.ins().bitcast(types::I64, MemFlags::new(), v)
}

pub fn dump_ir(module: &IrModule) {
    println!("{}", module);
}

/// Emit LLVM IR text to `out_path` (typically `.ll`).
pub fn emit_llvm(module: &IrModule, out_path: &str) -> Result<(), String> {
    let ir = super::llvm_emit::emit_llvm_ir(module)?;
    std::fs::write(out_path, ir).map_err(|e| e.to_string())
}

/// Cranelift-only: Hyper-IR → machine object (rustc_codegen_cranelift niche).
pub fn emit_object(module: &IrModule, out_path: &str) -> Result<(), String> {
    let flags = make_flags(true)?;
    let isa_builder =
        cranelift_native::builder().map_err(|msg| format!("host unsupported: {msg}"))?;
    let isa = isa_builder.finish(flags).map_err(|e| e.to_string())?;

    let builder = ObjectBuilder::new(isa, "hyper", default_libcall_names())
        .map_err(|e| e.to_string())?;
    let mut obj = ObjectModule::new(builder);
    let mut ctx = obj.make_context();
    let mut func_ctx = FunctionBuilderContext::new();
    let mut strings = StringData::new();

    let runtime = declare_runtime(&mut obj)?;
    let func_ids = declare_user_funcs(&mut obj, module)?;
    let main_id = func_ids["__main__"];

    for func in &module.functions {
        let id = func_ids[&func.name];
        define_function(
            &mut obj,
            &mut ctx,
            &mut func_ctx,
            id,
            &func.params,
            &func.body,
            &func_ids,
            &runtime,
            &mut strings,
            true,
        )?;
    }

    define_function(
        &mut obj,
        &mut ctx,
        &mut func_ctx,
        main_id,
        &[],
        &module.main,
        &func_ids,
        &runtime,
        &mut strings,
        false,
    )?;

    let product = obj.finish();
    let bytes = product.emit().map_err(|e| e.to_string())?;
    std::fs::write(out_path, &bytes).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn runtime_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("compiler").join("runtime").join("hyper_rt.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn runtime_file_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("compiler").join("runtime").join("hyper_rt_file.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn runtime_json_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("compiler").join("runtime").join("hyper_rt_json.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn runtime_mmap_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("compiler").join("runtime").join("hyper_rt_mmap.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn runtime_io_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("compiler").join("runtime").join("hyper_rt_io.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn runtime_str_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("compiler").join("runtime").join("hyper_rt_str.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn runtime_builtins_c_path() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .join("compiler")
        .join("runtime")
        .join("hyper_rt_builtins.c");
    if !path.exists() {
        return Err(format!("runtime source not found: {}", path.display()));
    }
    Ok(path)
}

fn find_cc() -> Result<(String, bool), String> {
    // Returns (program, is_msvc_cl).
    if let Ok(cc) = std::env::var("CC") {
        let trimmed = cc.trim();
        if !trimmed.is_empty() {
            let is_msvc = is_msvc_driver(trimmed);
            if linker_works(trimmed, is_msvc) {
                return Ok((trimmed.to_string(), is_msvc));
            }
            return Err(format!(
                "CC={trimmed} was set but could not be executed"
            ));
        }
    }

    // Prefer LLVM's clang for linking on all platforms; fall back to host cc / MSVC.
    #[cfg(windows)]
    let candidates: &[&str] = &["clang", "clang-cl", "gcc", "cl"];
    #[cfg(not(windows))]
    let candidates: &[&str] = &["clang", "gcc", "cc"];

    for cand in candidates {
        let is_msvc = is_msvc_driver(cand);
        if linker_works(cand, is_msvc) {
            return Ok(((*cand).to_string(), is_msvc));
        }
    }

    #[cfg(windows)]
    {
        Err(
            "no C compiler found (tried clang, clang-cl, gcc, cl). \
             Install Visual Studio Build Tools, LLVM, or MinGW, \
             or set the CC environment variable"
                .to_string(),
        )
    }
    #[cfg(not(windows))]
    {
        Err("no C compiler found (tried clang, gcc, cc)".to_string())
    }
}

fn is_msvc_driver(prog: &str) -> bool {
    let name = Path::new(prog)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(prog)
        .to_ascii_lowercase();
    name == "cl" || name == "clang-cl"
}

fn is_clang_driver(prog: &str) -> bool {
    let name = Path::new(prog)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(prog)
        .to_ascii_lowercase();
    name == "clang"
}

fn linker_works(prog: &str, is_msvc: bool) -> bool {
    let mut cmd = Command::new(prog);
    if is_msvc {
        cmd.arg("/?");
    } else {
        cmd.arg("--version");
    }
    cmd.output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

pub(crate) fn normalize_exe_path(out_path: &str) -> String {
    #[cfg(windows)]
    {
        let p = Path::new(out_path);
        if p.extension().is_none() {
            return format!("{out_path}.exe");
        }
    }
    out_path.to_string()
}

pub fn emit_exe(module: &IrModule, out_path: &str) -> Result<(), String> {
    match default_backend() {
        CodegenBackend::Llvm => super::llvm_emit::emit_exe_llvm(module, out_path),
        CodegenBackend::Cranelift => emit_exe_cranelift(module, out_path),
    }
}

fn emit_exe_cranelift(module: &IrModule, out_path: &str) -> Result<(), String> {
    // Cranelift emits the machine object; clang/LLVM (or the host C toolchain) links
    // it with the Hyper C runtime and the system C library.
    let tmp_dir = std::env::temp_dir();
    let obj_ext = if cfg!(windows) { "obj" } else { "o" };
    let obj_path = tmp_dir.join(format!("hyper_{}.{}", std::process::id(), obj_ext));
    let obj_str = obj_path
        .to_str()
        .ok_or_else(|| "temp object path is not valid UTF-8".to_string())?;

    emit_object(module, obj_str)?;

    let rt = runtime_c_path()?;
    let rt_file = runtime_file_c_path()?;
    let rt_json = runtime_json_c_path()?;
    let rt_mmap = runtime_mmap_c_path()?;
    let rt_io = runtime_io_c_path()?;
    let rt_str = runtime_str_c_path()?;
    let rt_builtins = runtime_builtins_c_path()?;
    let (cc, is_msvc) = find_cc()?;
    let out = normalize_exe_path(out_path);

    let status = if is_msvc {
        // cl / clang-cl: /Fe sets the executable name; /Fo keeps .obj junk out of cwd.
        let fo_dir = tmp_dir.join(format!("hyper_link_{}", std::process::id()));
        std::fs::create_dir_all(&fo_dir)
            .map_err(|e| format!("failed to create temp link dir: {e}"))?;
        let mut fo = fo_dir.to_string_lossy().into_owned();
        if !fo.ends_with('\\') && !fo.ends_with('/') {
            fo.push('\\');
        }
        let status = Command::new(&cc)
            .arg("/nologo")
            .arg("/D_CRT_SECURE_NO_WARNINGS")
            .arg(format!("/Fo{fo}"))
            .arg(format!("/Fe:{out}"))
            .arg(obj_str)
            .arg(rt.as_os_str())
            .arg(rt_file.as_os_str())
            .arg(rt_json.as_os_str())
            .arg(rt_mmap.as_os_str())
            .arg(rt_io.as_os_str())
            .arg(rt_str.as_os_str())
            .arg(rt_builtins.as_os_str())
            .status()
            .map_err(|e| format!("failed to invoke {cc}: {e}"))?;
        let _ = std::fs::remove_dir_all(&fo_dir);
        status
    } else {
        let mut cmd = Command::new(&cc);
        // Prefer -O2 when linking with clang for better AOT quality on the C runtime.
        if is_clang_driver(&cc) {
            cmd.arg("-O2");
        }
        cmd.arg(obj_str)
            .arg(rt.as_os_str())
            .arg(rt_file.as_os_str())
            .arg(rt_json.as_os_str())
            .arg(rt_mmap.as_os_str())
            .arg(rt_io.as_os_str())
            .arg(rt_str.as_os_str())
            .arg(rt_builtins.as_os_str())
            .arg("-o")
            .arg(&out);
        // libm is separate on many Unix toolchains; not required on Windows.
        if !cfg!(windows) {
            cmd.arg("-lm");
            cmd.arg("-pthread");
        }
        cmd.status()
            .map_err(|e| format!("failed to invoke {cc}: {e}"))?
    };

    let _ = std::fs::remove_file(&obj_path);

    if !status.success() {
        return Err(format!("{cc} failed with status {status}"));
    }
    Ok(())
}

fn file_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    // (func id, result kind, uses out_kind pointer like readline)
    match func {
        "hyper_rt_file_open" => Some((runtime.file_open, ValueKind::File, false)),
        "hyper_rt_file_read_all" | "hyper_rt_file_path" | "hyper_rt_file_mode" => {
            Some((match func {
                "hyper_rt_file_read_all" => runtime.file_read_all,
                "hyper_rt_file_path" => runtime.file_path,
                _ => runtime.file_mode,
            }, ValueKind::Str, false))
        }
        "hyper_rt_file_read_n" => Some((runtime.file_read_n, ValueKind::Str, false)),
        "hyper_rt_file_readline" => Some((runtime.file_readline, ValueKind::Dynamic, true)),
        "hyper_rt_file_readlines" => Some((runtime.file_readlines, ValueKind::List, false)),
        "hyper_rt_file_write" | "hyper_rt_file_writelines" | "hyper_rt_file_seek"
        | "hyper_rt_file_tell" | "hyper_rt_file_size" | "hyper_rt_file_is_closed" => {
            Some((
                match func {
                    "hyper_rt_file_write" => runtime.file_write,
                    "hyper_rt_file_writelines" => runtime.file_writelines,
                    "hyper_rt_file_seek" => runtime.file_seek,
                    "hyper_rt_file_tell" => runtime.file_tell,
                    "hyper_rt_file_size" => runtime.file_size,
                    _ => runtime.file_is_closed,
                },
                if func == "hyper_rt_file_is_closed" {
                    ValueKind::Bool
                } else {
                    ValueKind::I64
                },
                false,
            ))
        }
        "hyper_rt_file_close" => Some((runtime.file_close, ValueKind::None_, false)),
        "hyper_rt_file_flush" => Some((runtime.file_flush, ValueKind::None_, false)),
        _ => None,
    }
}

fn mmap_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_mmap_open" => Some((runtime.mmap_open, ValueKind::Mmap, false)),
        "hyper_rt_mmap_read_chunk" => Some((runtime.mmap_read_chunk, ValueKind::Str, false)),
        "hyper_rt_mmap_close" => Some((runtime.mmap_close, ValueKind::None_, false)),
        _ => None,
    }
}

fn clock_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_clock" => Some((runtime.clock_fn, ValueKind::F64, false)),
        _ => None,
    }
}

fn coll_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_coll_len" => Some((runtime.coll_len, ValueKind::I64, false)),
        "hyper_rt_coll_append" => Some((runtime.coll_append, ValueKind::None_, false)),
        "hyper_rt_coll_keys" => Some((runtime.coll_keys, ValueKind::List, false)),
        _ => None,
    }
}

fn builtin_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_builtin_len" => Some((runtime.builtin_len, ValueKind::I64, false)),
        "hyper_rt_builtin_abs" => Some((runtime.builtin_abs, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_min" => Some((runtime.builtin_min, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_max" => Some((runtime.builtin_max, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_sum" => Some((runtime.builtin_sum, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_round" => Some((runtime.builtin_round, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_pow" => Some((runtime.builtin_pow, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_divmod" => Some((runtime.builtin_divmod, ValueKind::List, false)),
        "hyper_rt_builtin_chr" => Some((runtime.builtin_chr, ValueKind::Str, false)),
        "hyper_rt_builtin_ord" => Some((runtime.builtin_ord, ValueKind::I64, false)),
        "hyper_rt_builtin_bin" => Some((runtime.builtin_bin, ValueKind::Str, false)),
        "hyper_rt_builtin_hex" => Some((runtime.builtin_hex, ValueKind::Str, false)),
        "hyper_rt_builtin_oct" => Some((runtime.builtin_oct, ValueKind::Str, false)),
        "hyper_rt_builtin_int" => Some((runtime.builtin_int, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_float" => Some((runtime.builtin_float, ValueKind::Dynamic, true)),
        "hyper_rt_builtin_str" => Some((runtime.builtin_str, ValueKind::Str, false)),
        "hyper_rt_builtin_bool" => Some((runtime.builtin_bool, ValueKind::Bool, false)),
        "hyper_rt_builtin_all" => Some((runtime.builtin_all, ValueKind::Bool, false)),
        "hyper_rt_builtin_any" => Some((runtime.builtin_any, ValueKind::Bool, false)),
        "hyper_rt_builtin_sorted" => Some((runtime.builtin_sorted, ValueKind::List, false)),
        "hyper_rt_builtin_reversed" => Some((runtime.builtin_reversed, ValueKind::List, false)),
        "hyper_rt_builtin_enumerate" => Some((runtime.builtin_enumerate, ValueKind::List, false)),
        "hyper_rt_builtin_zip" => Some((runtime.builtin_zip, ValueKind::List, false)),
        "hyper_rt_builtin_list" => Some((runtime.builtin_list, ValueKind::List, false)),
        "hyper_rt_builtin_range" => Some((runtime.builtin_range, ValueKind::List, false)),
        "hyper_rt_builtin_repr" => Some((runtime.builtin_repr, ValueKind::Str, false)),
        _ => None,
    }
}

fn str_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_str_upper" => Some((runtime.str_upper, ValueKind::Str, false)),
        "hyper_rt_str_lower" => Some((runtime.str_lower, ValueKind::Str, false)),
        "hyper_rt_str_capitalize" => Some((runtime.str_capitalize, ValueKind::Str, false)),
        "hyper_rt_str_title" => Some((runtime.str_title, ValueKind::Str, false)),
        "hyper_rt_str_swapcase" => Some((runtime.str_swapcase, ValueKind::Str, false)),
        "hyper_rt_str_strip" => Some((runtime.str_strip, ValueKind::Str, false)),
        "hyper_rt_str_lstrip" => Some((runtime.str_lstrip, ValueKind::Str, false)),
        "hyper_rt_str_rstrip" => Some((runtime.str_rstrip, ValueKind::Str, false)),
        "hyper_rt_str_replace" => Some((runtime.str_replace, ValueKind::Str, false)),
        "hyper_rt_str_join" => Some((runtime.str_join, ValueKind::Str, false)),
        "hyper_rt_str_center" => Some((runtime.str_center, ValueKind::Str, false)),
        "hyper_rt_str_ljust" => Some((runtime.str_ljust, ValueKind::Str, false)),
        "hyper_rt_str_rjust" => Some((runtime.str_rjust, ValueKind::Str, false)),
        "hyper_rt_str_zfill" => Some((runtime.str_zfill, ValueKind::Str, false)),
        "hyper_rt_str_removeprefix" => Some((runtime.str_removeprefix, ValueKind::Str, false)),
        "hyper_rt_str_removesuffix" => Some((runtime.str_removesuffix, ValueKind::Str, false)),
        "hyper_rt_str_startswith" => Some((runtime.str_startswith, ValueKind::Bool, false)),
        "hyper_rt_str_endswith" => Some((runtime.str_endswith, ValueKind::Bool, false)),
        "hyper_rt_str_isdigit" => Some((runtime.str_isdigit, ValueKind::Bool, false)),
        "hyper_rt_str_isalpha" => Some((runtime.str_isalpha, ValueKind::Bool, false)),
        "hyper_rt_str_isalnum" => Some((runtime.str_isalnum, ValueKind::Bool, false)),
        "hyper_rt_str_isspace" => Some((runtime.str_isspace, ValueKind::Bool, false)),
        "hyper_rt_str_islower" => Some((runtime.str_islower, ValueKind::Bool, false)),
        "hyper_rt_str_isupper" => Some((runtime.str_isupper, ValueKind::Bool, false)),
        "hyper_rt_str_istitle" => Some((runtime.str_istitle, ValueKind::Bool, false)),
        "hyper_rt_str_isascii" => Some((runtime.str_isascii, ValueKind::Bool, false)),
        "hyper_rt_str_find" => Some((runtime.str_find, ValueKind::I64, false)),
        "hyper_rt_str_rfind" => Some((runtime.str_rfind, ValueKind::I64, false)),
        "hyper_rt_str_index" => Some((runtime.str_index, ValueKind::I64, false)),
        "hyper_rt_str_rindex" => Some((runtime.str_rindex, ValueKind::I64, false)),
        "hyper_rt_str_count" => Some((runtime.str_count, ValueKind::I64, false)),
        "hyper_rt_str_split" => Some((runtime.str_split, ValueKind::List, false)),
        "hyper_rt_str_rsplit" => Some((runtime.str_rsplit, ValueKind::List, false)),
        "hyper_rt_str_partition" => Some((runtime.str_partition, ValueKind::List, false)),
        "hyper_rt_str_rpartition" => Some((runtime.str_rpartition, ValueKind::List, false)),
        _ => None,
    }
}

fn input_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_input" => Some((runtime.input_fn, ValueKind::Str, false)),
        _ => None,
    }
}

fn json_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_json_loads" | "hyper_rt_json_load" => Some((
            if func == "hyper_rt_json_loads" {
                runtime.json_loads
            } else {
                runtime.json_load
            },
            ValueKind::Dynamic,
            true,
        )),
        "hyper_rt_json_dumps" => Some((runtime.json_dumps, ValueKind::Str, false)),
        "hyper_rt_json_dump" => Some((runtime.json_dump, ValueKind::I64, false)),
        _ => None,
    }
}

fn error_runtime_call(func: &str, runtime: &RuntimeIds) -> Option<(FuncId, ValueKind, bool)> {
    match func {
        "hyper_rt_handle_enter" => Some((runtime.handle_enter, ValueKind::I64, false)),
        "hyper_rt_handle_leave" => Some((runtime.handle_leave, ValueKind::Bool, false)),
        "hyper_rt_raise" => Some((runtime.raise_fn, ValueKind::I64, false)),
        _ => None,
    }
}

fn instr_uses(instr: &IrInstr) -> Vec<ValueId> {
    match instr {
        IrInstr::ConstI64 { .. }
        | IrInstr::ConstF64 { .. }
        | IrInstr::ConstBool { .. }
        | IrInstr::ConstStr { .. }
        | IrInstr::ConstNone { .. }
        | IrInstr::Load { .. }
        | IrInstr::Label { .. }
        | IrInstr::Jump { .. }
        | IrInstr::MakeStruct { .. } => vec![],
        IrInstr::Store { value, .. } => vec![*value],
        IrInstr::Unary { src, .. } => vec![*src],
        IrInstr::Binary { left, right, .. } => vec![*left, *right],
        IrInstr::IntWrap { src, .. } => vec![*src],
        IrInstr::GuardDivisor { value, .. } => vec![*value],
        IrInstr::Call { args, .. } => args.clone(),
        IrInstr::MakeList { items, .. } => items.clone(),
        IrInstr::MakeDict { entries, .. } => entries.iter().flat_map(|(k, v)| [*k, *v]).collect(),
        IrInstr::IndexGet { object, index, .. } => vec![*object, *index],
        IrInstr::IndexSet {
            object,
            index,
            value,
        } => vec![*object, *index, *value],
        IrInstr::ListLen { list, .. } => vec![*list],
        IrInstr::ValueToStr { src, .. } => vec![*src],
        IrInstr::StrConcat { left, right, .. } => vec![*left, *right],
        IrInstr::StructGet { object, .. } => vec![*object],
        IrInstr::StructSet { object, value, .. } => vec![*object, *value],
        IrInstr::Print { args } => args.clone(),
        IrInstr::Return { value } => value.iter().copied().collect(),
        IrInstr::Branch { cond, .. } => vec![*cond],
        IrInstr::ParallelRange { start, end, .. } => vec![*start, *end],
    }
}

pub(crate) fn call_returns_owned_str(func: &str) -> bool {
    matches!(
        func,
        "hyper_rt_str_upper"
            | "hyper_rt_str_lower"
            | "hyper_rt_str_capitalize"
            | "hyper_rt_str_title"
            | "hyper_rt_str_swapcase"
            | "hyper_rt_str_strip"
            | "hyper_rt_str_lstrip"
            | "hyper_rt_str_rstrip"
            | "hyper_rt_str_replace"
            | "hyper_rt_str_join"
            | "hyper_rt_str_center"
            | "hyper_rt_str_ljust"
            | "hyper_rt_str_rjust"
            | "hyper_rt_str_zfill"
            | "hyper_rt_str_removeprefix"
            | "hyper_rt_str_removesuffix"
            | "hyper_rt_input"
            | "hyper_rt_json_dumps"
            | "hyper_rt_file_read_all"
            | "hyper_rt_file_read_n"
            | "hyper_rt_file_path"
            | "hyper_rt_file_mode"
            | "hyper_rt_mmap_read_chunk"
    )
}

/// True when `name` is overwritten with `concat_dest` before it is reloaded or
/// control leaves the current straight-line region (`s = s + …`).
fn concat_stored_back_to_name(
    body: &[IrInstr],
    concat_idx: usize,
    concat_dest: ValueId,
    name: &str,
) -> bool {
    for instr in body.iter().skip(concat_idx + 1) {
        match instr {
            IrInstr::Label { .. } => {}
            IrInstr::Store { name: n, value } if n == name => return *value == concat_dest,
            IrInstr::Load { name: n, .. } if n == name => return false,
            IrInstr::Jump { .. } | IrInstr::Branch { .. } | IrInstr::Return { .. } => return false,
            _ => {}
        }
    }
    false
}

fn should_consume_concat_operand(
    operand: ValueId,
    concat_idx: usize,
    concat_dest: ValueId,
    last_use: &HashMap<ValueId, usize>,
    owned_temps: &HashSet<ValueId>,
    load_names: &HashMap<ValueId, String>,
    body: &[IrInstr],
) -> bool {
    if last_use.get(&operand).copied() != Some(concat_idx) {
        return false;
    }
    if owned_temps.contains(&operand) {
        return true;
    }
    if let Some(name) = load_names.get(&operand) {
        return concat_stored_back_to_name(body, concat_idx, concat_dest, name);
    }
    false
}

/// Per-instruction `(consume_left, consume_right)` for `str_concat` / string `+`.
pub(crate) fn concat_consume_plan(body: &[IrInstr]) -> Vec<(bool, bool)> {
    let mut value_kinds: HashMap<ValueId, ValueKind> = HashMap::new();
    let mut named_kinds: HashMap<String, ValueKind> = HashMap::new();
    let mut owned_temps: HashSet<ValueId> = HashSet::new();
    let mut load_names: HashMap<ValueId, String> = HashMap::new();
    let mut last_use: HashMap<ValueId, usize> = HashMap::new();
    let mut concat_dests: Vec<Option<ValueId>> = vec![None; body.len()];

    let vk = |map: &HashMap<ValueId, ValueKind>, id: ValueId| {
        map.get(&id).copied().unwrap_or(ValueKind::Dynamic)
    };

    for (i, instr) in body.iter().enumerate() {
        for u in instr_uses(instr) {
            last_use.insert(u, i);
        }
        match instr {
            IrInstr::ConstI64 { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::I64);
            }
            IrInstr::ConstF64 { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::F64);
            }
            IrInstr::ConstBool { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::Bool);
            }
            IrInstr::ConstNone { dest } => {
                value_kinds.insert(*dest, ValueKind::None_);
            }
            IrInstr::ConstStr { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::Str);
            }
            IrInstr::Load { dest, name } => {
                let k = named_kinds.get(name).copied().unwrap_or(ValueKind::Dynamic);
                value_kinds.insert(*dest, k);
                load_names.insert(*dest, name.clone());
            }
            IrInstr::Store { name, value } => {
                let incoming = vk(&value_kinds, *value);
                let merged = match named_kinds.get(name) {
                    Some(prev) if *prev != incoming => ValueKind::Dynamic,
                    Some(prev) => *prev,
                    None => incoming,
                };
                named_kinds.insert(name.clone(), merged);
            }
            IrInstr::ValueToStr { dest, src } => {
                value_kinds.insert(*dest, ValueKind::Str);
                if vk(&value_kinds, *src) != ValueKind::Str {
                    owned_temps.insert(*dest);
                } else if owned_temps.contains(src) {
                    owned_temps.insert(*dest);
                }
            }
            IrInstr::StrConcat { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::Str);
                owned_temps.insert(*dest);
                concat_dests[i] = Some(*dest);
            }
            IrInstr::Binary {
                dest,
                op,
                left,
                right,
            } => {
                let lk = vk(&value_kinds, *left);
                let rk = vk(&value_kinds, *right);
                let out = if lk == ValueKind::Str
                    && rk == ValueKind::Str
                    && matches!(op, IrOp::Add)
                {
                    owned_temps.insert(*dest);
                    concat_dests[i] = Some(*dest);
                    ValueKind::Str
                } else if matches!(
                    op,
                    IrOp::Eq | IrOp::Ne | IrOp::Lt | IrOp::Le | IrOp::Gt | IrOp::Ge
                ) {
                    ValueKind::Bool
                } else if lk == ValueKind::F64 || rk == ValueKind::F64 {
                    ValueKind::F64
                } else {
                    ValueKind::I64
                };
                value_kinds.insert(*dest, out);
            }
            IrInstr::Call { dest, func, .. } => {
                if call_returns_owned_str(func) {
                    value_kinds.insert(*dest, ValueKind::Str);
                    owned_temps.insert(*dest);
                } else {
                    value_kinds.insert(*dest, ValueKind::Dynamic);
                }
            }
            IrInstr::ListLen { dest, .. } | IrInstr::IntWrap { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::I64);
            }
            IrInstr::MakeList { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::List);
            }
            IrInstr::MakeDict { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::Dict);
            }
            IrInstr::MakeStruct { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::Struct);
            }
            IrInstr::IndexGet { dest, .. } | IrInstr::StructGet { dest, .. } => {
                value_kinds.insert(*dest, ValueKind::Dynamic);
            }
            _ => {}
        }
    }

    let mut plan = vec![(false, false); body.len()];
    for (i, instr) in body.iter().enumerate() {
        let (left, right, dest) = match instr {
            IrInstr::StrConcat { dest, left, right } => (*left, *right, *dest),
            IrInstr::Binary {
                dest,
                op: IrOp::Add,
                left,
                right,
            } if concat_dests[i].is_some() => (*left, *right, *dest),
            _ => continue,
        };
        plan[i] = (
            should_consume_concat_operand(
                left, i, dest, &last_use, &owned_temps, &load_names, body,
            ),
            should_consume_concat_operand(
                right, i, dest, &last_use, &owned_temps, &load_names, body,
            ),
        );
    }
    plan
}

fn define_function<M: Module>(
    module: &mut M,
    ctx: &mut cranelift_codegen::Context,
    func_ctx: &mut FunctionBuilderContext,
    func_id: FuncId,
    params: &[String],
    body: &[IrInstr],
    func_ids: &HashMap<String, FuncId>,
    runtime: &RuntimeIds,
    strings: &mut StringData,
    returns_kind: bool,
) -> Result<(), String> {
    let mut sig = module.make_signature();
    for _ in params {
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    if returns_kind {
        sig.returns.push(AbiParam::new(types::I64));
    }

    ctx.func = Function::new();
    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, func_id.as_u32());

    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, func_ctx);

        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);

        let mut blocks: HashMap<BlockId, cranelift_codegen::ir::Block> = HashMap::new();
        for instr in body {
            if let IrInstr::Label { block } = instr {
                blocks
                    .entry(*block)
                    .or_insert_with(|| builder.create_block());
            }
        }

        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let mut next_var = 0usize;
        let mut value_vars: HashMap<ValueId, Variable> = HashMap::new();
        let mut named_vars: HashMap<String, Variable> = HashMap::new();
        let mut named_defs: HashSet<String> = HashSet::new();
        let mut value_kinds: HashMap<ValueId, ValueKind> = HashMap::new();
        let mut named_kinds: HashMap<String, ValueKind> = HashMap::new();
        let mut kind_vars: HashMap<ValueId, Variable> = HashMap::new();
        let mut named_kind_vars: HashMap<String, Variable> = HashMap::new();
        let mut terminated = false;

        let declare_var = |builder: &mut FunctionBuilder, next_var: &mut usize| {
            let v = Variable::new(*next_var);
            *next_var += 1;
            builder.declare_var(v, types::I64);
            v
        };

        let param_vals: Vec<Value> = builder.block_params(entry).to_vec();
        for (i, name) in params.iter().enumerate() {
            let var = declare_var(&mut builder, &mut next_var);
            builder.def_var(var, param_vals[i * 2]);
            named_vars.insert(name.clone(), var);
            named_defs.insert(name.clone());

            let kv = declare_var(&mut builder, &mut next_var);
            builder.def_var(kv, param_vals[i * 2 + 1]);
            named_kind_vars.insert(name.clone(), kv);
            named_kinds.insert(name.clone(), ValueKind::Dynamic);
        }

        let ensure_val = |id: ValueId,
                          builder: &mut FunctionBuilder,
                          next_var: &mut usize,
                          value_vars: &mut HashMap<ValueId, Variable>| {
            if !value_vars.contains_key(&id) {
                let v = declare_var(builder, next_var);
                value_vars.insert(id, v);
            }
        };
        let ensure_named = |name: &str,
                            builder: &mut FunctionBuilder,
                            next_var: &mut usize,
                            named_vars: &mut HashMap<String, Variable>| {
            if !named_vars.contains_key(name) {
                let v = declare_var(builder, next_var);
                named_vars.insert(name.to_string(), v);
            }
        };

        for instr in body {
            match instr {
                IrInstr::ConstI64 { dest, .. }
                | IrInstr::ConstF64 { dest, .. }
                | IrInstr::ConstBool { dest, .. }
                | IrInstr::ConstNone { dest }
                | IrInstr::ConstStr { dest, .. } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::Load { dest, name } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_named(name, &mut builder, &mut next_var, &mut named_vars);
                }
                IrInstr::Store { name, value } => {
                    ensure_named(name, &mut builder, &mut next_var, &mut named_vars);
                    ensure_val(*value, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::Unary { dest, src, .. } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*src, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::Binary {
                    dest,
                    left,
                    right,
                    ..
                } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*left, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*right, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::IntWrap { dest, src, .. } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*src, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::Call { dest, args, .. } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    for a in args {
                        ensure_val(*a, &mut builder, &mut next_var, &mut value_vars);
                    }
                }
                IrInstr::MakeList { dest, items } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    for a in items {
                        ensure_val(*a, &mut builder, &mut next_var, &mut value_vars);
                    }
                }
                IrInstr::MakeDict { dest, entries } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    for (k, v) in entries {
                        ensure_val(*k, &mut builder, &mut next_var, &mut value_vars);
                        ensure_val(*v, &mut builder, &mut next_var, &mut value_vars);
                    }
                }
                IrInstr::IndexGet {
                    dest,
                    object,
                    index,
                } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*object, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*index, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::IndexSet {
                    object,
                    index,
                    value,
                } => {
                    ensure_val(*object, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*index, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*value, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::ListLen { dest, list } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*list, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::GuardDivisor { value, .. } => {
                    ensure_val(*value, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::ValueToStr { dest, src } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*src, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::StrConcat { dest, left, right } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*left, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*right, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::MakeStruct { dest, .. } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::StructGet { dest, object, .. } => {
                    ensure_val(*dest, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*object, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::StructSet { object, value, .. } => {
                    ensure_val(*object, &mut builder, &mut next_var, &mut value_vars);
                    ensure_val(*value, &mut builder, &mut next_var, &mut value_vars);
                }
                IrInstr::Print { args } => {
                    for a in args {
                        ensure_val(*a, &mut builder, &mut next_var, &mut value_vars);
                    }
                }
                IrInstr::Return { value: Some(id) } | IrInstr::Branch { cond: id, .. } => {
                    ensure_val(*id, &mut builder, &mut next_var, &mut value_vars);
                }
                _ => {}
            }
        }

        for (name, var) in &named_vars {
            if !named_defs.contains(name) {
                let zero = builder.ins().iconst(types::I64, 0);
                builder.def_var(*var, zero);
                named_defs.insert(name.clone());
            }
        }

        // Only polymorphic (or param) names carry runtime kind SSA. Monomorphic
        // locals — especially range induction variables — skip the kind tax.
        let kind_needed = names_needing_kind_vars(body, params);
        let named_names: Vec<String> = named_vars.keys().cloned().collect();
        for name in named_names {
            if named_kind_vars.contains_key(&name) || !kind_needed.contains(&name) {
                continue;
            }
            let kv = declare_var(&mut builder, &mut next_var);
            let init = builder.ins().iconst(types::I64, ValueKind::I64.as_i64());
            builder.def_var(kv, init);
            named_kind_vars.insert(name, kv);
        }

        let concat_consume = concat_consume_plan(body);

        for (idx, instr) in body.iter().enumerate() {
            match instr {
                IrInstr::Label { block } => {
                    let b = blocks[block];
                    if !terminated {
                        builder.ins().jump(b, &[]);
                    }
                    builder.switch_to_block(b);
                    terminated = false;
                }
                _ if terminated => {}
                IrInstr::ConstI64 { dest, value } => {
                    let v = builder.ins().iconst(types::I64, *value);
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, ValueKind::I64);
                }
                IrInstr::IntWrap {
                    dest,
                    src,
                    bits,
                    signed,
                } => {
                    let s = builder.use_var(value_vars[src]);
                    let v = if *bits >= 64 {
                        s
                    } else if *signed {
                        let shift = builder.ins().iconst(types::I64, (64 - *bits) as i64);
                        let left = builder.ins().ishl(s, shift);
                        builder.ins().sshr(left, shift)
                    } else {
                        let mask_bits = if *bits == 0 {
                            0i64
                        } else {
                            (1i64 << *bits).wrapping_sub(1)
                        };
                        let mask = builder.ins().iconst(types::I64, mask_bits);
                        builder.ins().band(s, mask)
                    };
                    builder.def_var(value_vars[dest], v);
                    let out_kind = if !*signed && *bits == 64 {
                        ValueKind::U64
                    } else {
                        ValueKind::I64
                    };
                    value_kinds.insert(*dest, out_kind);
                }
                IrInstr::ConstF64 { dest, value } => {
                    let fv = builder
                        .ins()
                        .f64const(Ieee64::with_bits(value.to_bits()));
                    let v = f64_to_i64(&mut builder, fv);
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, ValueKind::F64);
                }
                IrInstr::ConstBool { dest, value } => {
                    let v = builder
                        .ins()
                        .iconst(types::I64, if *value { 1 } else { 0 });
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, ValueKind::Bool);
                }
                IrInstr::ConstNone { dest } => {
                    let v = builder.ins().iconst(types::I64, 0);
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, ValueKind::None_);
                }
                IrInstr::ConstStr { dest, value } => {
                    let data_id = strings.define(module, value)?;
                    let gv = module.declare_data_in_func(data_id, &mut builder.func);
                    let v = builder.ins().global_value(types::I64, gv);
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, ValueKind::Str);
                }
                IrInstr::Load { dest, name } => {
                    let val = builder.use_var(named_vars[name]);
                    builder.def_var(value_vars[dest], val);
                    let nk = named_kind(&named_kinds, name);
                    value_kinds.insert(*dest, nk);
                    if nk == ValueKind::Dynamic {
                        if let Some(kv) = named_kind_vars.get(name) {
                            let k = builder.use_var(*kv);
                            let dest_kv = declare_var(&mut builder, &mut next_var);
                            builder.def_var(dest_kv, k);
                            kind_vars.insert(*dest, dest_kv);
                        }
                    }
                }
                IrInstr::Store { name, value } => {
                    let val = builder.use_var(value_vars[value]);
                    builder.def_var(named_vars[name], val);
                    let vk = kind_of(&value_kinds, *value);

                    let merged = match named_kinds.get(name) {
                        Some(prev) if *prev != vk => ValueKind::Dynamic,
                        _ => vk,
                    };
                    named_kinds.insert(name.clone(), merged);

                    if let Some(kv) = named_kind_vars.get(name) {
                        let runtime_kind = if vk == ValueKind::Dynamic {
                            match kind_vars.get(value) {
                                Some(src_kv) => builder.use_var(*src_kv),
                                None => builder.ins().iconst(types::I64, 0),
                            }
                        } else {
                            builder.ins().iconst(types::I64, vk.as_i64())
                        };
                        builder.def_var(*kv, runtime_kind);
                    }
                }
                IrInstr::Unary { dest, op, src } => {
                    let s = builder.use_var(value_vars[src]);
                    let src_kind = kind_of(&value_kinds, *src);
                    let (v, out_kind) = match op {
                        IrOp::Neg if src_kind == ValueKind::F64 => {
                            let f = i64_to_f64(&mut builder, s);
                            let n = builder.ins().fneg(f);
                            (f64_to_i64(&mut builder, n), ValueKind::F64)
                        }
                        IrOp::Neg => (builder.ins().ineg(s), ValueKind::I64),
                        IrOp::Not => {
                            let zero = builder.ins().iconst(types::I64, 0);
                            let ne = builder.ins().icmp(IntCC::NotEqual, s, zero);
                            let one = builder.ins().iconst(types::I8, 1);
                            let b = builder.ins().bxor(ne, one);
                            (builder.ins().uextend(types::I64, b), ValueKind::Bool)
                        }
                        other => {
                            return Err(format!("codegen: unsupported unary op {other}"));
                        }
                    };
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, out_kind);
                }
                IrInstr::GuardDivisor { value, line } => {
                    let kind = kind_of(&value_kinds, *value);
                    if kind != ValueKind::F64 {
                        let v = builder.use_var(value_vars[value]);
                        let zero = builder.ins().iconst(types::I64, 0);
                        let mut is_zero = builder.ins().icmp(IntCC::Equal, v, zero);
                        if kind == ValueKind::Dynamic {
                            // A dynamic 0.0 has a zero payload but divides fine.
                            let vk = kind_operand(&mut builder, &kind_vars, kind, *value);
                            let f64_kind =
                                builder.ins().iconst(types::I64, ValueKind::F64.as_i64());
                            let not_float = builder.ins().icmp(IntCC::NotEqual, vk, f64_kind);
                            is_zero = builder.ins().band(is_zero, not_float);
                        }
                        let err_block = builder.create_block();
                        let ok_block = builder.create_block();
                        builder.ins().brif(is_zero, err_block, &[], ok_block, &[]);

                        builder.switch_to_block(err_block);
                        builder.seal_block(err_block);
                        let line_val = builder.ins().iconst(types::I64, *line as i64);
                        let fref = module
                            .declare_func_in_func(runtime.div_by_zero, &mut builder.func);
                        builder.ins().call(fref, &[line_val]);
                        builder.ins().jump(ok_block, &[]);

                        builder.switch_to_block(ok_block);
                        builder.seal_block(ok_block);
                    }
                }
                IrInstr::Binary {
                    dest,
                    op,
                    left,
                    right,
                } => {
                    let l = builder.use_var(value_vars[left]);
                    let r = builder.use_var(value_vars[right]);
                    let lk = kind_of(&value_kinds, *left);
                    let rk = kind_of(&value_kinds, *right);
                    let is_float = lk == ValueKind::F64 || rk == ValueKind::F64;

                    let (v, out_kind) = if lk == ValueKind::Str
                        && rk == ValueKind::Str
                        && matches!(op, IrOp::Add)
                    {
                        let (c_l, c_r) = concat_consume[idx];
                        let consume_l = builder.ins().iconst(types::I64, c_l as i64);
                        let consume_r = builder.ins().iconst(types::I64, c_r as i64);
                        let fref = module
                            .declare_func_in_func(runtime.str_concat, &mut builder.func);
                        let call = builder.ins().call(fref, &[l, r, consume_l, consume_r]);
                        (builder.inst_results(call)[0], ValueKind::Str)
                    } else if matches!(op, IrOp::Eq | IrOp::Ne)
                        && (needs_runtime_eq(lk) || needs_runtime_eq(rk))
                    {
                        // Strings, containers and dynamic values compare by content.
                        let lkind = kind_operand(&mut builder, &kind_vars, lk, *left);
                        let rkind = kind_operand(&mut builder, &kind_vars, rk, *right);
                        let fref =
                            module.declare_func_in_func(runtime.value_eq, &mut builder.func);
                        let call = builder.ins().call(fref, &[l, lkind, r, rkind]);
                        let eq = builder.inst_results(call)[0];
                        let v = if matches!(op, IrOp::Ne) {
                            let one = builder.ins().iconst(types::I64, 1);
                            builder.ins().bxor(eq, one)
                        } else {
                            eq
                        };
                        (v, ValueKind::Bool)
                    } else if is_float
                        && matches!(
                            op,
                            IrOp::Add | IrOp::Sub | IrOp::Mul | IrOp::Div | IrOp::FloorDiv | IrOp::Pow
                        )
                    {
                        let lf = if lk == ValueKind::F64 {
                            i64_to_f64(&mut builder, l)
                        } else {
                            builder.ins().fcvt_from_sint(types::F64, l)
                        };
                        let rf = if rk == ValueKind::F64 {
                            i64_to_f64(&mut builder, r)
                        } else {
                            builder.ins().fcvt_from_sint(types::F64, r)
                        };
                        let fv = match op {
                            IrOp::Add => builder.ins().fadd(lf, rf),
                            IrOp::Sub => builder.ins().fsub(lf, rf),
                            IrOp::Mul => builder.ins().fmul(lf, rf),
                            IrOp::Div => builder.ins().fdiv(lf, rf),
                            IrOp::FloorDiv => {
                                let fref = module.declare_func_in_func(
                                    runtime.floor_div_f64,
                                    &mut builder.func,
                                );
                                let call = builder.ins().call(fref, &[lf, rf]);
                                builder.inst_results(call)[0]
                            }
                            IrOp::Pow => {
                                let fref =
                                    module.declare_func_in_func(runtime.pow_f64, &mut builder.func);
                                let call = builder.ins().call(fref, &[lf, rf]);
                                builder.inst_results(call)[0]
                            }
                            _ => unreachable!(),
                        };
                        (f64_to_i64(&mut builder, fv), ValueKind::F64)
                    } else {
                        let v = match op {
                            IrOp::Add => builder.ins().iadd(l, r),
                            IrOp::Sub => builder.ins().isub(l, r),
                            IrOp::Mul => builder.ins().imul(l, r),
                            IrOp::Div => builder.ins().sdiv(l, r),
                            IrOp::FloorDiv => {
                                let fref = module.declare_func_in_func(
                                    runtime.floor_div_i64,
                                    &mut builder.func,
                                );
                                let call = builder.ins().call(fref, &[l, r]);
                                builder.inst_results(call)[0]
                            }
                            IrOp::Rem => builder.ins().srem(l, r),
                            IrOp::Pow => {
                                let fref =
                                    module.declare_func_in_func(runtime.pow_i64, &mut builder.func);
                                let call = builder.ins().call(fref, &[l, r]);
                                builder.inst_results(call)[0]
                            }
                            IrOp::Eq => {
                                let b = if is_float {
                                    let lf = i64_to_f64(&mut builder, l);
                                    let rf = i64_to_f64(&mut builder, r);
                                    builder.ins().fcmp(FloatCC::Equal, lf, rf)
                                } else {
                                    let ne = builder.ins().icmp(IntCC::NotEqual, l, r);
                                    let one = builder.ins().iconst(types::I8, 1);
                                    builder.ins().bxor(ne, one)
                                };
                                builder.ins().uextend(types::I64, b)
                            }
                            IrOp::Ne => {
                                let b = if is_float {
                                    let lf = i64_to_f64(&mut builder, l);
                                    let rf = i64_to_f64(&mut builder, r);
                                    builder.ins().fcmp(FloatCC::NotEqual, lf, rf)
                                } else {
                                    builder.ins().icmp(IntCC::NotEqual, l, r)
                                };
                                builder.ins().uextend(types::I64, b)
                            }
                            IrOp::Lt => {
                                let b = if is_float {
                                    let lf = i64_to_f64(&mut builder, l);
                                    let rf = i64_to_f64(&mut builder, r);
                                    builder.ins().fcmp(FloatCC::LessThan, lf, rf)
                                } else {
                                    builder.ins().icmp(IntCC::SignedLessThan, l, r)
                                };
                                builder.ins().uextend(types::I64, b)
                            }
                            IrOp::Le => {
                                let b = if is_float {
                                    let lf = i64_to_f64(&mut builder, l);
                                    let rf = i64_to_f64(&mut builder, r);
                                    builder.ins().fcmp(FloatCC::LessThanOrEqual, lf, rf)
                                } else {
                                    builder.ins().icmp(IntCC::SignedLessThanOrEqual, l, r)
                                };
                                builder.ins().uextend(types::I64, b)
                            }
                            IrOp::Gt => {
                                let b = if is_float {
                                    let lf = i64_to_f64(&mut builder, l);
                                    let rf = i64_to_f64(&mut builder, r);
                                    builder.ins().fcmp(FloatCC::GreaterThan, lf, rf)
                                } else {
                                    builder.ins().icmp(IntCC::SignedGreaterThan, l, r)
                                };
                                builder.ins().uextend(types::I64, b)
                            }
                            IrOp::Ge => {
                                let b = if is_float {
                                    let lf = i64_to_f64(&mut builder, l);
                                    let rf = i64_to_f64(&mut builder, r);
                                    builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lf, rf)
                                } else {
                                    builder
                                        .ins()
                                        .icmp(IntCC::SignedGreaterThanOrEqual, l, r)
                                };
                                builder.ins().uextend(types::I64, b)
                            }
                            IrOp::Neg | IrOp::Not => {
                                return Err(format!("codegen: {op} is unary, not binary"));
                            }
                        };
                        let out_kind = match op {
                            IrOp::Eq
                            | IrOp::Ne
                            | IrOp::Lt
                            | IrOp::Le
                            | IrOp::Gt
                            | IrOp::Ge => ValueKind::Bool,
                            _ => ValueKind::I64,
                        };
                        (v, out_kind)
                    };
                    builder.def_var(value_vars[dest], v);
                    value_kinds.insert(*dest, out_kind);
                }
                IrInstr::Call {
                    dest,
                    func,
                    args,
                } => {
                    let mut arg_vals: Vec<Value> = Vec::with_capacity(args.len() * 2);
                    for a in args {
                        let v = builder.use_var(value_vars[a]);
                        let k = match kind_of(&value_kinds, *a) {
                            ValueKind::Dynamic => match kind_vars.get(a) {
                                Some(kv) => builder.use_var(*kv),
                                None => builder.ins().iconst(types::I64, 0),
                            },
                            other => builder.ins().iconst(types::I64, other.as_i64()),
                        };
                        arg_vals.push(v);
                        arg_vals.push(k);
                    }

                    if let Some((rt_id, out_kind, uses_out_kind)) =
                        json_runtime_call(func, runtime)
                    {
                        // same as file branch below
                        if uses_out_kind {
                            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                8,
                                0,
                            ));
                            let kind_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                            arg_vals.push(kind_ptr);
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            let kind_val =
                                builder.ins().load(types::I64, MemFlags::new(), kind_ptr, 0);
                            let kv = declare_var(&mut builder, &mut next_var);
                            builder.def_var(kv, kind_val);
                            kind_vars.insert(*dest, kv);
                            value_kinds.insert(*dest, ValueKind::Dynamic);
                        } else {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            value_kinds.insert(*dest, out_kind);
                        }
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        file_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                8,
                                0,
                            ));
                            let kind_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                            arg_vals.push(kind_ptr);
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            let kind_val =
                                builder.ins().load(types::I64, MemFlags::new(), kind_ptr, 0);
                            let kv = declare_var(&mut builder, &mut next_var);
                            builder.def_var(kv, kind_val);
                            kind_vars.insert(*dest, kv);
                            value_kinds.insert(*dest, ValueKind::Dynamic);
                        } else if out_kind == ValueKind::None_ {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            builder.ins().call(fref, &arg_vals);
                            let zero = builder.ins().iconst(types::I64, 0);
                            builder.def_var(value_vars[dest], zero);
                            value_kinds.insert(*dest, ValueKind::None_);
                        } else {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            value_kinds.insert(*dest, out_kind);
                        }
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        mmap_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                8,
                                0,
                            ));
                            let kind_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                            arg_vals.push(kind_ptr);
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            let kind_val =
                                builder.ins().load(types::I64, MemFlags::new(), kind_ptr, 0);
                            let kv = declare_var(&mut builder, &mut next_var);
                            builder.def_var(kv, kind_val);
                            kind_vars.insert(*dest, kv);
                            value_kinds.insert(*dest, ValueKind::Dynamic);
                        } else if out_kind == ValueKind::None_ {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            builder.ins().call(fref, &arg_vals);
                            let zero = builder.ins().iconst(types::I64, 0);
                            builder.def_var(value_vars[dest], zero);
                            value_kinds.insert(*dest, ValueKind::None_);
                        } else {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            value_kinds.insert(*dest, out_kind);
                        }
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        coll_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            return Err(format!("codegen: {func} uses out_kind but should not"));
                        } else if out_kind == ValueKind::None_ {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            builder.ins().call(fref, &arg_vals);
                            let zero = builder.ins().iconst(types::I64, 0);
                            builder.def_var(value_vars[dest], zero);
                            value_kinds.insert(*dest, ValueKind::None_);
                        } else {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            value_kinds.insert(*dest, out_kind);
                        }
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        builtin_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                8,
                                0,
                            ));
                            let kind_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                            arg_vals.push(kind_ptr);
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            let kind_val =
                                builder.ins().load(types::I64, MemFlags::new(), kind_ptr, 0);
                            let kv = declare_var(&mut builder, &mut next_var);
                            builder.def_var(kv, kind_val);
                            kind_vars.insert(*dest, kv);
                            value_kinds.insert(*dest, ValueKind::Dynamic);
                        } else {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            value_kinds.insert(*dest, out_kind);
                        }
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        str_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            return Err(format!("codegen: {func} uses out_kind but should not"));
                        }
                        let fref = module.declare_func_in_func(rt_id, &mut builder.func);
                        let call = builder.ins().call(fref, &arg_vals);
                        let payload = builder.inst_results(call)[0];
                        builder.def_var(value_vars[dest], payload);
                        value_kinds.insert(*dest, out_kind);
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        clock_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            return Err(format!("codegen: {func} uses out_kind but should not"));
                        }
                        let fref = module.declare_func_in_func(rt_id, &mut builder.func);
                        let call = builder.ins().call(fref, &arg_vals);
                        let payload = builder.inst_results(call)[0];
                        builder.def_var(value_vars[dest], payload);
                        value_kinds.insert(*dest, out_kind);
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        error_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            return Err(format!("codegen: {func} uses out_kind but should not"));
                        }
                        let fref = module.declare_func_in_func(rt_id, &mut builder.func);
                        let call = builder.ins().call(fref, &arg_vals);
                        let payload = builder.inst_results(call)[0];
                        builder.def_var(value_vars[dest], payload);
                        value_kinds.insert(*dest, out_kind);
                    } else if let Some((rt_id, out_kind, uses_out_kind)) =
                        input_runtime_call(func, runtime)
                    {
                        if uses_out_kind {
                            return Err(format!("codegen: {func} uses out_kind but should not"));
                        } else if out_kind == ValueKind::None_ {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            builder.ins().call(fref, &arg_vals);
                            let zero = builder.ins().iconst(types::I64, 0);
                            builder.def_var(value_vars[dest], zero);
                            value_kinds.insert(*dest, ValueKind::None_);
                        } else {
                            let fref =
                                module.declare_func_in_func(rt_id, &mut builder.func);
                            let call = builder.ins().call(fref, &arg_vals);
                            let payload = builder.inst_results(call)[0];
                            builder.def_var(value_vars[dest], payload);
                            value_kinds.insert(*dest, out_kind);
                        }
                    } else {
                        let id = func_ids.get(func).copied().ok_or_else(|| {
                            crate::error::format_error(
                                crate::error::ErrorKind::Runtime,
                                0,
                                &format!("undefined function '{func}'"),
                            )
                        })?;
                        let fref = module.declare_func_in_func(id, &mut builder.func);
                        let call = builder.ins().call(fref, &arg_vals);
                        let results = builder.inst_results(call).to_vec();
                        builder.def_var(value_vars[dest], results[0]);
                        if results.len() > 1 {
                            let kv = declare_var(&mut builder, &mut next_var);
                            builder.def_var(kv, results[1]);
                            kind_vars.insert(*dest, kv);
                            value_kinds.insert(*dest, ValueKind::Dynamic);
                        } else {
                            value_kinds.insert(*dest, ValueKind::I64);
                        }
                    }
                }
                IrInstr::MakeList { dest, items } => {
                    let fnew =
                        module.declare_func_in_func(runtime.list_new, &mut builder.func);
                    let call = builder.ins().call(fnew, &[]);
                    let list = builder.inst_results(call)[0];
                    builder.def_var(value_vars[dest], list);
                    value_kinds.insert(*dest, ValueKind::List);

                    let fpush =
                        module.declare_func_in_func(runtime.list_push, &mut builder.func);
                    for item in items {
                        let val = builder.use_var(value_vars[item]);
                        let kind = match kind_of(&value_kinds, *item) {
                            ValueKind::Dynamic => builder.use_var(kind_vars[item]),
                            other => builder.ins().iconst(types::I64, other.as_i64()),
                        };
                        builder.ins().call(fpush, &[list, val, kind]);
                    }
                }
                IrInstr::MakeDict { dest, entries } => {
                    let fnew =
                        module.declare_func_in_func(runtime.dict_new, &mut builder.func);
                    let call = builder.ins().call(fnew, &[]);
                    let dict = builder.inst_results(call)[0];
                    builder.def_var(value_vars[dest], dict);
                    value_kinds.insert(*dest, ValueKind::Dict);

                    let fpush =
                        module.declare_func_in_func(runtime.dict_push, &mut builder.func);
                    for (k, v) in entries {
                        let key = builder.use_var(value_vars[k]);
                        let key_kind = match kind_of(&value_kinds, *k) {
                            ValueKind::Dynamic => builder.use_var(kind_vars[k]),
                            other => builder.ins().iconst(types::I64, other.as_i64()),
                        };
                        let val = builder.use_var(value_vars[v]);
                        let val_kind = match kind_of(&value_kinds, *v) {
                            ValueKind::Dynamic => builder.use_var(kind_vars[v]),
                            other => builder.ins().iconst(types::I64, other.as_i64()),
                        };
                        builder
                            .ins()
                            .call(fpush, &[dict, key, key_kind, val, val_kind]);
                    }
                }
                IrInstr::IndexGet {
                    dest,
                    object,
                    index,
                } => {
                    let obj = builder.use_var(value_vars[object]);
                    let idx = builder.use_var(value_vars[index]);
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        8,
                        0,
                    ));
                    let kind_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                    let payload = match kind_of(&value_kinds, *object) {
                        ValueKind::Dict => {
                            let key_kind = match kind_of(&value_kinds, *index) {
                                ValueKind::Dynamic => builder.use_var(kind_vars[index]),
                                other => builder.ins().iconst(types::I64, other.as_i64()),
                            };
                            let fref = module
                                .declare_func_in_func(runtime.dict_get, &mut builder.func);
                            let call =
                                builder.ins().call(fref, &[obj, idx, key_kind, kind_ptr]);
                            builder.inst_results(call)[0]
                        }
                        ValueKind::Dynamic => {
                            let obj_kind = builder.use_var(kind_vars[object]);
                            let key_kind = match kind_of(&value_kinds, *index) {
                                ValueKind::Dynamic => builder.use_var(kind_vars[index]),
                                other => builder.ins().iconst(types::I64, other.as_i64()),
                            };
                            let fref = module
                                .declare_func_in_func(runtime.index_get, &mut builder.func);
                            let call = builder.ins().call(
                                fref,
                                &[obj, obj_kind, idx, key_kind, kind_ptr],
                            );
                            builder.inst_results(call)[0]
                        }
                        _ => {
                            let fref = module
                                .declare_func_in_func(runtime.list_get, &mut builder.func);
                            let call = builder.ins().call(fref, &[obj, idx, kind_ptr]);
                            builder.inst_results(call)[0]
                        }
                    };
                    builder.def_var(value_vars[dest], payload);
                    let kind_val = builder.ins().load(types::I64, MemFlags::new(), kind_ptr, 0);
                    let kv = declare_var(&mut builder, &mut next_var);
                    builder.def_var(kv, kind_val);
                    kind_vars.insert(*dest, kv);
                    value_kinds.insert(*dest, ValueKind::Dynamic);
                }
                IrInstr::IndexSet {
                    object,
                    index,
                    value,
                } => {
                    let obj = builder.use_var(value_vars[object]);
                    let idx = builder.use_var(value_vars[index]);
                    let val = builder.use_var(value_vars[value]);
                    let val_kind = match kind_of(&value_kinds, *value) {
                        ValueKind::Dynamic => builder.use_var(kind_vars[value]),
                        other => builder.ins().iconst(types::I64, other.as_i64()),
                    };
                    match kind_of(&value_kinds, *object) {
                        ValueKind::Dict => {
                            let key_kind = match kind_of(&value_kinds, *index) {
                                ValueKind::Dynamic => builder.use_var(kind_vars[index]),
                                other => builder.ins().iconst(types::I64, other.as_i64()),
                            };
                            let fref = module
                                .declare_func_in_func(runtime.dict_set, &mut builder.func);
                            builder
                                .ins()
                                .call(fref, &[obj, idx, key_kind, val, val_kind]);
                        }
                        ValueKind::Dynamic => {
                            let obj_kind = builder.use_var(kind_vars[object]);
                            let key_kind = match kind_of(&value_kinds, *index) {
                                ValueKind::Dynamic => builder.use_var(kind_vars[index]),
                                other => builder.ins().iconst(types::I64, other.as_i64()),
                            };
                            let fref = module
                                .declare_func_in_func(runtime.index_set, &mut builder.func);
                            builder.ins().call(
                                fref,
                                &[obj, obj_kind, idx, key_kind, val, val_kind],
                            );
                        }
                        _ => {
                            let fref = module
                                .declare_func_in_func(runtime.list_set, &mut builder.func);
                            builder.ins().call(fref, &[obj, idx, val, val_kind]);
                        }
                    }
                }
                IrInstr::ListLen { dest, list } => {
                    let l = builder.use_var(value_vars[list]);
                    let fref =
                        module.declare_func_in_func(runtime.list_len, &mut builder.func);
                    let call = builder.ins().call(fref, &[l]);
                    let ret = builder.inst_results(call)[0];
                    builder.def_var(value_vars[dest], ret);
                    value_kinds.insert(*dest, ValueKind::I64);
                }
                IrInstr::ValueToStr { dest, src } => {
                    let src_kind = kind_of(&value_kinds, *src);
                    if src_kind == ValueKind::Str {
                        // Already a string pointer — skip malloc via value_to_str.
                        let v = builder.use_var(value_vars[src]);
                        builder.def_var(value_vars[dest], v);
                        value_kinds.insert(*dest, ValueKind::Str);
                    } else {
                        let v = builder.use_var(value_vars[src]);
                        let kind = match src_kind {
                            ValueKind::Dynamic => builder.use_var(kind_vars[src]),
                            other => builder.ins().iconst(types::I64, other.as_i64()),
                        };
                        let fref =
                            module.declare_func_in_func(runtime.value_to_str, &mut builder.func);
                        let call = builder.ins().call(fref, &[v, kind]);
                        let ret = builder.inst_results(call)[0];
                        builder.def_var(value_vars[dest], ret);
                        value_kinds.insert(*dest, ValueKind::Str);
                    }
                }
                IrInstr::StrConcat { dest, left, right } => {
                    let l = builder.use_var(value_vars[left]);
                    let r = builder.use_var(value_vars[right]);
                    let (c_l, c_r) = concat_consume[idx];
                    let consume_l = builder.ins().iconst(types::I64, c_l as i64);
                    let consume_r = builder.ins().iconst(types::I64, c_r as i64);
                    let fref =
                        module.declare_func_in_func(runtime.str_concat, &mut builder.func);
                    let call = builder.ins().call(fref, &[l, r, consume_l, consume_r]);
                    let ret = builder.inst_results(call)[0];
                    builder.def_var(value_vars[dest], ret);
                    value_kinds.insert(*dest, ValueKind::Str);
                }
                IrInstr::MakeStruct { dest, nfields } => {
                    let n = builder.ins().iconst(types::I64, *nfields as i64);
                    let fref =
                        module.declare_func_in_func(runtime.struct_new, &mut builder.func);
                    let call = builder.ins().call(fref, &[n]);
                    let ret = builder.inst_results(call)[0];
                    builder.def_var(value_vars[dest], ret);
                    value_kinds.insert(*dest, ValueKind::Struct);
                }
                IrInstr::StructGet {
                    dest,
                    object,
                    field,
                } => {
                    let obj = builder.use_var(value_vars[object]);
                    let idx = builder.ins().iconst(types::I64, *field as i64);
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        8,
                        0,
                    ));
                    let kind_ptr = builder.ins().stack_addr(types::I64, slot, 0);
                    let fref =
                        module.declare_func_in_func(runtime.struct_get, &mut builder.func);
                    let call = builder.ins().call(fref, &[obj, idx, kind_ptr]);
                    let payload = builder.inst_results(call)[0];
                    builder.def_var(value_vars[dest], payload);
                    let kind_val = builder.ins().load(types::I64, MemFlags::new(), kind_ptr, 0);
                    let kv = declare_var(&mut builder, &mut next_var);
                    builder.def_var(kv, kind_val);
                    kind_vars.insert(*dest, kv);
                    value_kinds.insert(*dest, ValueKind::Dynamic);
                }
                IrInstr::StructSet {
                    object,
                    field,
                    value,
                } => {
                    let obj = builder.use_var(value_vars[object]);
                    let idx = builder.ins().iconst(types::I64, *field as i64);
                    let val = builder.use_var(value_vars[value]);
                    let val_kind = match kind_of(&value_kinds, *value) {
                        ValueKind::Dynamic => builder.use_var(kind_vars[value]),
                        other => builder.ins().iconst(types::I64, other.as_i64()),
                    };
                    let fref =
                        module.declare_func_in_func(runtime.struct_set, &mut builder.func);
                    builder.ins().call(fref, &[obj, idx, val, val_kind]);
                }
                IrInstr::Print { args } => {
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            let sep_ref = module
                                .declare_func_in_func(runtime.print_sep, &mut builder.func);
                            builder.ins().call(sep_ref, &[]);
                        }
                        let v = builder.use_var(value_vars[a]);
                        match kind_of(&value_kinds, *a) {
                            ValueKind::Dynamic => {
                                let k = builder.use_var(kind_vars[a]);
                                let fref = module.declare_func_in_func(
                                    runtime.print_value,
                                    &mut builder.func,
                                );
                                builder.ins().call(fref, &[v, k]);
                            }
                            ValueKind::F64 => {
                                let f = i64_to_f64(&mut builder, v);
                                let fref = module
                                    .declare_func_in_func(runtime.print_f64, &mut builder.func);
                                builder.ins().call(fref, &[f]);
                            }
                            ValueKind::Str => {
                                let fref = module
                                    .declare_func_in_func(runtime.print_str, &mut builder.func);
                                builder.ins().call(fref, &[v]);
                            }
                            ValueKind::List => {
                                let fref = module
                                    .declare_func_in_func(runtime.print_list, &mut builder.func);
                                builder.ins().call(fref, &[v]);
                            }
                            ValueKind::Dict => {
                                let fref = module
                                    .declare_func_in_func(runtime.print_dict, &mut builder.func);
                                builder.ins().call(fref, &[v]);
                            }
                            ValueKind::Struct => {
                                let fref = module
                                    .declare_func_in_func(runtime.print_struct, &mut builder.func);
                                builder.ins().call(fref, &[v]);
                            }
                            ValueKind::File => {
                                let k = builder.ins().iconst(types::I64, ValueKind::File.as_i64());
                                let fref = module.declare_func_in_func(
                                    runtime.print_value,
                                    &mut builder.func,
                                );
                                builder.ins().call(fref, &[v, k]);
                            }
                            ValueKind::Mmap => {
                                let k = builder.ins().iconst(types::I64, ValueKind::Mmap.as_i64());
                                let fref = module.declare_func_in_func(
                                    runtime.print_value,
                                    &mut builder.func,
                                );
                                builder.ins().call(fref, &[v, k]);
                            }
                            kind @ (ValueKind::Bool | ValueKind::None_) => {
                                let k =
                                    builder.ins().iconst(types::I64, kind.as_i64());
                                let fref = module.declare_func_in_func(
                                    runtime.print_value,
                                    &mut builder.func,
                                );
                                builder.ins().call(fref, &[v, k]);
                            }
                            ValueKind::I64 => {
                                let fref = module
                                    .declare_func_in_func(runtime.print_i64, &mut builder.func);
                                builder.ins().call(fref, &[v]);
                            }
                            ValueKind::U64 => {
                                let k = builder
                                    .ins()
                                    .iconst(types::I64, ValueKind::U64.as_i64());
                                let fref = module.declare_func_in_func(
                                    runtime.print_value,
                                    &mut builder.func,
                                );
                                builder.ins().call(fref, &[v, k]);
                            }
                        }
                    }
                    let nl_ref =
                        module.declare_func_in_func(runtime.print_nl, &mut builder.func);
                    builder.ins().call(nl_ref, &[]);
                }
                IrInstr::Return { value } => {
                    let v = match value {
                        Some(id) => builder.use_var(value_vars[id]),
                        None => builder.ins().iconst(types::I64, 0),
                    };
                    if returns_kind {
                        let k = match value {
                            Some(id) => match kind_of(&value_kinds, *id) {
                                ValueKind::Dynamic => match kind_vars.get(id) {
                                    Some(kv) => builder.use_var(*kv),
                                    None => builder.ins().iconst(types::I64, 0),
                                },
                                other => {
                                    builder.ins().iconst(types::I64, other.as_i64())
                                }
                            },
                            None => builder
                                .ins()
                                .iconst(types::I64, ValueKind::None_.as_i64()),
                        };
                        builder.ins().return_(&[v, k]);
                    } else {
                        builder.ins().return_(&[v]);
                    }
                    terminated = true;
                }
                IrInstr::Jump { target } => {
                    let b = blocks[target];
                    builder.ins().jump(b, &[]);
                    terminated = true;
                }
                IrInstr::Branch {
                    cond,
                    then_block,
                    else_block,
                } => {
                    let c = builder.use_var(value_vars[cond]);
                    let t = blocks[then_block];
                    let e = blocks[else_block];
                    builder.ins().brif(c, t, &[], e, &[]);
                    terminated = true;
                }
                IrInstr::ParallelRange {
                    start,
                    end,
                    worker,
                } => {
                    let worker_id = func_ids.get(worker).copied().ok_or_else(|| {
                        format!("codegen: parallel worker '{worker}' not declared")
                    })?;
                    let worker_ref = module.declare_func_in_func(worker_id, &mut builder.func);
                    let worker_ptr = builder.ins().func_addr(types::I64, worker_ref);
                    let s = builder.use_var(value_vars[start]);
                    let e = builder.use_var(value_vars[end]);
                    let cal = module.declare_func_in_func(runtime.parallel_for, &mut builder.func);
                    builder.ins().call(cal, &[s, e, worker_ptr]);
                }
            }
        }

        if !terminated {
            let zero = builder.ins().iconst(types::I64, 0);
            if returns_kind {
                // Falling off the end yields None, matching the interpreter.
                let none = builder
                    .ins()
                    .iconst(types::I64, ValueKind::None_.as_i64());
                builder.ins().return_(&[zero, none]);
            } else {
                builder.ins().return_(&[zero]);
            }
        }

        builder.seal_all_blocks();
        builder.finalize();
    }

    module
        .define_function(func_id, ctx)
        .map_err(|e| format!("define function: {e}"))?;
    module.clear_context(ctx);
    Ok(())
}
