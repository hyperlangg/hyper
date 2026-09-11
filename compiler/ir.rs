use std::fmt;

pub type ValueId = u32;
pub type BlockId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum IrOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrInstr {
    ConstI64 { dest: ValueId, value: i64 },
    ConstF64 { dest: ValueId, value: f64 },
    ConstBool { dest: ValueId, value: bool },
    ConstStr { dest: ValueId, value: String },
    ConstNone { dest: ValueId },
    Load { dest: ValueId, name: String },
    Store { name: String, value: ValueId },
    Unary { dest: ValueId, op: IrOp, src: ValueId },
    Binary {
        dest: ValueId,
        op: IrOp,
        left: ValueId,
        right: ValueId,
    },
    /// Truncate/sign-extend or zero-extend an i64 payload to a fixed integer width.
    IntWrap {
        dest: ValueId,
        src: ValueId,
        bits: u8,
        signed: bool,
    },
    /// Stop with a runtime error when an integer divisor is zero.
    GuardDivisor { value: ValueId, line: u32 },
    Call {
        dest: ValueId,
        func: String,
        args: Vec<ValueId>,
    },
    MakeList {
        dest: ValueId,
        items: Vec<ValueId>,
    },
    MakeDict {
        dest: ValueId,
        entries: Vec<(ValueId, ValueId)>,
    },
    IndexGet {
        dest: ValueId,
        object: ValueId,
        index: ValueId,
    },
    IndexSet {
        object: ValueId,
        index: ValueId,
        value: ValueId,
    },
    ListLen {
        dest: ValueId,
        list: ValueId,
    },
    ValueToStr {
        dest: ValueId,
        src: ValueId,
    },
    StrConcat {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    MakeStruct {
        dest: ValueId,
        nfields: u32,
    },
    StructGet {
        dest: ValueId,
        object: ValueId,
        field: u32,
    },
    StructSet {
        object: ValueId,
        field: u32,
        value: ValueId,
    },
    Print { args: Vec<ValueId> },
    Return { value: Option<ValueId> },
    Jump { target: BlockId },
    Branch {
        cond: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Label { block: BlockId },
    /// Run `worker(i)` for `i` in `[start, end)` on a thread pool (AOT runtime).
    /// `worker` is a Hyper function with a single integer parameter (pair ABI).
    ParallelRange {
        start: ValueId,
        end: ValueId,
        worker: String,
    },
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<IrInstr>,
}

#[derive(Debug, Clone)]
pub struct IrModule {
    pub functions: Vec<IrFunction>,
    pub main: Vec<IrInstr>,
}

impl fmt::Display for IrOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            IrOp::Add => "add",
            IrOp::Sub => "sub",
            IrOp::Mul => "mul",
            IrOp::Div => "div",
            IrOp::FloorDiv => "floordiv",
            IrOp::Rem => "rem",
            IrOp::Pow => "pow",
            IrOp::Eq => "eq",
            IrOp::Ne => "ne",
            IrOp::Lt => "lt",
            IrOp::Le => "le",
            IrOp::Gt => "gt",
            IrOp::Ge => "ge",
            IrOp::Neg => "neg",
            IrOp::Not => "not",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for IrInstr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrInstr::ConstI64 { dest, value } => write!(f, "  v{} = const.i64 {}", dest, value),
            IrInstr::ConstF64 { dest, value } => write!(f, "  v{} = const.f64 {}", dest, value),
            IrInstr::ConstBool { dest, value } => write!(f, "  v{} = const.bool {}", dest, value),
            IrInstr::ConstStr { dest, value } => {
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                write!(f, "  v{} = const.str \"{}\"", dest, escaped)
            }
            IrInstr::ConstNone { dest } => write!(f, "  v{} = const.none", dest),
            IrInstr::Load { dest, name } => write!(f, "  v{} = load {}", dest, name),
            IrInstr::Store { name, value } => write!(f, "  store {} v{}", name, value),
            IrInstr::Unary { dest, op, src } => write!(f, "  v{} = {} v{}", dest, op, src),
            IrInstr::Binary {
                dest,
                op,
                left,
                right,
            } => write!(f, "  v{} = {} v{} v{}", dest, op, left, right),
            IrInstr::IntWrap {
                dest,
                src,
                bits,
                signed,
            } => write!(
                f,
                "  v{} = int_wrap.{}{} v{}",
                dest,
                if *signed { "i" } else { "u" },
                bits,
                src
            ),
            IrInstr::GuardDivisor { value, line } => {
                write!(f, "  guard.divisor v{} line {}", value, line)
            }
            IrInstr::Call { dest, func, args } => {
                if args.is_empty() {
                    write!(f, "  v{} = call {}()", dest, func)
                } else {
                    let args_str: Vec<String> = args.iter().map(|a| format!("v{}", a)).collect();
                    write!(f, "  v{} = call {}({})", dest, func, args_str.join(", "))
                }
            }
            IrInstr::MakeList { dest, items } => {
                if items.is_empty() {
                    write!(f, "  v{} = make_list()", dest)
                } else {
                    let items_str: Vec<String> =
                        items.iter().map(|a| format!("v{}", a)).collect();
                    write!(f, "  v{} = make_list({})", dest, items_str.join(", "))
                }
            }
            IrInstr::MakeDict { dest, entries } => {
                if entries.is_empty() {
                    write!(f, "  v{} = make_dict()", dest)
                } else {
                    let entries_str: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("v{}:v{}", k, v))
                        .collect();
                    write!(f, "  v{} = make_dict({})", dest, entries_str.join(", "))
                }
            }
            IrInstr::IndexGet {
                dest,
                object,
                index,
            } => write!(f, "  v{} = index_get v{}[v{}]", dest, object, index),
            IrInstr::IndexSet {
                object,
                index,
                value,
            } => write!(f, "  index_set v{}[v{}] = v{}", object, index, value),
            IrInstr::ListLen { dest, list } => write!(f, "  v{} = list_len v{}", dest, list),
            IrInstr::ValueToStr { dest, src } => write!(f, "  v{} = value_to_str v{}", dest, src),
            IrInstr::StrConcat { dest, left, right } => {
                write!(f, "  v{} = str_concat v{} v{}", dest, left, right)
            }
            IrInstr::MakeStruct { dest, nfields } => {
                write!(f, "  v{} = make_struct nfields={}", dest, nfields)
            }
            IrInstr::StructGet {
                dest,
                object,
                field,
            } => write!(f, "  v{} = struct_get v{}[{}]", dest, object, field),
            IrInstr::StructSet {
                object,
                field,
                value,
            } => write!(f, "  struct_set v{}[{}] = v{}", object, field, value),
            IrInstr::Print { args } => {
                let args_str: Vec<String> = args.iter().map(|a| format!("v{}", a)).collect();
                write!(f, "  print {}", args_str.join(", "))
            }
            IrInstr::Return { value } => match value {
                Some(v) => write!(f, "  return v{}", v),
                None => write!(f, "  return"),
            },
            IrInstr::Jump { target } => write!(f, "  jump b{}", target),
            IrInstr::Branch {
                cond,
                then_block,
                else_block,
            } => write!(
                f,
                "  branch v{} then b{} else b{}",
                cond, then_block, else_block
            ),
            IrInstr::Label { block } => write!(f, "b{}:", block),
            IrInstr::ParallelRange {
                start,
                end,
                worker,
            } => write!(
                f,
                "  parallel_range v{}..v{} worker {}",
                start, end, worker
            ),
        }
    }
}

impl fmt::Display for IrFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {}({}) {{", self.name, self.params.join(", "))?;
        for instr in &self.body {
            writeln!(f, "{}", instr)?;
        }
        write!(f, "}}")
    }
}

impl fmt::Display for IrModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for func in &self.functions {
            writeln!(f, "{}", func)?;
            writeln!(f)?;
        }
        writeln!(f, "fn __main__() {{")?;
        for instr in &self.main {
            writeln!(f, "{}", instr)?;
        }
        write!(f, "}}")
    }
}
