//! Type system: `Type`, conversions, operator rules (architecture §14).

use crate::semantic::provider::ExternalCategory;
use crate::semantic::symbols::SymbolId;

#[derive(Clone, PartialEq, Debug)]
pub enum Type {
    Number,
    String,
    Bool,
    Any,
    Void,
    Null,
    Vector,
    Team,
    Hero,
    Player,
    Players,
    Color,
    Class(SymbolId),
    Struct(SymbolId),
    Enum(SymbolId),
    Array(Box<Type>),
    GenericInstantiation {
        def: SymbolId,
        args: Vec<Type>,
    },
    TypeParam {
        param: SymbolId,
        bound: Option<TypeParamBound>,
    },
    FunctionValue(FunctionType),
    /// `T | U` anonymous struct unions (parse-only per PM Q11).
    Union(Vec<Type>),
    External(ExternalType),
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeParamBound {
    None,
    Single,
}

#[derive(Clone, PartialEq, Debug)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub ret: Box<Type>,
    pub constant: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ExternalType {
    pub category: ExternalCategory,
    pub constant: bool,
}

/// Primitive type table (also used by the HIR lowerer).
pub fn primitive(name: &str) -> Option<Type> {
    Some(match name {
        "Number" => Type::Number,
        "String" => Type::String,
        "Boolean" | "Bool" => Type::Bool,
        "Any" => Type::Any,
        "void" => Type::Void,
        "Vector" => Type::Vector,
        "Team" => Type::Team,
        "Hero" => Type::Hero,
        "Player" => Type::Player,
        "Players" => Type::Players,
        "Color" => Type::Color,
        "null" => Type::Null,
        _ => return None,
    })
}

impl Type {
    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    pub fn is_external(&self) -> bool {
        matches!(self, Type::External(_))
    }

    pub fn describe(&self) -> String {
        match self {
            Type::Number => "Number".into(),
            Type::String => "String".into(),
            Type::Bool => "Bool".into(),
            Type::Any => "Any".into(),
            Type::Void => "void".into(),
            Type::Null => "null".into(),
            Type::Vector => "Vector".into(),
            Type::Team => "Team".into(),
            Type::Hero => "Hero".into(),
            Type::Player => "Player".into(),
            Type::Players => "Players".into(),
            Type::Color => "Color".into(),
            Type::Class(_) => "class".into(),
            Type::Struct(_) => "struct".into(),
            Type::Enum(_) => "enum".into(),
            Type::Array(_) => "array".into(),
            Type::GenericInstantiation { .. } => "generic".into(),
            Type::TypeParam { .. } => "type parameter".into(),
            Type::FunctionValue(_) => "function".into(),
            Type::Union(_) => "union".into(),
            Type::External(_) => "external".into(),
            Type::Error => "error".into(),
        }
    }

    /// The element type if this is an array.
    pub fn array_element(&self) -> Option<&Type> {
        match self {
            Type::Array(inner) => Some(inner),
            _ => None,
        }
    }

    /// Array targets accept element values via `+=` (append) and element
    /// arrays via `=` even when the literal element type differs.
    pub fn is_array_like(&self) -> bool {
        matches!(self, Type::Array(_))
    }

    pub fn is_element_compatible(&self, array_ty: &Type) -> bool {
        let Some(elem) = array_ty.array_element() else {
            return false;
        };
        matches!(self, Type::Any) || self == elem
    }
}

/// Conversion ranking for overload resolution (§13.6/§14).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conversion {
    Identity = 0,
    UpcastClass = 1,
    ToAny = 2,
    FromNull = 3,
    UnwrapTypeParam = 4,
    ExternalUnknown = 5,
    None = 255,
}

impl Conversion {
    pub fn rank(self) -> u8 {
        self as u8
    }
}

pub trait TypeInfo {
    /// Single-struct/single-enum storage mode (parallel vs single).
    fn single(&self) -> bool;
    /// Class inheritance base.
    fn base(&self) -> Option<&Type>;
}

/// Conversions and assignability, given declaration info for user types.
/// `is_single` and `is_subclass` are supplied by the semantic program.
pub fn conversion(
    from: &Type,
    to: &Type,
    single_of: &dyn Fn(SymbolId) -> bool,
    base_of: &dyn Fn(SymbolId) -> Option<SymbolId>,
) -> Conversion {
    if from == to {
        return Conversion::Identity;
    }
    match (from, to) {
        (Type::Error, _) | (_, Type::Error) => Conversion::Identity,
        (_, Type::Any) => {
            // Parallel structs/enums are not assignable to Any (SM038).
            if let Type::Struct(id) | Type::Enum(id) = from {
                if !single_of(*id) {
                    return Conversion::None;
                }
            }
            Conversion::ToAny
        }
        (Type::Number, Type::Enum(_)) | (Type::Enum(_), Type::Number) => {
            // Payload-less enums are numbers (corpus enum-basic, module
            // `CurrentMode = 0`).
            Conversion::Identity
        }
        (Type::Null, Type::Class(_))
        | (Type::Null, Type::Any)
        | (Type::Null, Type::Array(_))
        | (Type::Null, Type::String)
        | (Type::Null, Type::Vector)
        | (Type::Null, Type::Team)
        | (Type::Null, Type::Hero)
        | (Type::Null, Type::Color)
        | (Type::Null, Type::Player)
        | (Type::Null, Type::Players) => Conversion::FromNull,
        (Type::Class(sub), Type::Class(base)) => {
            if is_subclass(*sub, *base, base_of) {
                Conversion::UpcastClass
            } else {
                Conversion::None
            }
        }
        (
            Type::GenericInstantiation { def: a, args: aa },
            Type::GenericInstantiation { def: b, args: bb },
        ) => {
            if a == b && aa.len() == bb.len() {
                let mut rank = Conversion::Identity;
                for (x, y) in aa.iter().zip(bb) {
                    match conversion(x, y, single_of, base_of) {
                        Conversion::None => return Conversion::None,
                        r if r.rank() > rank.rank() => rank = r,
                        _ => {}
                    }
                }
                rank
            } else {
                Conversion::None
            }
        }
        (Type::TypeParam { .. }, to) => {
            // Unwrap type params at instantiation sites.
            let _ = to;
            Conversion::UnwrapTypeParam
        }
        (Type::Any, _) => {
            // Any is universally assignable (corpus: defines inferred from
            // null later hold vectors, filtered arrays, etc.).
            Conversion::ToAny
        }
        (Type::FunctionValue(a), Type::FunctionValue(b)) => {
            if a.params.len() == b.params.len()
                && a.params
                    .iter()
                    .zip(b.params.iter())
                    .all(|(x, y)| x == y || matches!(x, Type::Any))
            {
                Conversion::Identity
            } else {
                Conversion::None
            }
        }
        (Type::Array(a), Type::Array(b)) => {
            // Element-wildcard: Any-element arrays convert to typed arrays.
            if **a == Type::Any || **a == **b {
                Conversion::Identity
            } else {
                Conversion::None
            }
        }
        (Type::External(_), _) | (_, Type::External(_)) => Conversion::ExternalUnknown,
        (Type::Union(members), to) => {
            if members
                .iter()
                .any(|m| conversion(m, to, single_of, base_of).rank() < 255)
            {
                Conversion::Identity
            } else {
                Conversion::None
            }
        }
        _ => Conversion::None,
    }
}

pub fn is_subclass(
    sub: SymbolId,
    base: SymbolId,
    base_of: &dyn Fn(SymbolId) -> Option<SymbolId>,
) -> bool {
    let mut cur = Some(sub);
    while let Some(c) = cur {
        if c == base {
            return true;
        }
        cur = base_of(c);
    }
    false
}

pub fn is_assignable(
    from: &Type,
    to: &Type,
    single_of: &dyn Fn(SymbolId) -> bool,
    base_of: &dyn Fn(SymbolId) -> Option<SymbolId>,
) -> bool {
    conversion(from, to, single_of, base_of).rank() < 255
}

/// Explicit cast legality (§14): casts between number-like values, to/from
/// Any, enum casts with default discriminants.
pub fn cast_legal(from: &Type, to: &Type) -> bool {
    if from == to {
        return true;
    }
    match (from, to) {
        (Type::Error, _) | (_, Type::Error) => true,
        (_, Type::Any) | (Type::Any, _) => true,
        (Type::Number, Type::Number) => true,
        (Type::Class(_), Type::Class(_)) => true,
        (Type::Number, Type::Class(_)) => true,
        (Type::Enum(_), Type::Number) | (Type::Number, Type::Enum(_)) => true,
        (Type::Enum(_), Type::Enum(_)) => true,
        (Type::External(_), _) | (_, Type::External(_)) => true,
        _ => false,
    }
}

/// Whether a type is a "constant or parallel data type" (enum keys, rule
/// conditions — SM042/SM046).
pub fn is_constant_or_parallel(ty: &Type) -> bool {
    match ty {
        Type::External(e) => e.constant,
        Type::Struct(_) | Type::Enum(_) => true,
        Type::Array(_) => true,
        Type::Union(_) => true,
        _ => false,
    }
}
