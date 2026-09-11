use crate::ast::*;
use crate::driver;
use crate::error::{self, ErrorKind};
use std::collections::{HashMap, HashSet};
use std::process;

#[derive(Debug, Clone, PartialEq)]
pub enum HyperType {
    None,
    Bool,
    String,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    List(Box<HyperType>),
    Dict,
    Array(Box<HyperType>),
    Function {
        params: Vec<HyperType>,
        ret: Box<HyperType>,
    },
    Struct(String),
    Trait(String),
    Mmap,
    File,
    Any,
}

#[derive(Debug, Clone)]
struct Binding {
    ty: HyperType,
    mutable: bool,
}

struct Scope {
    bindings: HashMap<String, Binding>,
    /// Names declared without a type annotation, whose numeric type may widen.
    inferred: HashSet<String>,
}

#[derive(Debug, Clone)]
struct StructFieldInfo {
    ty: HyperType,
    #[allow(dead_code)]
    is_pub: bool,
    is_mut: bool,
}

struct TypeChecker {
    scopes: Vec<Scope>,
    errors: Vec<String>,
    expected_return: Option<HyperType>,
    /// Struct name → field metadata (types + pub/mut).
    structs: HashMap<String, HashMap<String, StructFieldInfo>>,
    /// Trait name → method signatures a struct must provide to implement it.
    traits: HashMap<String, Vec<MethodSig>>,
    /// Enclosing loops, so `break` / `continue` can be rejected outside one.
    loop_depth: u32,
    /// Nested `handle … else …` expressions that may recover from `raise`.
    handle_depth: u32,
    /// Current function is marked `raises` (may contain bare `raise`).
    allows_raise: bool,
}

impl TypeChecker {
    fn new() -> Self {
        let mut tc = TypeChecker {
            scopes: Vec::new(),
            errors: Vec::new(),
            expected_return: None,
            structs: HashMap::new(),
            traits: HashMap::new(),
            loop_depth: 0,
            handle_depth: 0,
            allows_raise: false,
        };
        tc.push_scope();
        // Builtins
        tc.define(
            "print",
            Binding {
                ty: HyperType::Function {
                    params: vec![HyperType::Any],
                    ret: Box::new(HyperType::None),
                },
                mutable: false,
            },
        );
        tc.define(
            "input",
            Binding {
                ty: HyperType::Function {
                    params: vec![HyperType::Any],
                    ret: Box::new(HyperType::String),
                },
                mutable: false,
            },
        );
        tc.define(
            "clock",
            Binding {
                ty: HyperType::Function {
                    params: vec![],
                    ret: Box::new(HyperType::F64),
                },
                mutable: false,
            },
        );
        tc.define(
            "open",
            Binding {
                ty: HyperType::Function {
                    params: vec![HyperType::Any],
                    ret: Box::new(HyperType::File),
                },
                mutable: false,
            },
        );
        // Python-like builtins (compile path). Soft arity via single Any where needed.
        let any1 = |ret: HyperType| Binding {
            ty: HyperType::Function {
                params: vec![HyperType::Any],
                ret: Box::new(ret),
            },
            mutable: false,
        };
        let any2 = |ret: HyperType| Binding {
            ty: HyperType::Function {
                params: vec![HyperType::Any, HyperType::Any],
                ret: Box::new(ret),
            },
            mutable: false,
        };
        tc.define("len", any1(HyperType::I64));
        tc.define("abs", any1(HyperType::Any));
        tc.define("min", any1(HyperType::Any));
        tc.define("max", any1(HyperType::Any));
        tc.define("sum", any1(HyperType::Any));
        tc.define("round", any1(HyperType::Any));
        tc.define("pow", any2(HyperType::Any));
        tc.define("divmod", any2(HyperType::List(Box::new(HyperType::Any))));
        tc.define("chr", any1(HyperType::String));
        tc.define("ord", any1(HyperType::I64));
        tc.define("bin", any1(HyperType::String));
        tc.define("hex", any1(HyperType::String));
        tc.define("oct", any1(HyperType::String));
        tc.define("int", any1(HyperType::I64));
        tc.define("float", any1(HyperType::F64));
        tc.define("str", any1(HyperType::String));
        tc.define("bool", any1(HyperType::Bool));
        tc.define("all", any1(HyperType::Bool));
        tc.define("any", any1(HyperType::Bool));
        tc.define("sorted", any1(HyperType::List(Box::new(HyperType::Any))));
        tc.define("reversed", any1(HyperType::List(Box::new(HyperType::Any))));
        tc.define("enumerate", any1(HyperType::List(Box::new(HyperType::Any))));
        tc.define("zip", any1(HyperType::List(Box::new(HyperType::Any))));
        tc.define("list", any1(HyperType::List(Box::new(HyperType::Any))));
        tc.define("range", any1(HyperType::List(Box::new(HyperType::I64))));
        tc.define("repr", any1(HyperType::String));
        tc
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            bindings: HashMap::new(),
            inferred: HashSet::new(),
        });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str, binding: Binding) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(name.to_string(), binding);
        }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.bindings.get(name) {
                return Some(b);
            }
        }
        None
    }

    /// Let an inferred numeric variable adopt a wider type instead of failing:
    /// `let mut sum = 0` must accept an i64 coming out of `range`.
    fn widen_inferred(&mut self, name: &str, ty: &HyperType) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.bindings.get_mut(name) {
                if !scope.inferred.contains(name) {
                    return false;
                }
                binding.ty = ty.clone();
                return true;
            }
        }
        false
    }

    fn syntax_error(&mut self, line: u32, message: impl Into<String>) {
        self.errors
            .push(error::format_error(ErrorKind::Syntax, line, &message.into()));
    }

    fn error(&mut self, msg: String) {
        self.syntax_error(0, msg);
    }

    fn resolve_type_name(&self, name: &str) -> HyperType {
        if let Some(inner) = name
            .strip_prefix("Array[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            return HyperType::Array(Box::new(self.resolve_type_name(inner.trim())));
        }
        if let Some(rest) = name
            .strip_prefix("Dict[")
            .and_then(|body| body.strip_suffix(']'))
        {
            if let Some((_key, val)) = rest.split_once(',') {
                let _ = self.resolve_type_name(val.trim());
            }
            return HyperType::Dict;
        }

        let ty = match name {
            "int8" => "i8",
            "int16" => "i16",
            "int32" => "i32",
            "int64" => "i64",
            "uint8" => "u8",
            "uint16" => "u16",
            "uint32" => "u32",
            "uint64" => "u64",
            "float32" => "f32",
            "float64" => "f64",
            "boolean" => "bool",
            other => other,
        };
        match ty {
            "None" | "none" => HyperType::None,
            "bool" => HyperType::Bool,
            "string" | "str" => HyperType::String,
            "i8" => HyperType::I8,
            "i16" => HyperType::I16,
            "i32" => HyperType::I32,
            "i64" => HyperType::I64,
            "u8" => HyperType::U8,
            "u16" => HyperType::U16,
            "u32" => HyperType::U32,
            "u64" => HyperType::U64,
            "f32" => HyperType::F32,
            "f64" => HyperType::F64,
            "any" | "Any" => HyperType::Any,
            "list" | "List" => HyperType::List(Box::new(HyperType::Any)),
            "dict" | "Dict" => HyperType::Dict,
            "mmap" | "Mmap" => HyperType::Mmap,
            "file" | "File" => HyperType::File,
            other => {
                if self.structs.contains_key(other) {
                    HyperType::Struct(other.to_string())
                } else if self.traits.contains_key(other) {
                    HyperType::Trait(other.to_string())
                } else {
                    // Unknown named type — treat softly as Any for forward compat.
                    HyperType::Any
                }
            }
        }
    }

    fn type_ann_to_hyper(&self, ann: &TypeAnn) -> HyperType {
        match ann {
            TypeAnn::None => HyperType::Any,
            TypeAnn::Named(name) => self.resolve_type_name(name),
            TypeAnn::Array { inner } => HyperType::Array(Box::new(self.resolve_type_name(inner))),
            TypeAnn::Dict { .. } => HyperType::Dict,
        }
    }

    fn is_numeric(ty: &HyperType) -> bool {
        matches!(
            ty,
            HyperType::I8
                | HyperType::I16
                | HyperType::I32
                | HyperType::I64
                | HyperType::U8
                | HyperType::U16
                | HyperType::U32
                | HyperType::U64
                | HyperType::F32
                | HyperType::F64
                | HyperType::Any
        )
    }

    fn is_boolish(ty: &HyperType) -> bool {
        matches!(ty, HyperType::Bool | HyperType::Any | HyperType::None)
            || Self::is_numeric(ty)
            || matches!(ty, HyperType::String)
    }

    fn numeric_rank(ty: &HyperType) -> Option<u8> {
        match ty {
            HyperType::I8 | HyperType::U8 => Some(1),
            HyperType::I16 | HyperType::U16 => Some(2),
            HyperType::I32 | HyperType::U32 => Some(3),
            HyperType::I64 | HyperType::U64 => Some(4),
            HyperType::F32 => Some(5),
            HyperType::F64 => Some(6),
            HyperType::Any => Some(0),
            _ => None,
        }
    }

    fn widen_numeric(a: &HyperType, b: &HyperType) -> HyperType {
        if matches!(a, HyperType::Any) {
            return b.clone();
        }
        if matches!(b, HyperType::Any) {
            return a.clone();
        }
        let ra = Self::numeric_rank(a).unwrap_or(0);
        let rb = Self::numeric_rank(b).unwrap_or(0);
        if ra >= rb {
            a.clone()
        } else {
            b.clone()
        }
    }

    fn is_compatible(dest: &HyperType, src: &HyperType) -> bool {
        if matches!(dest, HyperType::Any) || matches!(src, HyperType::Any) {
            return true;
        }
        if dest == src {
            return true;
        }
        // Numeric widening: source rank <= dest rank, or float destination.
        if let (Some(rd), Some(rs)) = (Self::numeric_rank(dest), Self::numeric_rank(src)) {
            return rs <= rd;
        }
        // List with Any element accepts any list.
        match (dest, src) {
            (HyperType::F32, HyperType::F64) => true,
            (HyperType::List(d), HyperType::List(s)) => {
                matches!(d.as_ref(), HyperType::Any) || Self::is_compatible(d, s)
            }
            (HyperType::Array(d), HyperType::Array(s)) => {
                matches!(d.as_ref(), HyperType::Any) || Self::is_compatible(d, s)
            }
            (HyperType::Array(d), HyperType::List(s)) => {
                matches!(d.as_ref(), HyperType::Any) || Self::is_compatible(d, s)
            }
            (HyperType::Dict, HyperType::Dict) => true,
            (HyperType::Struct(a), HyperType::Struct(b)) => a == b,
            _ => false,
        }
    }

    fn expr_fits_type(expr: &Expr, dest: &HyperType) -> bool {
        match (expr, dest) {
            (Expr::Literal(Literal::None), HyperType::None) => true,
            (Expr::Literal(Literal::Number(n)), HyperType::F32) => n.parse::<f64>().is_ok(),
            (Expr::Literal(Literal::Number(n)), HyperType::F64) => {
                n.parse::<f64>().is_ok()
            }
            (Expr::Literal(Literal::Number(n)), ty) if Self::is_numeric(ty) => {
                if matches!(
                    ty,
                    HyperType::U8 | HyperType::U16 | HyperType::U32 | HyperType::U64
                ) {
                    Self::parse_uint_literal(n).is_some_and(|v| Self::uint_fits(v, ty))
                } else {
                    Self::parse_int_literal(n).is_some_and(|v| Self::int_fits(v, ty))
                }
            }
            (
                Expr::Unary {
                    op: UnaryOp::Neg,
                    right,
                },
                ty,
            ) if Self::is_numeric(ty) => match right.as_ref() {
                Expr::Literal(Literal::Number(n)) => Self::parse_int_literal(n)
                    .and_then(|v| v.checked_neg())
                    .is_some_and(|v| Self::int_fits(v, ty)),
                _ => false,
            },
            _ => false,
        }
    }

    fn parse_int_literal(text: &str) -> Option<i64> {
        text.replace('_', "").parse::<i64>().ok()
    }

    fn int_fits(value: i64, ty: &HyperType) -> bool {
        match ty {
            HyperType::I8 => i8::MIN as i64 <= value && value <= i8::MAX as i64,
            HyperType::I16 => i16::MIN as i64 <= value && value <= i16::MAX as i64,
            HyperType::I32 => i32::MIN as i64 <= value && value <= i32::MAX as i64,
            HyperType::I64 => true,
            HyperType::U8 => 0 <= value && value <= u8::MAX as i64,
            HyperType::U16 => 0 <= value && value <= u16::MAX as i64,
            HyperType::U32 => 0 <= value && value <= u32::MAX as i64,
            HyperType::U64 => value >= 0,
            _ => false,
        }
    }

    fn parse_uint_literal(text: &str) -> Option<u64> {
        text.replace('_', "").parse::<u64>().ok()
    }

    fn uint_fits(value: u64, ty: &HyperType) -> bool {
        match ty {
            HyperType::U8 => value <= u64::from(u8::MAX),
            HyperType::U16 => value <= u64::from(u16::MAX),
            HyperType::U32 => value <= u64::from(u32::MAX),
            HyperType::U64 => true,
            _ => false,
        }
    }

    fn infer_literal(lit: &Literal) -> HyperType {
        match lit {
            Literal::None => HyperType::None,
            Literal::Bool(_) => HyperType::Bool,
            Literal::String(_) => HyperType::String,
            Literal::Number(n) => {
                let cleaned = n.replace('_', "");
                if cleaned.contains('.') || cleaned.contains('e') || cleaned.contains('E') {
                    HyperType::F64
                } else if cleaned.parse::<i32>().is_ok() {
                    HyperType::I32
                } else if cleaned.parse::<i64>().is_ok() {
                    HyperType::I64
                } else if cleaned.parse::<u64>().is_ok() {
                    // Fits u64 but not i64 (e.g. values above i64::MAX).
                    HyperType::U64
                } else {
                    HyperType::F64
                }
            }
        }
    }

    /// If one operand is a fixed-width int and the other is an integer literal that
    /// fits that width, treat the literal as the fixed width (so `x: i8` + `1` stays i8).
    fn harmonize_int_binary(
        left_expr: &Expr,
        right_expr: &Expr,
        left_ty: HyperType,
        right_ty: HyperType,
    ) -> (HyperType, HyperType) {
        let is_fixed_int = |t: &HyperType| {
            matches!(
                t,
                HyperType::I8
                    | HyperType::I16
                    | HyperType::I32
                    | HyperType::I64
                    | HyperType::U8
                    | HyperType::U16
                    | HyperType::U32
                    | HyperType::U64
            )
        };
        let literal_fits = |expr: &Expr, ty: &HyperType| -> bool {
            Self::expr_fits_type(expr, ty)
        };

        if is_fixed_int(&left_ty) && literal_fits(right_expr, &left_ty) {
            return (left_ty.clone(), left_ty);
        }
        if is_fixed_int(&right_ty) && literal_fits(left_expr, &right_ty) {
            return (right_ty.clone(), right_ty);
        }
        (left_ty, right_ty)
    }

    fn check_expr(&mut self, expr: &Expr) -> HyperType {
        match expr {
            Expr::Literal(lit) => Self::infer_literal(lit),
            Expr::Variable { name, line } => {
                if let Some(b) = self.lookup(name) {
                    b.ty.clone()
                } else if self.structs.contains_key(name) {
                    // Struct name used as constructor / type value.
                    HyperType::Struct(name.clone())
                } else {
                    self.syntax_error(*line, format!("undefined variable '{}'", name));
                    HyperType::Any
                }
            }
            Expr::Group(inner) => self.check_expr(inner),
            Expr::Unary { op, right } => {
                let rt = self.check_expr(right);
                match op {
                    UnaryOp::Neg => {
                        if !Self::is_numeric(&rt) {
                            self.error(format!(
                                "Type error: unary '-' requires a numeric operand, got {:?}.",
                                rt
                            ));
                        }
                        rt
                    }
                    UnaryOp::Not => {
                        // Soft: Not is always ok (truthiness).
                        HyperType::Bool
                    }
                }
            }
            Expr::Binary { op, left, right } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                let (lt, rt) = Self::harmonize_int_binary(left, right, lt, rt);
                if matches!(op, BinOp::Div | BinOp::FloorDiv | BinOp::Rem)
                    && Self::is_zero_literal(right)
                {
                    self.error(format!(
                        "Type error: division by zero in '{}'.",
                        op
                    ));
                }
                self.check_binary(op, &lt, &rt)
            }
            Expr::Assign { name, value } => {
                let vt = self.check_expr(value);
                match self.lookup(name).cloned() {
                    Some(b) => {
                        if !b.mutable {
                            self.error(format!(
                                "Error: Cannot reassign immutable variable '{}'. Use 'let mut' to make it mutable.",
                                name
                            ));
                        } else if !Self::is_compatible(&b.ty, &vt)
                            && !matches!(b.ty, HyperType::Any)
                        {
                            let widened = Self::is_numeric(&b.ty)
                                && Self::is_numeric(&vt)
                                && self.widen_inferred(name, &vt);
                            // Soft: allow if annotated Any; otherwise warn-style error.
                            if !widened && !matches!(vt, HyperType::Any) {
                                self.error(format!(
                                    "Type error: cannot assign {:?} to '{}' of type {:?}.",
                                    vt, name, b.ty
                                ));
                            }
                            if widened {
                                return vt;
                            }
                        }
                        b.ty
                    }
                    None => {
                        self.error(format!(
                            "Error: Undefined variable '{}'.",
                            name
                        ));
                        HyperType::Any
                    }
                }
            }
            Expr::GetField { object, field } => {
                let Some(binding) = self.lookup(object) else {
                    if !self.structs.contains_key(object) {
                        self.error(format!("Error: Undefined variable '{}'.", object));
                    }
                    return HyperType::Any;
                };
                match &binding.ty {
                    HyperType::Struct(name) => {
                        if let Some(fields) = self.structs.get(name) {
                            if let Some(info) = fields.get(field) {
                                return info.ty.clone();
                            }
                            self.error(format!(
                                "Type error: struct '{}' has no field '{}'.",
                                name, field
                            ));
                            return HyperType::Any;
                        }
                        HyperType::Any
                    }
                    HyperType::Any => HyperType::Any,
                    other => {
                        self.error(format!(
                            "Type error: cannot read field '{}' on value of type {:?}.",
                            field, other
                        ));
                        HyperType::Any
                    }
                }
            }
            Expr::SetField {
                object,
                field,
                value,
            } => {
                let vt = self.check_expr(value);
                let Some(binding) = self.lookup(object) else {
                    self.error(format!("Error: Undefined variable '{}'.", object));
                    return HyperType::Any;
                };
                match &binding.ty {
                    HyperType::Struct(name) => {
                        let name = name.clone();
                        if let Some(info) = self
                            .structs
                            .get(&name)
                            .and_then(|fields| fields.get(field))
                            .cloned()
                        {
                            if !info.is_mut {
                                self.error(format!(
                                    "Type error: field '{}.{}' is not mutable.",
                                    name, field
                                ));
                            }
                            if !Self::is_compatible(&info.ty, &vt) {
                                self.error(format!(
                                    "Type error: cannot assign {:?} to field '{}.{}' of type {:?}.",
                                    vt, name, field, info.ty
                                ));
                            }
                            return info.ty;
                        }
                        self.error(format!(
                            "Type error: struct '{}' has no field '{}'.",
                            name, field
                        ));
                        HyperType::Any
                    }
                    HyperType::Any => HyperType::Any,
                    other => {
                        self.error(format!(
                            "Type error: cannot assign field '{}' on value of type {:?}.",
                            field, other
                        ));
                        HyperType::Any
                    }
                }
            }
            Expr::Call { callee, args } => self.check_call(callee, args),
            Expr::CallMethod {
                object,
                method,
                args,
            } => {
                let ot = self.check_expr(object);
                for a in args {
                    let _ = self.check_expr(a);
                }
                self.check_method_on_type(&ot, method, args.len())
            }
            Expr::List(items) => {
                let mut elem = HyperType::Any;
                for (i, item) in items.iter().enumerate() {
                    let t = self.check_expr(item);
                    if i == 0 {
                        elem = t;
                    } else if !Self::is_compatible(&elem, &t) && !Self::is_compatible(&t, &elem) {
                        elem = HyperType::Any;
                    } else {
                        elem = Self::widen_numeric(&elem, &t);
                    }
                }
                HyperType::List(Box::new(elem))
            }
            Expr::Dict(entries) => {
                for (k, v) in entries {
                    let _ = self.check_expr(k);
                    let _ = self.check_expr(v); // Soft: dict values when unknown
                }
                HyperType::Dict
            }
            Expr::Index { object, index } => {
                let ot = self.check_expr(object);
                let _ = self.check_expr(index);
                match ot {
                    HyperType::List(inner) | HyperType::Array(inner) => *inner,
                    HyperType::Dict => HyperType::Any,
                    HyperType::String => HyperType::String,
                    HyperType::Any => HyperType::Any,
                    other => {
                        self.error(format!(
                            "Type error: cannot index value of type {:?}.",
                            other
                        ));
                        HyperType::Any
                    }
                }
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                let ot = self.check_expr(object);
                let _ = self.check_expr(index);
                let vt = self.check_expr(value);
                match ot {
                    HyperType::List(_)
                    | HyperType::Array(_)
                    | HyperType::Dict
                    | HyperType::Any => {}
                    other => {
                        self.error(format!(
                            "Type error: cannot index-assign value of type {:?}.",
                            other
                        ));
                    }
                }
                vt
            }
            Expr::FString { parts, .. } => {
                for part in parts {
                    if let FStringPart::Expr(e) = part {
                        let _ = self.check_expr(e); // Soft: f-string parts
                    }
                }
                HyperType::String
            }
            Expr::Ternary {
                condition,
                then_branch,
                else_branch,
            } => {
                let _ = self.check_expr(condition);
                let tt = self.check_expr(then_branch);
                let et = self.check_expr(else_branch);
                if Self::is_compatible(&tt, &et) || Self::is_compatible(&et, &tt) {
                    if Self::is_numeric(&tt) && Self::is_numeric(&et) {
                        Self::widen_numeric(&tt, &et)
                    } else {
                        tt
                    }
                } else {
                    HyperType::Any
                }
            }
            Expr::Handle { attempt, fallback } => {
                self.handle_depth += 1;
                let at = self.check_expr(attempt);
                self.handle_depth -= 1;
                let ft = self.check_expr(fallback);
                if Self::is_compatible(&at, &ft) || Self::is_compatible(&ft, &at) {
                    if Self::is_numeric(&at) && Self::is_numeric(&ft) {
                        Self::widen_numeric(&at, &ft)
                    } else {
                        at
                    }
                } else {
                    HyperType::Any
                }
            }
        }
    }

    fn is_zero_literal(expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Number(n)) => {
                let t = n.trim();
                t == "0"
                    || t == "0.0"
                    || t == "0.00"
                    || t == "0e0"
                    || t == "0E0"
                    || t.parse::<f64>().ok() == Some(0.0)
            }
            Expr::Group(inner) => Self::is_zero_literal(inner),
            Expr::Unary {
                op: UnaryOp::Neg,
                right,
            } => Self::is_zero_literal(right),
            _ => false,
        }
    }

    fn is_lengthable(ty: &HyperType) -> bool {
        matches!(
            ty,
            HyperType::List(_)
                | HyperType::Array(_)
                | HyperType::Dict
                | HyperType::String
                | HyperType::Any
        )
    }

    fn check_method_on_type(
        &mut self,
        receiver: &HyperType,
        method: &str,
        argc: usize,
    ) -> HyperType {
        let string_methods: &[&str] = &[
            "len",
            "upper",
            "lower",
            "capitalize",
            "title",
            "swapcase",
            "strip",
            "lstrip",
            "rstrip",
            "startswith",
            "endswith",
            "split",
            "rsplit",
            "replace",
            "join",
            "find",
            "rfind",
            "index",
            "rindex",
            "count",
            "isdigit",
            "isalpha",
            "isalnum",
            "isspace",
            "islower",
            "isupper",
            "istitle",
            "isascii",
            "center",
            "ljust",
            "rjust",
            "zfill",
            "removeprefix",
            "removesuffix",
            "partition",
            "rpartition",
        ];
        match receiver {
            HyperType::Any
                | HyperType::Struct(_)
                | HyperType::File
                | HyperType::Mmap
                | HyperType::None
                | HyperType::Trait(_) => HyperType::Any,
            HyperType::String => {
                if !string_methods.contains(&method) {
                    self.error(format!(
                        "Type error: string has no method '{}'.",
                        method
                    ));
                } else if method == "len" && argc != 0 {
                    self.error(format!(
                        "Type error: '{method}' expects 0 argument(s) but got {argc}."
                    ));
                }
                match method {
                    "len" | "find" | "rfind" | "index" | "rindex" | "count" => HyperType::I64,
                    "startswith" | "endswith" | "isdigit" | "isalpha" | "isalnum" | "isspace"
                    | "islower" | "isupper" | "istitle" | "isascii" => HyperType::Bool,
                    "split" | "rsplit" | "partition" | "rpartition" => {
                        HyperType::List(Box::new(HyperType::String))
                    }
                    _ => HyperType::String,
                }
            }
            HyperType::List(_) | HyperType::Array(_) => match method {
                "len" => {
                    if argc != 0 {
                        self.error(format!(
                            "Type error: 'len' expects 0 argument(s) but got {argc}."
                        ));
                    }
                    HyperType::I64
                }
                "append" => {
                    if argc != 1 {
                        self.error(format!(
                            "Type error: 'append' expects 1 argument(s) but got {argc}."
                        ));
                    }
                    HyperType::None
                }
                other => {
                    self.error(format!(
                        "Type error: list has no method '{}'.",
                        other
                    ));
                    HyperType::Any
                }
            },
            HyperType::Dict => match method {
                "len" => {
                    if argc != 0 {
                        self.error(format!(
                            "Type error: 'len' expects 0 argument(s) but got {argc}."
                        ));
                    }
                    HyperType::I64
                }
                "keys" => {
                    if argc != 0 {
                        self.error(format!(
                            "Type error: 'keys' expects 0 argument(s) but got {argc}."
                        ));
                    }
                    HyperType::List(Box::new(HyperType::String))
                }
                other => {
                    self.error(format!(
                        "Type error: dict has no method '{}'.",
                        other
                    ));
                    HyperType::Any
                }
            },
            other => {
                self.error(format!(
                    "Type error: type {:?} has no method '{}'.",
                    other, method
                ));
                HyperType::Any
            }
        }
    }

    fn check_builtin_call(
        &mut self,
        name: &str,
        args: &[CallArg],
        arg_tys: &[HyperType],
    ) -> Option<HyperType> {
        let n = arg_tys.len();
        let expect_exact = |this: &mut Self, want: usize| {
            if n != want {
                this.error(format!(
                    "Type error: {name} expects {want} argument(s) but got {n}."
                ));
            }
        };
        let ret = match name {
            "print" => return None, // varargs; already soft
            "clock" => {
                expect_exact(self, 0);
                HyperType::F64
            }
            "input" => {
                if n > 1 {
                    self.error(format!(
                        "Type error: input expects 0 or 1 argument(s) but got {n}."
                    ));
                }
                HyperType::String
            }
            "open" => {
                if n == 0 || n > 2 {
                    self.error(format!(
                        "Type error: open expects 1 or 2 argument(s) but got {n}."
                    ));
                }
                HyperType::File
            }
            "len" => {
                expect_exact(self, 1);
                if n == 1 && !Self::is_lengthable(&arg_tys[0]) {
                    self.error(format!(
                        "Type error: len() argument must be a list, array, dict, or string, got {:?}.",
                        arg_tys[0]
                    ));
                }
                HyperType::I64
            }
            "abs" | "chr" | "ord" | "bin" | "hex" | "oct" | "int" | "float" | "str" | "bool"
            | "all" | "any" | "sorted" | "reversed" | "enumerate" | "sum" | "repr" => {
                if name == "enumerate" {
                    if n == 0 || n > 2 {
                        self.error(format!(
                            "Type error: enumerate expects 1 or 2 argument(s) but got {n}."
                        ));
                    }
                } else {
                    expect_exact(self, 1);
                }
                if name == "abs" && n == 1 && !Self::is_numeric(&arg_tys[0]) && !matches!(arg_tys[0], HyperType::Any) {
                    self.error(format!(
                        "Type error: abs() expects a number, got {:?}.",
                        arg_tys[0]
                    ));
                }
                if (name == "all" || name == "any" || name == "sum" || name == "sorted" || name == "reversed")
                    && n == 1
                    && !matches!(
                        arg_tys[0],
                        HyperType::List(_) | HyperType::Array(_) | HyperType::Any
                    )
                {
                    self.error(format!(
                        "Type error: {name}() expects a list, got {:?}.",
                        arg_tys[0]
                    ));
                }
                match name {
                    "chr" | "bin" | "hex" | "oct" | "str" | "repr" => HyperType::String,
                    "ord" | "int" => HyperType::I64,
                    "float" => HyperType::F64,
                    "bool" | "all" | "any" => HyperType::Bool,
                    "sorted" | "reversed" | "enumerate" => {
                        HyperType::List(Box::new(HyperType::Any))
                    }
                    _ => HyperType::Any,
                }
            }
            "pow" | "divmod" => {
                expect_exact(self, 2);
                if n == 2 {
                    for (i, t) in arg_tys.iter().enumerate() {
                        if !Self::is_numeric(t) && !matches!(t, HyperType::Any) {
                            self.error(format!(
                                "Type error: {name}() argument {} must be numeric, got {:?}.",
                                i + 1,
                                t
                            ));
                        }
                    }
                    if name == "divmod" {
                        let e = match &args[1] {
                            CallArg::Positional(e) | CallArg::Named { value: e, .. } => e,
                        };
                        if Self::is_zero_literal(e) {
                            self.error(
                                "Type error: divmod() division by zero.".to_string(),
                            );
                        }
                    }
                }
                if name == "divmod" {
                    HyperType::List(Box::new(HyperType::Any))
                } else {
                    HyperType::Any
                }
            }
            "round" => {
                if n == 0 || n > 2 {
                    self.error(format!(
                        "Type error: round expects 1 or 2 argument(s) but got {n}."
                    ));
                }
                HyperType::Any
            }
            "min" | "max" => {
                if n == 0 {
                    self.error(format!(
                        "Type error: {name} expects at least 1 argument."
                    ));
                }
                HyperType::Any
            }
            "list" => {
                if n > 1 {
                    self.error(format!(
                        "Type error: list expects 0 or 1 argument(s) but got {n}."
                    ));
                }
                HyperType::List(Box::new(HyperType::Any))
            }
            "range" => {
                if n == 0 || n > 3 {
                    self.error(format!(
                        "Type error: range expects 1 to 3 argument(s) but got {n}."
                    ));
                }
                if n == 3 {
                    let e = match &args[2] {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => e,
                    };
                    if Self::is_zero_literal(e) {
                        self.error(
                            "Type error: range() step must not be zero.".to_string(),
                        );
                    }
                }
                HyperType::List(Box::new(HyperType::I64))
            }
            "zip" => {
                // zip packs args into a list at runtime; 0 args yields empty list.
                HyperType::List(Box::new(HyperType::Any))
            }
            _ => return None,
        };
        Some(ret)
    }

    fn check_binary(&mut self, op: &BinOp, left: &HyperType, right: &HyperType) -> HyperType {
        match op {
            BinOp::Add => {
                if matches!(left, HyperType::String) && matches!(right, HyperType::String) {
                    return HyperType::String;
                }
                if matches!(left, HyperType::String) || matches!(right, HyperType::String) {
                    // Soft: string + other via coercion in interpreter — allow as String if either is string + Any
                    if matches!(left, HyperType::Any) || matches!(right, HyperType::Any) {
                        return HyperType::String;
                    }
                }
                if Self::is_numeric(left) && Self::is_numeric(right) {
                    return Self::widen_numeric(left, right);
                }
                if matches!(left, HyperType::Any) || matches!(right, HyperType::Any) {
                    return HyperType::Any;
                }
                self.error(format!(
                    "Type error: '+' requires numeric or string operands, got {:?} and {:?}.",
                    left, right
                ));
                HyperType::Any
            }
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Rem | BinOp::Pow => {
                if Self::is_numeric(left) && Self::is_numeric(right) {
                    return Self::widen_numeric(left, right);
                }
                self.error(format!(
                    "Type error: arithmetic '{}' requires numeric operands, got {:?} and {:?}.",
                    op, left, right
                ));
                HyperType::Any
            }
            BinOp::Eq | BinOp::Ne => HyperType::Bool,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ok = (Self::is_numeric(left) && Self::is_numeric(right))
                    || (matches!(left, HyperType::Bool) && matches!(right, HyperType::Bool))
                    || (matches!(left, HyperType::String) && matches!(right, HyperType::String))
                    || matches!(left, HyperType::Any)
                    || matches!(right, HyperType::Any);
                if !ok {
                    self.error(format!(
                        "Type error: comparison requires numeric, bool, or string operands, got {:?} and {:?}.",
                        left, right
                    ));
                }
                HyperType::Bool
            }
            BinOp::And | BinOp::Or => {
                if !Self::is_boolish(left) || !Self::is_boolish(right) {
                    self.error(format!(
                        "Type error: '{}' requires bool-ish operands, got {:?} and {:?}.",
                        op, left, right
                    ));
                }
                HyperType::Bool
            }
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[CallArg]) -> HyperType {
        let callee_ty = self.check_expr(callee);

        // Collect positional arg types (named args still typechecked).
        let mut arg_tys = Vec::new();
        for arg in args {
            match arg {
                CallArg::Positional(e) => arg_tys.push(self.check_expr(e)),
                CallArg::Named { value, .. } => arg_tys.push(self.check_expr(value)),
            }
        }

        // Named builtins: arity / operand checks before generic Function rules.
        if let Expr::Variable { name, .. } = callee {
            if let Some(ret) = self.check_builtin_call(name, args, &arg_tys) {
                return ret;
            }
        }

        // Struct construction: Call on struct name.
        if let HyperType::Struct(ref name) = callee_ty {
            let _ = name;
            return callee_ty;
        }

        match &callee_ty {
            HyperType::Function { params, ret } => {
                // Arity check when callee type known (skip for print-style varargs soft).
                // print is registered with 1 Any param but accepts any arity — soft skip if Any params.
                let all_any = params.iter().all(|p| matches!(p, HyperType::Any))
                    && params.len() <= 1;
                if !all_any && params.len() != arg_tys.len() {
                    self.error(format!(
                        "Type error: expected {} argument(s) but got {}.",
                        params.len(),
                        arg_tys.len()
                    ));
                } else if !all_any {
                    for (i, (pt, at)) in params.iter().zip(arg_tys.iter()).enumerate() {
                        if !Self::is_compatible(pt, at) {
                            self.error(format!(
                                "Type error: argument {} expected {:?}, got {:?}.",
                                i + 1,
                                pt,
                                at
                            ));
                        }
                    }
                }
                ret.as_ref().clone()
            }
            HyperType::Any => HyperType::Any,
            other => {
                self.error(format!(
                    "Type error: value of type {:?} is not callable.",
                    other
                ));
                HyperType::Any
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                line,
                is_mutable,
                name,
                type_ann,
                initializer,
            } => {
                let init_ty = self.check_expr(initializer);
                let declared = match type_ann {
                    TypeAnn::None => init_ty.clone(),
                    other => {
                        let ann = self.type_ann_to_hyper(other);
                        if !Self::is_compatible(&ann, &init_ty)
                            && !Self::expr_fits_type(initializer, &ann)
                        {
                            self.syntax_error(
                                *line,
                                format!(
                                    "cannot initialize '{}' of type {:?} with {:?}",
                                    name, ann, init_ty
                                ),
                            );
                        }
                        // Prefer the annotation when present.
                        if matches!(ann, HyperType::Any) {
                            init_ty
                        } else {
                            ann
                        }
                    }
                };
                self.define(
                    name,
                    Binding {
                        ty: declared,
                        mutable: *is_mutable,
                    },
                );
                if let Some(scope) = self.scopes.last_mut() {
                    if matches!(type_ann, TypeAnn::None) {
                        scope.inferred.insert(name.clone());
                    } else {
                        scope.inferred.remove(name);
                    }
                }
            }
            Stmt::Print { values, .. } => {
                for v in values {
                    let _ = self.check_expr(v);
                }
            }
            Stmt::Expr { expr, .. } => {
                let _ = self.check_expr(expr);
            }
            Stmt::Block(stmts) => {
                self.push_scope();
                self.hoist_functions(stmts);
                for s in stmts {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let _ = self.check_expr(condition);
                self.check_stmt(then_branch);
                if let Some(else_b) = else_branch {
                    self.check_stmt(else_b);
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                let _ = self.check_expr(condition);
                self.loop_depth += 1;
                self.check_stmt(body);
                self.loop_depth -= 1;
            }
            Stmt::For {
                var,
                iter,
                body,
                ..
            } => {
                match iter {
                    ForIter::Range { start, end } => {
                        let st = self.check_expr(start);
                        let et = self.check_expr(end);
                        if !Self::is_numeric(&st) {
                            self.error(format!(
                                "Type error: for-loop start must be numeric, got {:?}.",
                                st
                            ));
                        }
                        if !Self::is_numeric(&et) {
                            self.error(format!(
                                "Type error: for-loop end must be numeric, got {:?}.",
                                et
                            ));
                        }
                        self.push_scope();
                        self.define(
                            var,
                            Binding {
                                ty: HyperType::I64,
                                mutable: false,
                            },
                        );
                    }
                    ForIter::Iterable(iterable) => {
                        let it = self.check_expr(iterable);
                        let elem_ty = match it {
                            HyperType::List(inner) => *inner,
                            HyperType::Array(inner) => *inner,
                            HyperType::Any => HyperType::Any,
                            other => {
                                self.error(format!(
                                    "Type error: for-in iterable must be a list, got {:?}.",
                                    other
                                ));
                                HyperType::Any
                            }
                        };
                        self.push_scope();
                        self.define(
                            var,
                            Binding {
                                ty: elem_ty,
                                mutable: false,
                            },
                        );
                    }
                }
                self.loop_depth += 1;
                self.check_stmt(body);
                self.loop_depth -= 1;
                self.pop_scope();
            }
            Stmt::Function(decl) => self.check_function(decl),
            Stmt::Return { line, value } => {
                let vt = self.check_expr(value);
                if let Some(ref expected) = self.expected_return {
                    if !Self::is_compatible(expected, &vt)
                        && !matches!(expected, HyperType::Any)
                        && !matches!(vt, HyperType::Any | HyperType::None)
                    {
                        self.syntax_error(
                            *line,
                            format!(
                                "return type {:?} is not compatible with {:?}",
                                vt, expected
                            ),
                        );
                    }
                }
            }
            Stmt::Break { line } => {
                if self.loop_depth == 0 {
                    self.syntax_error(*line, "break outside loop");
                }
            }
            Stmt::Continue { line } => {
                if self.loop_depth == 0 {
                    self.syntax_error(*line, "continue outside loop");
                }
            }
            Stmt::Raise { line, value } => {
                let _ = self.check_expr(value);
                // Module-level `raise` is allowed (process exits). Inside a function,
                // require `raises` on the signature (or an enclosing `handle`).
                if self.expected_return.is_some() && !self.allows_raise && self.handle_depth == 0 {
                    self.syntax_error(
                        *line,
                        "raise outside a `raises` function or `handle` expression",
                    );
                }
            }
            Stmt::Struct {
                name,
                implemented_trait,
                fields,
                methods,
            } => {
                if let Some(t) = implemented_trait {
                    match self.traits.get(t).cloned() {
                        Some(required) => {
                            let provided = MethodSig::from_struct_methods(methods);
                            for msg in trait_conformance_errors(name, t, &required, &provided) {
                                self.error(msg);
                            }
                        }
                        // The interpreter needs the trait bound before the
                        // struct runs, so a later declaration is still an error.
                        None => self.error(format!("trait '{}' is not defined", t)),
                    }
                }
                let mut field_map = HashMap::new();
                for field in fields {
                    let ty = self.resolve_type_name(&field.type_name);
                    field_map.insert(
                        field.name.clone(),
                        StructFieldInfo {
                            ty,
                            is_pub: field.is_pub,
                            is_mut: field.is_mut,
                        },
                    );
                }
                self.structs.insert(name.clone(), field_map);
                self.define(
                    name,
                    Binding {
                        ty: HyperType::Struct(name.clone()),
                        mutable: false,
                    },
                );
                for m in methods {
                    // Methods checked in a soft scope; register loosely.
                    self.check_function(&m.function);
                }
            }
            Stmt::Trait { name, methods } => {
                self.traits
                    .insert(name.clone(), MethodSig::from_trait_methods(methods));
                self.define(
                    name,
                    Binding {
                        ty: HyperType::Trait(name.clone()),
                        mutable: false,
                    },
                );
                for m in methods {
                    // Soft: just register signatures.
                    let params: Vec<HyperType> = m
                        .params
                        .iter()
                        .map(|p| {
                            p.type_ann
                                .as_ref()
                                .map(|t| self.resolve_type_name(t))
                                .unwrap_or(HyperType::Any)
                        })
                        .collect();
                    let ret = m
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_name(t))
                        .unwrap_or(HyperType::Any);
                    self.define(
                        &m.name,
                        Binding {
                            ty: HyperType::Function {
                                params,
                                ret: Box::new(ret),
                            },
                            mutable: false,
                        },
                    );
                }
            }
            Stmt::WithMmap {
                path, var, body, ..
            } => {
                let _ = self.check_expr(path);
                self.push_scope();
                self.define(
                    var,
                    Binding {
                        ty: HyperType::Mmap,
                        mutable: false,
                    },
                );
                self.check_stmt(body);
                self.pop_scope();
            }
            Stmt::With {
                value, var, body, ..
            } => {
                let ty = self.check_expr(value);
                self.push_scope();
                self.define(var, Binding { ty, mutable: false });
                self.check_stmt(body);
                self.pop_scope();
            }
            Stmt::Import {
                module, alias, ..
            } => {
                let bind = alias.as_ref().unwrap_or(module);
                self.define(
                    bind,
                    Binding {
                        ty: HyperType::Any,
                        mutable: false,
                    },
                );
            }
            Stmt::ImportFrom { names, .. } => {
                for item in names {
                    let bind = item.alias.as_ref().unwrap_or(&item.name);
                    self.define(
                        bind,
                        Binding {
                            ty: HyperType::Any,
                            mutable: false,
                        },
                    );
                }
            }
        }
    }

    /// Register a function signature so calls can appear before the definition.
    fn declare_function(&mut self, decl: &FunctionDecl) {
        let params: Vec<HyperType> = decl
            .params
            .iter()
            .map(|p| {
                p.type_ann
                    .as_ref()
                    .map(|t| self.resolve_type_name(t))
                    .unwrap_or(HyperType::Any)
            })
            .collect();
        let ret = decl
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_name(t))
            .unwrap_or(HyperType::Any);
        self.define(
            &decl.name,
            Binding {
                ty: HyperType::Function {
                    params,
                    ret: Box::new(ret),
                },
                mutable: false,
            },
        );
    }

    fn hoist_functions(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            if let Stmt::Function(decl) = stmt {
                self.declare_function(decl);
            }
        }
    }

    fn check_function(&mut self, decl: &FunctionDecl) {
        let params: Vec<HyperType> = decl
            .params
            .iter()
            .map(|p| {
                p.type_ann
                    .as_ref()
                    .map(|t| self.resolve_type_name(t))
                    .unwrap_or(HyperType::Any)
            })
            .collect();
        let ret = decl
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_name(t))
            .unwrap_or(HyperType::Any);

        // Register function in current scope before checking body (allows recursion).
        self.define(
            &decl.name,
            Binding {
                ty: HyperType::Function {
                    params: params.clone(),
                    ret: Box::new(ret.clone()),
                },
                mutable: false,
            },
        );

        self.push_scope();
        for (param, pty) in decl.params.iter().zip(params.iter()) {
            self.define(
                &param.name,
                Binding {
                    ty: pty.clone(),
                    mutable: true,
                },
            );
        }
        let prev_ret = self.expected_return.replace(ret);
        // A function body does not sit inside the loop that encloses its declaration.
        let prev_depth = std::mem::take(&mut self.loop_depth);
        let prev_raise = self.allows_raise;
        self.allows_raise = decl.raises;
        self.check_stmt(&decl.body);
        self.allows_raise = prev_raise;
        self.loop_depth = prev_depth;
        self.expected_return = prev_ret;
        self.pop_scope();
    }
}

pub fn typecheck(stmts: &[Stmt]) -> Result<(), Vec<String>> {
    let mut tc = TypeChecker::new();
    tc.hoist_functions(stmts);
    for stmt in stmts {
        tc.check_stmt(stmt);
    }
    if tc.errors.is_empty() {
        Ok(())
    } else {
        Err(tc.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str) -> Result<(), Vec<String>> {
        let stmts = driver::parse_program(source).expect("source should parse");
        typecheck(&stmts)
    }

    #[test]
    fn functions_may_be_called_before_they_are_defined() {
        check(
            "fn outer(n: i64) -> i64:\n\
             \x20   return inner(n) + 1\n\
             \n\
             fn inner(n: i64) -> i64:\n\
             \x20   return n * 2\n\
             \n\
             print(outer(4))\n",
        )
        .expect("a forward call should typecheck");
    }

    #[test]
    fn inferred_counter_accepts_a_wider_number() {
        check(
            "let mut total = 0\n\
             for i in range(3):\n\
             \x20   total = total + i\n",
        )
        .expect("an inferred counter should widen");
    }

    #[test]
    fn literal_fits_smaller_integer_annotation() {
        check("let a: i8 = -128\n").expect("i8 literal should fit");
        check("let pi: float32 = 3.14\n").expect("float literal should fit f32");
    }

    #[test]
    fn annotated_variable_keeps_its_type() {
        let errors = check(
            "let mut total: i32 = 0\n\
             for i in range(3):\n\
             \x20   total = i\n",
        )
        .expect_err("an annotated variable should not widen");
        assert!(
            errors.iter().any(|e| e.contains("cannot assign")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn loop_jumps_are_allowed_inside_loops() {
        check(
            "let mut i = 0\n\
             while i < 5:\n\
             \x20   i = i + 1\n\
             \x20   if i == 2:\n\
             \x20       continue\n\
             \x20   break\n\
             \n\
             for n in range(3):\n\
             \x20   if n == 1:\n\
             \x20       break\n",
        )
        .expect("break and continue inside loops should typecheck");
    }

    #[test]
    fn break_outside_a_loop_is_rejected() {
        let errors = check("break\n").expect_err("a top-level break should fail");
        assert!(
            errors.iter().any(|e| e.contains("break outside loop")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn continue_outside_a_loop_is_rejected() {
        let errors = check("continue\n").expect_err("a top-level continue should fail");
        assert!(
            errors.iter().any(|e| e.contains("continue outside loop")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn literal_division_by_zero_is_rejected_at_typecheck() {
        for src in ["print(1 / 0)\n", "print(1 // 0)\n", "print(1 % 0)\n", "print(divmod(1, 0))\n"] {
            let errors = check(src).expect_err("literal division by zero should fail typecheck");
            assert!(
                errors.iter().any(|e| e.contains("division by zero")),
                "src={src:?} errors={errors:?}"
            );
        }
    }

    #[test]
    fn variable_division_by_zero_still_typechecks() {
        check("let d = 0\nprint(10 / d)\n").expect("dynamic zero stays a runtime check");
    }

    #[test]
    fn unknown_method_on_known_type_is_rejected() {
        let errors = check("print([1, 2].keys())\n").expect_err("list has no keys");
        assert!(
            errors.iter().any(|e| e.contains("no method")),
            "unexpected errors: {:?}",
            errors
        );
        let errors = check("print(\"hi\".append(1))\n").expect_err("string has no append");
        assert!(
            errors.iter().any(|e| e.contains("no method")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn non_callable_values_are_rejected() {
        let errors = check("let x = 1\nprint(x())\n").expect_err("int is not callable");
        assert!(
            errors.iter().any(|e| e.contains("not callable")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn len_rejects_non_lengthable_types() {
        let errors = check("print(len(3))\n").expect_err("len(int) should fail");
        assert!(
            errors.iter().any(|e| e.contains("len()")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn range_zero_step_is_rejected() {
        let errors = check("print(range(0, 10, 0))\n").expect_err("zero step");
        assert!(
            errors.iter().any(|e| e.contains("step must not be zero")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn a_function_body_does_not_inherit_the_enclosing_loop() {
        let errors = check(
            "for n in range(3):\n\
             \x20   fn helper():\n\
             \x20       break\n",
        )
        .expect_err("a break in a nested function should fail");
        assert!(
            errors.iter().any(|e| e.contains("break outside loop")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn struct_field_types_are_checked() {
        let src = "\
struct Point:\n\
\x20   let pub mut x: i64\n\
\x20   let pub y: i64\n\
let mut p = Point(x: 1, y: 2)\n\
p.x = \"no\"\n";
        let errors = check(src).expect_err("assign string to i64 field");
        assert!(
            errors.iter().any(|e| e.contains("cannot assign")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn struct_unknown_field_is_rejected() {
        let src = "\
struct Point:\n\
\x20   let pub x: i64\n\
let p = Point(x: 1)\n\
print(p.z)\n";
        let errors = check(src).expect_err("unknown field");
        assert!(
            errors.iter().any(|e| e.contains("no field")),
            "unexpected errors: {:?}",
            errors
        );
    }

    #[test]
    fn immutable_struct_field_rejects_assign() {
        let src = "\
struct Point:\n\
\x20   let pub x: i64\n\
let mut p = Point(x: 1)\n\
p.x = 2\n";
        let errors = check(src).expect_err("immutable field");
        assert!(
            errors.iter().any(|e| e.contains("not mutable")),
            "unexpected errors: {:?}",
            errors
        );
    }
}

pub fn run_typecheck(file_contents: String) {
    let stmts = match driver::parse_program(&file_contents) {
        Ok(s) => s,
        Err(()) => process::exit(65),
    };

    match typecheck(&stmts) {
        Ok(()) => {
            println!("Typecheck passed.");
        }
        Err(errors) => {
            for e in errors {
                error::report_formatted(&e);
            }
            process::exit(65);
        }
    }
}
