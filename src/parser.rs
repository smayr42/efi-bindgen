extern crate clang;

use clang::*;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::fs::{canonicalize, File};
use std::error::Error;
use std::iter::Iterator;
use types::*;

trait Check<T> {
    fn and_check<F>(self, f: F) -> Option<T> where F: FnOnce(&T) -> bool;
}

impl<T> Check<T> for Option<T> {
    fn and_check<F>(self, f: F) -> Option<T>
        where F: FnOnce(&T) -> bool
    {
        match self {
            Some(x) => if f(&x) { Some(x) } else { None },
            None => None,
        }
    }
}

fn type_name(typ: &Type) -> String {
    String::from(typ.get_declaration()
        .and_then(|d| d.get_name())
        .unwrap_or_else(|| typ.get_display_name())
        .trim_left_matches('_'))
}

fn to_efi_type(typ: &Type) -> Result<EfiType, String> {
    match typ.get_kind() {
        TypeKind::Record => {
            let name = type_name(typ);
            match name.as_ref() {
                "efi_status" => Ok(EfiType::Status),
                "efi_uintn" => Ok(EfiType::UIntN),
                "efi_intn" => Ok(EfiType::IntN),
                "efi_bool" => Ok(EfiType::Bool),
                "efi_int8" => Ok(EfiType::Int8),
                "efi_uint8" => Ok(EfiType::UInt8),
                "efi_int16" => Ok(EfiType::Int16),
                "efi_uint16" => Ok(EfiType::UInt16),
                "efi_int32" => Ok(EfiType::Int32),
                "efi_uint32" => Ok(EfiType::UInt32),
                "efi_int64" => Ok(EfiType::Int64),
                "efi_uint64" => Ok(EfiType::UInt64),
                "efi_char8" => Ok(EfiType::Char8),
                "efi_char16" => Ok(EfiType::Char16),
                _ => Ok(EfiType::Id(name)),
            }
        }
        TypeKind::Enum => Ok(EfiType::Id(type_name(typ))),
        TypeKind::Pointer => {
            let pointee = &try!(typ.get_pointee_type().ok_or("pointer has not pointee type"));
            let typ = try!(to_efi_type(pointee));
            Ok(EfiType::Ptr(Box::new(typ)))
        }
        TypeKind::Typedef => {
            let ctyp = &typ.get_canonical_type();
            Ok(to_efi_type(ctyp).unwrap_or_else(|_| EfiType::Id(type_name(typ))))
        }
        _ => Err(String::from(format!("unsupported type {:?}", typ))),
    }
}

fn to_efi_argdir(typ: &Type) -> Option<EfiArgDir> {
    typ.get_canonical_type().get_declaration().and_then(|d| d.get_name()).and_then(|name| {
        match name.as_ref() {
            "efi_arg_in" => Some(EfiArgDir::In),
            "efi_arg_out" => Some(EfiArgDir::Out),
            _ => None,
        }
    })
}

fn to_efi_argopt(typ: &Type) -> bool {
    typ.get_canonical_type()
        .get_declaration()
        .and_then(|d| d.get_name())
        .and_check(|name| name == "efi_arg_optional")
        .is_some()
}

fn to_efi_method(typ: &Type) -> Result<EfiMethod, String> {
    let res = &try!(typ.get_result_type().ok_or("function prototype without result"));
    let args = try!(typ.get_argument_types().ok_or("function prototype without args"));

    let mut efi_args: Vec<EfiArg> = Vec::new();
    let mut dir = EfiArgDir::In;

    for ref arg in args {
        if let Some(new_dir) = to_efi_argdir(arg) {
            dir = new_dir;
            continue;
        }

        if to_efi_argopt(arg) {
            if let Some(last) = efi_args.last_mut() {
                last.optional = true;
            }
            continue;
        }

        efi_args.push(EfiArg {
            name: String::new(),
            typ: try!(to_efi_type(arg)),
            dir: dir,
            optional: false,
        });
    }

    Ok(EfiMethod {
        name: String::new(),
        typ: try!(to_efi_type(res)),
        args: efi_args,
    })
}

fn process_typedef(entity: &Entity, module: &mut EfiModule) -> Result<(), String> {
    let name = try!(entity.get_name().ok_or("typedef without name"));

    if !name.starts_with("EFI_") {
        return Err(format!("unknown typedef {}", name));
    }
    if name.ends_with("_PROTOCOL") {
        return Ok(());
    }

    let typ = try!(entity.get_typedef_underlying_type().ok_or("efi typedef without type"));
    if let Some(fields) = typ.get_canonical_type().get_fields() {
        let mut efi_fields = Vec::new();

        for ref field in fields {
            let typ = &try!(field.get_type().ok_or("field without type"));
            efi_fields.push(EfiField {
                name: try!(field.get_name().ok_or("field without name")),
                typ: try!(to_efi_type(typ)),
            });
        }

        let kind = try!(typ.get_declaration()
            .map(|d| d.get_kind())
            .ok_or("record type without declaration"));

        module.records.push(match kind {
            EntityKind::StructDecl => {
                EfiRecord::EfiStruct {
                    name: name,
                    fields: efi_fields,
                }
            }
            EntityKind::UnionDecl => {
                EfiRecord::EfiUnion {
                    name: name,
                    fields: efi_fields,
                }
            }
            _ => return Err(String::from(format!("unsupported type {:?}", typ))),
        })
    }

    Ok(())
}

fn process_method_args(entity: &Entity, args: &mut Iterator<Item = &mut EfiArg>) {
    if entity.get_kind() == EntityKind::ParmDecl {
        if let Some(name) = entity.get_name() {
            let arg = args.next().expect("missing argument name");
            arg.name = name;
        }
    }

    for ref child in entity.get_children() {
        process_method_args(child, args);
    }
}

fn process_struct(entity: &Entity, module: &mut EfiModule) -> Result<(), String> {
    if let Some(name) = entity.get_name()
        .and_check(|n| n.starts_with("_EFI") && n.ends_with("PROTOCOL")) {
        let mut protocol = EfiProtocol {
            name: String::from(&name[1..]),
            methods: Vec::new(),
            fields: Vec::new(),
        };

        let fields = try!(entity.get_type()
            .and_then(|t| t.get_fields())
            .ok_or("protocol definition without fields"));
        for ref field in fields {
            let name = try!(field.get_name().ok_or("field lacks name"));
            let ftype = &try!(field.get_type().ok_or("protocol field lacks type"));

            if let Some(ref ptype) = ftype.get_canonical_type()
                .get_pointee_type()
                .and_check(|typ| typ.get_kind() == TypeKind::FunctionPrototype) {
                let decl = &try!(field.get_type()
                    .and_then(|t| t.get_declaration())
                    .ok_or("method lacks declaration"));
                let mut method = try!(to_efi_method(ptype));
                process_method_args(decl, &mut method.args.iter_mut());
                method.name = name;
                protocol.methods.push(method);
            } else {
                protocol.fields.push(EfiField {
                    name: name,
                    typ: try!(to_efi_type(ftype)),
                });
            }
        }

        module.protocols.push(protocol);
    }

    Ok(())
}

fn process_tu(entity: &Entity, efi_header: &Path, module: &mut EfiModule) -> Result<(), String> {
    if entity.get_location()
        .and_check(|loc| loc.get_file_location().file.get_path() == efi_header)
        .is_some() {
        match entity.get_kind() {
            EntityKind::TypedefDecl => return process_typedef(entity, module),
            EntityKind::StructDecl => return process_struct(entity, module),
            _ => {}
        }
    }

    for ref child in entity.get_children() {
        try!(process_tu(child, efi_header, module));
    }

    Ok(())
}

fn write_aux_header<P: AsRef<Path>, Q: AsRef<Path>>(efi_header: P,
                                                    out_header: Q)
                                                    -> io::Result<PathBuf> {
    let template = include_str!("template.h");
    let efi_header_path = try!(canonicalize(efi_header));

    let header = format!("{}\n#include \"{}\"",
                         template,
                         (*efi_header_path).to_string_lossy());

    let mut aux = try!(File::create(out_header));
    try!(aux.write_all(header.as_bytes()));
    Ok(efi_header_path)
}

pub fn parse(efi_header: &str) -> Result<EfiModule, Box<Error>> {
    let efi_header = try!(write_aux_header(efi_header, "aux.h"));
    let clang = try!(Clang::new());
    let index = Index::new(&clang, false, true);
    let tu = try!(index.parser("aux.h").arguments(["-fsyntax-only"].as_ref()).parse());
    let mut proto = EfiModule {
        protocols: Vec::new(),
        records: Vec::new(),
    };

    try!(process_tu(&tu.get_entity(), &efi_header, &mut proto));
    Ok(proto)
}