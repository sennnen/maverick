use crate::LimitProfile;
use mav_connector_abi::{AbiDescriptor, WasmFeature};
use mav_model::error::{codes, MavError, Result};
use wasmi::{CompilationMode, Config, EnforcedLimits, Engine};
use wasmparser::{ElementItems, Encoding, Operator, Parser, Payload, RefType, TableInit, ValType};

pub(crate) fn engine(abi: &AbiDescriptor, profile: &LimitProfile) -> Result<Engine> {
    if !profile.is_valid() {
        return Err(error(
            codes::CONNECTOR_RUNTIME_LIMIT_PROFILE,
            "host limit profile values differ from frozen mobile-v1",
        ));
    }
    let mutable_globals = abi.wasm_features.contains(&WasmFeature::MutableGlobals);
    let sign_extension = abi.wasm_features.contains(&WasmFeature::SignExtension);
    let bulk_memory = abi.wasm_features.contains(&WasmFeature::BulkMemory);
    let mut config = Config::default();
    config
        .compilation_mode(CompilationMode::Eager)
        .consume_fuel(true)
        .ignore_custom_sections(true)
        .set_max_recursion_depth(profile.max_recursion_depth)
        .set_max_stack_height(profile.max_value_stack_height)
        .set_max_cached_stacks(0)
        .wasm_mutable_global(mutable_globals)
        .wasm_sign_extension(sign_extension)
        .wasm_bulk_memory(bulk_memory)
        .wasm_saturating_float_to_int(false)
        .wasm_multi_value(false)
        .wasm_multi_memory(false)
        .wasm_reference_types(true)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_custom_page_sizes(false)
        .wasm_memory64(false)
        .wasm_wide_arithmetic(false)
        .enforced_limits(EnforcedLimits::strict());
    Ok(Engine::new(&config))
}

pub(crate) fn preflight(bytes: &[u8], profile: &LimitProfile) -> Result<()> {
    let mut functions = 0_u32;
    let mut globals = 0_u32;
    let mut tables = 0_u32;
    let mut memories = 0_u32;
    let mut elements = 0_u32;
    let mut data = 0_u32;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|source| {
            error(
                codes::CONNECTOR_RUNTIME_INSTANTIATION,
                format!("runtime preflight parse failed: {source}"),
            )
        })? {
            Payload::Version {
                encoding: Encoding::Module,
                ..
            } => {}
            Payload::Version { .. } => {
                return Err(error(
                    codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                    "component model artifacts are forbidden",
                ));
            }
            Payload::ImportSection(reader) if reader.count() != 0 => {
                return Err(error(
                    codes::CONNECTOR_RUNTIME_IMPORT_FORBIDDEN,
                    "ABI v1 connector modules cannot import symbols",
                ));
            }
            Payload::StartSection { .. } => {
                return Err(error(
                    codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                    "connector start functions are forbidden",
                ));
            }
            Payload::FunctionSection(reader) => functions = reader.count(),
            Payload::TypeSection(reader) => {
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty.map_err(|source| {
                        error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            format!("function type rejected: {source}"),
                        )
                    })?;
                    if ty
                        .params()
                        .iter()
                        .chain(ty.results())
                        .any(|value| matches!(value, ValType::Ref(_)))
                    {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "reference-type function signatures are forbidden",
                        ));
                    }
                }
            }
            Payload::GlobalSection(reader) => {
                globals = reader.count();
                for global in reader {
                    let global = global.map_err(|source| {
                        error(
                            codes::CONNECTOR_RUNTIME_INSTANTIATION,
                            format!("global declaration rejected: {source}"),
                        )
                    })?;
                    if global.ty.shared || matches!(global.ty.content_type, ValType::Ref(_)) {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "shared and reference-type globals are forbidden",
                        ));
                    }
                }
            }
            Payload::TableSection(reader) => {
                tables = reader.count();
                for table in reader {
                    let table = table.map_err(|source| {
                        error(
                            codes::CONNECTOR_RUNTIME_INSTANTIATION,
                            format!("table declaration rejected: {source}"),
                        )
                    })?;
                    if table.ty.shared || table.ty.table64 {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "shared and 64-bit tables are forbidden",
                        ));
                    }
                    if table.ty.element_type != RefType::FUNCREF {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "non-function reference tables are forbidden",
                        ));
                    }
                    if matches!(table.init, TableInit::Expr(_)) {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "table initializer expressions are forbidden",
                        ));
                    }
                    if table.ty.initial > profile.max_table_elements {
                        return Err(module_limit("table element minimum"));
                    }
                }
            }
            Payload::MemorySection(reader) => {
                memories = reader.count();
                for memory in reader {
                    let memory = memory.map_err(|source| {
                        error(
                            codes::CONNECTOR_RUNTIME_INSTANTIATION,
                            format!("memory declaration rejected: {source}"),
                        )
                    })?;
                    if memory.shared || memory.memory64 || memory.page_size_log2.is_some() {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "shared, 64-bit, and custom-page memories are forbidden",
                        ));
                    }
                    let initial_bytes = memory
                        .initial
                        .checked_mul(65_536)
                        .ok_or_else(|| module_limit("linear memory minimum overflows host size"))?;
                    if initial_bytes > profile.max_memory_bytes as u64 {
                        return Err(module_limit("linear memory minimum"));
                    }
                }
            }
            Payload::ElementSection(reader) => {
                elements = reader.count();
                for element in reader {
                    let element = element.map_err(|source| {
                        error(
                            codes::CONNECTOR_RUNTIME_INSTANTIATION,
                            format!("element declaration rejected: {source}"),
                        )
                    })?;
                    if matches!(element.items, ElementItems::Expressions(_, _)) {
                        return Err(error(
                            codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                            "reference-type element expressions are forbidden",
                        ));
                    }
                }
            }
            Payload::DataSection(reader) => data = reader.count(),
            Payload::TagSection(_) => {
                return Err(error(
                    codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                    "exception tags are forbidden",
                ));
            }
            Payload::CodeSectionEntry(body) => {
                let mut operators = body.get_operators_reader().map_err(|source| {
                    error(
                        codes::CONNECTOR_RUNTIME_INSTANTIATION,
                        format!("function operators rejected: {source}"),
                    )
                })?;
                while !operators.eof() {
                    let operator = operators.read().map_err(|source| {
                        error(
                            codes::CONNECTOR_RUNTIME_INSTANTIATION,
                            format!("function operator rejected: {source}"),
                        )
                    })?;
                    match operator {
                        Operator::TypedSelect { .. }
                        | Operator::TypedSelectMulti { .. }
                        | Operator::RefNull { .. }
                        | Operator::RefIsNull
                        | Operator::RefFunc { .. }
                        | Operator::TableFill { .. }
                        | Operator::TableGet { .. }
                        | Operator::TableSet { .. }
                        | Operator::TableGrow { .. }
                        | Operator::TableSize { .. }
                        | Operator::ReturnCallIndirect { .. } => {
                            return Err(error(
                                codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                                "reference-type and tail-call operators are forbidden",
                            ));
                        }
                        Operator::CallIndirect { table_index, .. } if table_index != 0 => {
                            return Err(error(
                                codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                                "indirect calls may use only the MVP function table",
                            ));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    for (actual, maximum, label) in [
        (functions, profile.max_functions, "functions"),
        (globals, profile.max_globals, "globals"),
        (tables, profile.max_tables, "tables"),
        (memories, profile.max_memories, "memories"),
        (elements, profile.max_element_segments, "element segments"),
        (data, profile.max_data_segments, "data segments"),
    ] {
        if actual > maximum {
            return Err(module_limit(label));
        }
    }
    if memories != 1 {
        return Err(module_limit("exactly one linear memory is required"));
    }
    Ok(())
}

fn module_limit(label: &'static str) -> MavError {
    error(
        codes::CONNECTOR_RUNTIME_MODULE_LIMIT,
        format!("connector module exceeds {label} limit"),
    )
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}
