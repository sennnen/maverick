use mav_connector_abi::LimitsProfileId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitProfile {
    pub(crate) id: String,
    pub(crate) max_functions: u32,
    pub(crate) max_globals: u32,
    pub(crate) max_tables: u32,
    pub(crate) max_table_elements: u64,
    pub(crate) max_memories: u32,
    pub(crate) max_element_segments: u32,
    pub(crate) max_data_segments: u32,
    pub(crate) max_memory_bytes: usize,
    pub(crate) max_recursion_depth: usize,
    pub(crate) max_value_stack_height: usize,
    pub(crate) max_input_bytes: usize,
    pub(crate) max_output_bytes: usize,
    pub(crate) max_state_bytes: usize,
    pub(crate) fuel_per_call: u64,
}

impl LimitProfile {
    pub fn mobile_v1() -> Self {
        Self {
            id: "mobile-v1".to_owned(),
            max_functions: 4_096,
            max_globals: 1_000,
            max_tables: 1,
            max_table_elements: 1_024,
            max_memories: 1,
            max_element_segments: 1_000,
            max_data_segments: 1_000,
            max_memory_bytes: 4 * 1024 * 1024,
            max_recursion_depth: 128,
            max_value_stack_height: 256 * 1024,
            max_input_bytes: 64 * 1024,
            max_output_bytes: 1024 * 1024,
            max_state_bytes: 64 * 1024,
            fuel_per_call: 5_000_000,
        }
    }

    pub(crate) fn matches(&self, requested: &LimitsProfileId) -> bool {
        self.id == requested.as_str()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.id == "mobile-v1"
            && self.max_functions == 4_096
            && self.max_globals == 1_000
            && self.max_tables == 1
            && self.max_table_elements == 1_024
            && self.max_memories == 1
            && self.max_element_segments == 1_000
            && self.max_data_segments == 1_000
            && self.max_memory_bytes == 4 * 1024 * 1024
            && self.max_recursion_depth == 128
            && self.max_value_stack_height == 256 * 1024
            && self.max_input_bytes == 64 * 1024
            && self.max_output_bytes == 1024 * 1024
            && self.max_state_bytes == 64 * 1024
            && self.fuel_per_call > 0
            && self.fuel_per_call <= 5_000_000
    }

    pub(crate) fn for_fixture(&self, max_fuel: u64) -> Self {
        let mut profile = self.clone();
        profile.fuel_per_call = profile.fuel_per_call.min(max_fuel);
        profile
    }
}
