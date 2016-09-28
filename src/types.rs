#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EfiType {
    Status,
    Bool,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    IntN,
    UIntN,
    Char8,
    Char16,
    Id(String),
    Ptr(Box<EfiType>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EfiArgDir {
    In,
    Out,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EfiArg {
    pub name: String,
    pub typ: EfiType,
    pub dir: EfiArgDir,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EfiMethod {
    pub name: String,
    pub typ: EfiType,
    pub args: Vec<EfiArg>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EfiField {
    pub name: String,
    pub typ: EfiType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EfiRecord {
    EfiStruct { name: String, fields: Vec<EfiField> },
    EfiUnion { name: String, fields: Vec<EfiField> },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EfiProtocol {
    pub name: String,
    pub methods: Vec<EfiMethod>,
    pub fields: Vec<EfiField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EfiModule {
    pub protocols: Vec<EfiProtocol>,
    pub records: Vec<EfiRecord>,
}