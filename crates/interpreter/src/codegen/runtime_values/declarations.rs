//! Source declarations remain unchanged; codegen consumers use these physical
//! signatures to agree with independently compiled producers.

use crate::{MangledName, PackageDeclaration, RuntimeFunction, Type, TypedAst};
use std::collections::BTreeMap;

pub(crate) fn signatures(ast: &TypedAst) -> BTreeMap<MangledName, RuntimeFunction> {
    let mut signatures = BTreeMap::new();
    for function in &ast.functions {
        signatures.insert(
            function.mangled_name.clone(),
            RuntimeFunction {
                params: function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                ret: function.return_type.clone(),
            },
        );
    }
    for declaration in &ast.types {
        let crate::TypedTypeDecl::Class(class) = declaration else {
            continue;
        };
        for method in &class.methods {
            signatures.insert(
                crate::mangle::extend(&class.mangled_name, &method.name.name),
                RuntimeFunction {
                    params: method.params.iter().map(|param| param.ty.clone()).collect(),
                    ret: method.return_type.clone(),
                },
            );
        }
        for accessor in &class.accessors {
            let (name, params, ret) = match accessor {
                crate::TypedClassAccessor::Getter { name, ret_ty, .. } => {
                    (format!("get {}", name.name), vec![], ret_ty.clone())
                }
                crate::TypedClassAccessor::Setter { name, param, .. } => (
                    format!("set {}", name.name),
                    vec![param.ty.clone()],
                    Type::Void,
                ),
            };
            signatures.insert(
                crate::mangle::extend(&class.mangled_name, &name),
                RuntimeFunction { params, ret },
            );
        }
        signatures.insert(
            crate::mangle::extend(&class.mangled_name, "constructor"),
            RuntimeFunction {
                params: class
                    .effective_ctor_params()
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                ret: Type::class_ref(
                    crate::Package(ast.package_name.clone()),
                    class.name.name.clone(),
                    class.mangled_name.clone(),
                    vec![],
                ),
            },
        );
    }
    for export in &ast.exports {
        if let Some(signature) = signatures.get(&export.target).cloned() {
            signatures.insert(export.public_name.clone(), signature);
        }
    }
    signatures
}

pub(crate) fn global_types(ast: &TypedAst) -> BTreeMap<MangledName, Type> {
    let mut globals: BTreeMap<_, _> = ast
        .globals
        .iter()
        .map(|global| (global.mangled_name.clone(), global.ty.clone()))
        .collect();
    for export in &ast.exports {
        if let Some(ty) = globals.get(&export.target).cloned() {
            globals.insert(export.public_name.clone(), ty);
        }
    }
    globals
}

pub(crate) fn lower_declaration(source: &PackageDeclaration) -> PackageDeclaration {
    let mut lowered = source.clone();
    for value in lowered.values.values_mut() {
        match &mut value.kind {
            crate::ValueKind::Function { params, ret, .. } => {
                if let Some(signature) = source.runtime_functions.get(&value.mangled_name) {
                    set_params(params, &signature.params);
                    *ret = signature.ret.clone();
                }
            }
            crate::ValueKind::Let { ty, .. } | crate::ValueKind::Const { ty, .. } => {
                if let Some(runtime) = source.runtime_globals.get(&value.mangled_name) {
                    *ty = runtime.clone();
                }
            }
        }
    }
    for symbol in lowered
        .types
        .values_mut()
        .chain(lowered.runtime_types.values_mut())
    {
        let crate::TypeKind::Class {
            methods,
            constructor,
            accessors,
            statics,
            ..
        } = &mut symbol.kind
        else {
            continue;
        };
        for (name, method) in methods {
            if let Some(signature) = source
                .runtime_functions
                .get(&crate::mangle::extend(&symbol.mangled_name, name))
            {
                set_params(&mut method.params, &signature.params);
                method.ret = signature.ret.clone();
            }
        }
        for (name, method) in statics {
            if let Some(signature) = source
                .runtime_functions
                .get(&crate::mangle::static_member(&symbol.mangled_name, name))
            {
                set_params(&mut method.params, &signature.params);
                method.ret = signature.ret.clone();
            }
        }
        if let Some(signature) = source
            .runtime_functions
            .get(&crate::mangle::extend(&symbol.mangled_name, "constructor"))
        {
            set_params(constructor, &signature.params);
        }
        for accessor in accessors {
            match accessor {
                crate::AccessorSig::Getter { name, ret_ty } => {
                    if let Some(signature) = source.runtime_functions.get(&crate::mangle::extend(
                        &symbol.mangled_name,
                        &format!("get {name}"),
                    )) {
                        *ret_ty = signature.ret.clone();
                    }
                }
                crate::AccessorSig::Setter { name, param } => {
                    if let Some(signature) = source.runtime_functions.get(&crate::mangle::extend(
                        &symbol.mangled_name,
                        &format!("set {name}"),
                    )) {
                        set_params(std::slice::from_mut(param), &signature.params);
                    }
                }
            }
        }
    }
    lowered
}

fn set_params(params: &mut [crate::Param], types: &[Type]) {
    for (param, ty) in params.iter_mut().zip(types) {
        param.ty = ty.clone();
    }
}
