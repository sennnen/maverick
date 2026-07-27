use crate::{engine, memory, Artifact, LimitProfile};
use mav_connector_abi::{
    decode_canonical, encode_canonical, unpack_ptr_len, ActionBatch, ConnectorEvent,
};
use mav_model::error::{codes, MavError, Result};
use wasmi::errors::ErrorKind;
use wasmi::{Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TrapCode, TypedFunc};

pub(crate) struct HostState {
    limits: StoreLimits,
}

pub struct ConnectorInstance {
    store: Store<HostState>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    dealloc: TypedFunc<(i32, i32), ()>,
    init: TypedFunc<(i32, i32), i64>,
    handle: TypedFunc<(i32, i32), i64>,
    snapshot: TypedFunc<(), i64>,
    profile: LimitProfile,
    /// The guest buffer host events are written into, kept between calls. Allocating and freeing it
    /// per event meant two extra interpreter calls and two guest allocator runs for every strap
    /// notification — once a second, all day, for nothing.
    input_slot: Option<(u32, u32)>,
    usable: bool,
}

impl ConnectorInstance {
    pub fn instantiate(artifact: &Artifact, profile: LimitProfile) -> Result<Self> {
        if !profile.matches(&artifact.report().manifest.artifact_limits_profile) {
            return Err(error(
                codes::CONNECTOR_RUNTIME_LIMIT_PROFILE,
                "selected host profile differs from signed manifest profile",
            ));
        }
        let (engine, module) = artifact.compiled(|| {
            engine::preflight(artifact.bytes(), &profile)?;
            let engine = engine::engine(&artifact.report().abi, &profile)?;
            let module = Module::new(&engine, artifact.bytes()).map_err(|source| {
                let code = match source.kind() {
                    ErrorKind::Limits(_) => codes::CONNECTOR_RUNTIME_MODULE_LIMIT,
                    ErrorKind::Wasm(_) => codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
                    _ => codes::CONNECTOR_RUNTIME_INSTANTIATION,
                };
                error(code, format!("module compilation failed: {source}"))
            })?;
            Ok((engine, module))
        })?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(profile.max_memory_bytes)
            .table_elements(profile.max_table_elements as usize)
            .instances(1)
            .tables(profile.max_tables as usize)
            .memories(profile.max_memories as usize)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(engine, HostState { limits });
        store.limiter(|state| &mut state.limits);
        store.set_fuel(profile.fuel_per_call).map_err(|source| {
            error(
                codes::CONNECTOR_RUNTIME_INSTANTIATION,
                format!("initial fuel setup failed: {source}"),
            )
        })?;
        let linker = Linker::<HostState>::new(engine);
        let instance = linker
            .instantiate_and_start(&mut store, module)
            .map_err(|source| map_wasmi(source, "module instantiation"))?;
        let memory = instance.get_memory(&store, "memory").ok_or_else(|| {
            error(
                codes::CONNECTOR_RUNTIME_EXPORT_INVALID,
                "required memory export is missing or has the wrong kind",
            )
        })?;
        let abi_version = typed::<(), i64>(&instance, &store, "mav_abi_version")?;
        let alloc = typed::<i32, i32>(&instance, &store, "mav_alloc")?;
        let dealloc = typed::<(i32, i32), ()>(&instance, &store, "mav_dealloc")?;
        let init = typed::<(i32, i32), i64>(&instance, &store, "mav_init")?;
        let handle = typed::<(i32, i32), i64>(&instance, &store, "mav_handle")?;
        let snapshot = typed::<(), i64>(&instance, &store, "mav_snapshot")?;
        let version = abi_version
            .call(&mut store, ())
            .map_err(|source| map_wasmi(source, "ABI version call"))?;
        if version != mav_connector_abi::pack_ptr_len(1, 0) {
            return Err(error(
                codes::CONNECTOR_RUNTIME_EXPORT_INVALID,
                "mav_abi_version did not report ABI 1.0",
            ));
        }
        Ok(Self {
            store,
            memory,
            alloc,
            dealloc,
            init,
            handle,
            snapshot,
            profile,
            input_slot: None,
            usable: true,
        })
    }

    pub fn init(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        self.invoke_event(event, self.init)
    }

    pub fn handle(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        self.invoke_event(event, self.handle)
    }

    pub fn snapshot(&mut self) -> Result<Vec<u8>> {
        self.ensure_usable()?;
        self.reset_fuel()?;
        let packed = self
            .snapshot
            .call(&mut self.store, ())
            .map_err(|source| self.fail_wasmi(source, "snapshot call"))?;
        if packed == 0 {
            return Ok(Vec::new());
        }
        if packed < 0 {
            return Err(self.fail(error(
                codes::CONNECTOR_RUNTIME_SNAPSHOT_FAILED,
                "connector reported that building its snapshot failed",
            )));
        }
        let (pointer, length) = unpack_ptr_len(packed);
        if length as usize > self.profile.max_state_bytes {
            return Err(self.fail(error(
                codes::CONNECTOR_RUNTIME_STATE_OVERSIZED,
                "connector snapshot exceeds state byte limit",
            )));
        }
        let (bytes, _) = memory::read(self.memory, &self.store, pointer, length)
            .map_err(|error| self.fail(error))?;
        self.deallocate(pointer, length)?;
        Ok(bytes)
    }

    pub fn is_usable(&self) -> bool {
        self.usable
    }

    pub(crate) fn resource_usage(&self) -> Result<(u64, u64)> {
        let remaining = self.store.get_fuel().map_err(|source| {
            error(
                codes::CONNECTOR_RUNTIME_INSTANTIATION,
                format!("fuel inspection failed: {source}"),
            )
        })?;
        let consumed = self.profile.fuel_per_call.saturating_sub(remaining);
        let memory_bytes = u64::try_from(self.memory.data_size(&self.store)).map_err(|_| {
            error(
                codes::CONNECTOR_RUNTIME_RESOURCE_LIMIT,
                "linear memory size exceeds the host report range",
            )
        })?;
        Ok((consumed, memory_bytes))
    }

    fn invoke_event(
        &mut self,
        event: &ConnectorEvent,
        function: TypedFunc<(i32, i32), i64>,
    ) -> Result<ActionBatch> {
        self.ensure_usable()?;
        let input = encode_canonical(event).map_err(|source| {
            error(
                codes::CONNECTOR_RUNTIME_INPUT_INVALID,
                format!("host event is not canonical ABI input: {source}"),
            )
        })?;
        if input.len() > self.profile.max_input_bytes {
            return Err(error(
                codes::CONNECTOR_RUNTIME_INPUT_OVERSIZED,
                "canonical event exceeds input byte limit",
            ));
        }
        let input_length = i32::try_from(input.len()).map_err(|_| {
            error(
                codes::CONNECTOR_RUNTIME_INPUT_OVERSIZED,
                "canonical event exceeds ABI i32 length",
            )
        })?;
        self.reset_fuel()?;
        let input_pointer_bits = self.input_buffer(input_length)?;
        let input_range = memory::write(self.memory, &mut self.store, input_pointer_bits, &input)
            .map_err(|error| self.fail(error))?;
        let input_pointer = input_pointer_bits as i32;
        let packed = function
            .call(&mut self.store, (input_pointer, input_length))
            .map_err(|source| self.fail_wasmi(source, "connector event call"))?;
        let (output_pointer, output_length) = unpack_ptr_len(packed);
        if output_length == 0 {
            return Err(self.fail(error(
                codes::CONNECTOR_RUNTIME_OUTPUT_INVALID,
                "connector returned an empty action batch",
            )));
        }
        if output_length as usize > self.profile.max_output_bytes {
            return Err(self.fail(error(
                codes::CONNECTOR_RUNTIME_OUTPUT_OVERSIZED,
                "connector action batch exceeds output byte limit",
            )));
        }
        let (output, output_range) =
            memory::read(self.memory, &self.store, output_pointer, output_length)
                .map_err(|error| self.fail(error))?;
        if memory::overlaps(&input_range, &output_range) {
            return Err(self.fail(error(
                codes::CONNECTOR_RUNTIME_MEMORY_ACCESS,
                "connector output overlaps its input allocation",
            )));
        }
        self.deallocate(output_pointer, output_length)?;
        decode_canonical(&output).map_err(|source| {
            self.fail(error(
                codes::CONNECTOR_RUNTIME_OUTPUT_INVALID,
                format!("connector action batch rejected: {source}"),
            ))
        })
    }

    /// A guest buffer of at least `length` bytes, reusing the one from the last event when it is
    /// big enough. Grown by replacement rather than in place, because the guest allocator has no
    /// realloc in the ABI.
    fn input_buffer(&mut self, length: i32) -> Result<u32> {
        let wanted = length as u32;
        if let Some((pointer, capacity)) = self.input_slot {
            if capacity >= wanted {
                return Ok(pointer);
            }
            self.deallocate(pointer, capacity)?;
            self.input_slot = None;
        }
        let pointer = self
            .alloc
            .call(&mut self.store, length)
            .map_err(|source| self.fail_wasmi(source, "mav_alloc"))? as u32;
        self.input_slot = Some((pointer, wanted));
        Ok(pointer)
    }

    fn deallocate(&mut self, pointer: u32, length: u32) -> Result<()> {
        self.dealloc
            .call(&mut self.store, (pointer as i32, length as i32))
            .map_err(|source| self.fail_wasmi(source, "mav_dealloc"))
    }

    fn reset_fuel(&mut self) -> Result<()> {
        self.store
            .set_fuel(self.profile.fuel_per_call)
            .map_err(|source| self.fail_wasmi(source, "fuel reset"))
    }

    fn ensure_usable(&self) -> Result<()> {
        if !self.usable {
            return Err(error(
                codes::CONNECTOR_RUNTIME_INSTANCE_UNUSABLE,
                "connector instance was invalidated by a prior failure",
            ));
        }
        Ok(())
    }

    fn fail_wasmi(&mut self, source: wasmi::Error, context: &str) -> MavError {
        self.fail(map_wasmi(source, context))
    }

    fn fail(&mut self, error: MavError) -> MavError {
        self.usable = false;
        error
    }
}

fn typed<Params, Results>(
    instance: &wasmi::Instance,
    store: &Store<HostState>,
    name: &str,
) -> Result<TypedFunc<Params, Results>>
where
    Params: wasmi::WasmParams,
    Results: wasmi::WasmResults,
{
    instance.get_typed_func(store, name).map_err(|source| {
        error(
            codes::CONNECTOR_RUNTIME_EXPORT_INVALID,
            format!("required export {name} has the wrong signature: {source}"),
        )
    })
}

fn map_wasmi(source: wasmi::Error, context: &str) -> MavError {
    let code = match source.as_trap_code() {
        Some(TrapCode::OutOfFuel) => codes::CONNECTOR_RUNTIME_FUEL_EXHAUSTED,
        Some(TrapCode::StackOverflow) => codes::CONNECTOR_RUNTIME_STACK_LIMIT,
        Some(TrapCode::GrowthOperationLimited) => codes::CONNECTOR_RUNTIME_RESOURCE_LIMIT,
        Some(_) => codes::CONNECTOR_RUNTIME_TRAP,
        None => match source.kind() {
            ErrorKind::Memory(_) | ErrorKind::Table(_) => codes::CONNECTOR_RUNTIME_RESOURCE_LIMIT,
            _ => codes::CONNECTOR_RUNTIME_INSTANTIATION,
        },
    };
    error(code, format!("{context} failed: {source}"))
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}
