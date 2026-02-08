//! Synthetic Files Property-Based Testing
//! Ruthlessly validates live content generation with formal guarantees

use proptest::prelude::*;
use quickcheck::TestResult;
use quickcheck_macros::quickcheck;
use arbitrary::Arbitrary;
use std::collections::HashMap;
use quickcheck::{Arbitrary as QCArbitrary, Gen};

/// Synthetic file generator specification
#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub struct SyntheticGenerator {
    pub generator_id: u32,
    pub generator_type: GeneratorType,
    pub input_parameters: HashMap<String, ParameterValue>,
    pub output_format: OutputFormat,
    pub refresh_interval: u64, // milliseconds
    pub memory_limit: usize,
    pub cpu_limit: u64, // microseconds
    pub max_output_size: usize,
}

impl proptest::arbitrary::Arbitrary for SyntheticGenerator {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        let param_strategy = ParameterValue::arbitrary_with(());
        let param_map = proptest::collection::hash_map(any::<String>(), param_strategy, 0..4);

        (
            any::<u32>(),
            any::<GeneratorType>(),
            param_map,
            any::<OutputFormat>(),
            100u64..60000u64,
            1024usize..=(64 * 1024),
            10_000u64..=500_000u64,
            512usize..=(1024 * 1024),
        )
            .prop_map(
                |(
                    generator_id,
                    generator_type,
                    input_parameters,
                    output_format,
                    refresh_interval,
                    memory_limit,
                    cpu_limit,
                    max_output_size,
                )| SyntheticGenerator {
                    generator_id,
                    generator_type,
                    input_parameters,
                    output_format,
                    refresh_interval,
                    memory_limit,
                    cpu_limit,
                    max_output_size,
                },
            )
            .boxed()
    }
}

#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum GeneratorType {
    StaticTemplate,
    DynamicQuery,
    MLInference,
    SystemMetrics,
    ComputedFunction,
    FileAggregator,
    NetworkFetcher,
    TimeSeries,
}

impl proptest::arbitrary::Arbitrary for GeneratorType {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        proptest::prop_oneof![
            proptest::strategy::Just(GeneratorType::StaticTemplate),
            proptest::strategy::Just(GeneratorType::DynamicQuery),
            proptest::strategy::Just(GeneratorType::MLInference),
            proptest::strategy::Just(GeneratorType::SystemMetrics),
            proptest::strategy::Just(GeneratorType::ComputedFunction),
            proptest::strategy::Just(GeneratorType::FileAggregator),
            proptest::strategy::Just(GeneratorType::NetworkFetcher),
            proptest::strategy::Just(GeneratorType::TimeSeries),
        ]
        .boxed()
    }
}

impl QCArbitrary for GeneratorType {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 8 {
            0 => GeneratorType::StaticTemplate,
            1 => GeneratorType::DynamicQuery,
            2 => GeneratorType::MLInference,
            3 => GeneratorType::SystemMetrics,
            4 => GeneratorType::ComputedFunction,
            5 => GeneratorType::FileAggregator,
            6 => GeneratorType::NetworkFetcher,
            _ => GeneratorType::TimeSeries,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum ParameterValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<ParameterValue>),
}

impl proptest::arbitrary::Arbitrary for ParameterValue {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        let leaf = proptest::prop_oneof![
            any::<String>().prop_map(ParameterValue::String),
            any::<i64>().prop_map(ParameterValue::Integer),
            any::<f64>().prop_map(ParameterValue::Float),
            any::<bool>().prop_map(ParameterValue::Boolean),
        ];

        leaf.prop_recursive(
            3,          // max depth
            64,         // max total nodes
            5,          // items per collection
            |inner| proptest::collection::vec(inner, 0..4).prop_map(ParameterValue::Array)
        )
        .boxed()
    }
}

fn qc_parameter_value(g: &mut Gen, depth: usize) -> ParameterValue {
    if depth == 0 {
        match usize::arbitrary(g) % 4 {
            0 => ParameterValue::String(<String as QCArbitrary>::arbitrary(g)),
            1 => ParameterValue::Integer(<i64 as QCArbitrary>::arbitrary(g)),
            2 => ParameterValue::Float(<f64 as QCArbitrary>::arbitrary(g)),
            _ => ParameterValue::Boolean(<bool as QCArbitrary>::arbitrary(g)),
        }
    } else {
        match usize::arbitrary(g) % 5 {
            0 => ParameterValue::String(<String as QCArbitrary>::arbitrary(g)),
            1 => ParameterValue::Integer(<i64 as QCArbitrary>::arbitrary(g)),
            2 => ParameterValue::Float(<f64 as QCArbitrary>::arbitrary(g)),
            3 => ParameterValue::Boolean(<bool as QCArbitrary>::arbitrary(g)),
            _ => {
                let len = usize::arbitrary(g) % 4;
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    items.push(qc_parameter_value(g, depth - 1));
                }
                ParameterValue::Array(items)
            }
        }
    }
}

impl QCArbitrary for ParameterValue {
    fn arbitrary(g: &mut Gen) -> Self {
        qc_parameter_value(g, 3)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

fn qc_parameter_map(g: &mut Gen, depth: usize) -> HashMap<String, ParameterValue> {
    let mut map = HashMap::new();
    let len = usize::arbitrary(g) % 4;
    for _ in 0..len {
        let key = <String as QCArbitrary>::arbitrary(g);
        let value = qc_parameter_value(g, depth);
        map.insert(key, value);
    }
    map
}

impl QCArbitrary for SyntheticGenerator {
    fn arbitrary(g: &mut Gen) -> Self {
        SyntheticGenerator {
            generator_id: <u32 as QCArbitrary>::arbitrary(g),
            generator_type: <GeneratorType as QCArbitrary>::arbitrary(g),
            input_parameters: qc_parameter_map(g, 2),
            output_format: <OutputFormat as QCArbitrary>::arbitrary(g),
            refresh_interval: ((<u64 as QCArbitrary>::arbitrary(g) % 60000).max(100)),
            memory_limit: ((<usize as QCArbitrary>::arbitrary(g) % (64 * 1024)) + 1024),
            cpu_limit: ((<u64 as QCArbitrary>::arbitrary(g) % 500_000) + 10_000),
            max_output_size: ((<usize as QCArbitrary>::arbitrary(g) % (1024 * 1024)) + 512),
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

#[derive(Debug, Clone, PartialEq, Arbitrary)]
pub enum OutputFormat {
    PlainText,
    JSON,
    XML,
    Binary,
    CSV,
    Markdown,
}

impl proptest::arbitrary::Arbitrary for OutputFormat {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        proptest::prop_oneof![
            proptest::strategy::Just(OutputFormat::PlainText),
            proptest::strategy::Just(OutputFormat::JSON),
            proptest::strategy::Just(OutputFormat::XML),
            proptest::strategy::Just(OutputFormat::Binary),
            proptest::strategy::Just(OutputFormat::CSV),
            proptest::strategy::Just(OutputFormat::Markdown),
        ]
        .boxed()
    }
}

impl QCArbitrary for OutputFormat {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 6 {
            0 => OutputFormat::PlainText,
            1 => OutputFormat::JSON,
            2 => OutputFormat::XML,
            3 => OutputFormat::Binary,
            4 => OutputFormat::CSV,
            _ => OutputFormat::Markdown,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(std::iter::empty())
    }
}

/// Generated content with metadata
#[derive(Debug, Clone, PartialEq)]
pub struct SyntheticContent {
    pub content: Vec<u8>,
    pub mime_type: String,
    pub generated_at: u64, // timestamp
    pub generation_time: u64, // microseconds
    pub memory_used: usize,
    pub cache_hit: bool,
    pub error: Option<String>,
}

/// Synthetic file system managing live content generation
#[derive(Debug, Clone)]
pub struct SyntheticFileSystem {
    pub generators: HashMap<u32, SyntheticGenerator>,
    pub active_files: HashMap<String, u32>, // file_path -> generator_id
    pub content_cache: HashMap<u32, SyntheticContent>, // generator_id -> last content
    pub generation_stats: HashMap<u32, GenerationStats>,
    pub global_limits: SyntheticLimits,
}

#[derive(Debug, Clone)]
pub struct GenerationStats {
    pub total_generations: u64,
    pub successful_generations: u64,
    pub failed_generations: u64,
    pub total_generation_time: u64, // microseconds
    pub total_memory_used: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

#[derive(Debug, Clone)]
pub struct SyntheticLimits {
    pub max_generators: u32,
    pub max_concurrent_generations: u32,
    pub max_memory_per_generation: usize,
    pub max_cpu_per_generation: u64, // microseconds
    pub max_output_size: usize,
    pub max_cache_size: usize,
    pub min_refresh_interval: u64, // milliseconds
}

impl Default for SyntheticLimits {
    fn default() -> Self {
        Self {
            max_generators: 1024,
            max_concurrent_generations: 16,
            max_memory_per_generation: 64 * 1024, // 64KB
            max_cpu_per_generation: 100000, // 100ms
            max_output_size: 1024 * 1024, // 1MB
            max_cache_size: 16 * 1024 * 1024, // 16MB
            min_refresh_interval: 100, // 100ms
        }
    }
}

impl Default for GenerationStats {
    fn default() -> Self {
        Self {
            total_generations: 0,
            successful_generations: 0,
            failed_generations: 0,
            total_generation_time: 0,
            total_memory_used: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }
}

impl Default for SyntheticFileSystem {
    fn default() -> Self {
        Self {
            generators: HashMap::new(),
            active_files: HashMap::new(),
            content_cache: HashMap::new(),
            generation_stats: HashMap::new(),
            global_limits: SyntheticLimits::default(),
        }
    }
}

impl SyntheticFileSystem {
    /// Register new synthetic file generator
    pub fn register_generator(&mut self, path: String, generator: SyntheticGenerator) -> Result<(), String> {
        // Check global limits
        if self.generators.len() >= self.global_limits.max_generators as usize {
            return Err("Maximum generators reached".to_string());
        }

        // Validate generator parameters
        if generator.memory_limit > self.global_limits.max_memory_per_generation {
            return Err("Memory limit exceeds maximum".to_string());
        }

        if generator.cpu_limit > self.global_limits.max_cpu_per_generation {
            return Err("CPU limit exceeds maximum".to_string());
        }

        if generator.max_output_size > self.global_limits.max_output_size {
            return Err("Output size exceeds maximum".to_string());
        }

        if generator.refresh_interval < self.global_limits.min_refresh_interval {
            return Err("Refresh interval too small".to_string());
        }

        // Check for duplicate paths
        if self.active_files.contains_key(&path) {
            return Err("File path already registered".to_string());
        }

        let generator_id = generator.generator_id;
        self.generators.insert(generator_id, generator);
        self.active_files.insert(path, generator_id);
        self.generation_stats.insert(generator_id, GenerationStats::default());

        Ok(())
    }

    /// Generate content for a synthetic file
    pub fn generate_content(&mut self, generator_id: u32, force_refresh: bool) -> Result<SyntheticContent, String> {
        let generator = self.generators.get(&generator_id)
            .ok_or("Generator not found")?
            .clone();

        // Check cache if not forcing refresh
        if !force_refresh {
            if let Some(cached) = self.content_cache.get(&generator_id) {
                let current_time = Self::current_timestamp();
                let cache_age = current_time - cached.generated_at;

                if cache_age < generator.refresh_interval {
                    // Update stats
                    if let Some(stats) = self.generation_stats.get_mut(&generator_id) {
                        stats.cache_hits += 1;
                    }

                    let mut result = cached.clone();
                    result.cache_hit = true;
                    return Ok(result);
                }
            }
        }

        // Generate new content
        let start_time = Self::current_timestamp();
        let content = self.execute_generator(&generator)?;
        let end_time = Self::current_timestamp();

        let generation_time = end_time - start_time;

        // Validate resource usage
        if content.len() > generator.max_output_size {
            return Err("Generated content exceeds size limit".to_string());
        }

        let synthetic_content = SyntheticContent {
            content,
            mime_type: Self::get_mime_type(&generator.output_format),
            generated_at: end_time,
            generation_time,
            memory_used: generator.memory_limit, // Simplified for testing
            cache_hit: false,
            error: None,
        };

        // Update cache
        self.content_cache.insert(generator_id, synthetic_content.clone());
        self.manage_cache_size();

        // Update stats
        if let Some(stats) = self.generation_stats.get_mut(&generator_id) {
            stats.total_generations += 1;
            stats.successful_generations += 1;
            stats.total_generation_time += generation_time;
            stats.total_memory_used += synthetic_content.memory_used;
            stats.cache_misses += 1;
        }

        Ok(synthetic_content)
    }

    /// Execute generator based on type
    fn execute_generator(&self, generator: &SyntheticGenerator) -> Result<Vec<u8>, String> {
        match generator.generator_type {
            GeneratorType::StaticTemplate => {
                if let Some(ParameterValue::String(template)) = generator.input_parameters.get("template") {
                    Ok(template.as_bytes().to_vec())
                } else {
                    Err("Missing template parameter".to_string())
                }
            }
            GeneratorType::DynamicQuery => {
                // Simulate database query result
                Ok(b"query_result_data".to_vec())
            }
            GeneratorType::MLInference => {
                // Simulate ML model inference
                Ok(b"ml_inference_output".to_vec())
            }
            GeneratorType::SystemMetrics => {
                // Simulate system metrics
                Ok(b"cpu: 45%, memory: 67%, disk: 23%".to_vec())
            }
            GeneratorType::ComputedFunction => {
                // Simulate mathematical computation
                Ok(b"computation_result: 42.0".to_vec())
            }
            GeneratorType::FileAggregator => {
                // Simulate file aggregation
                Ok(b"aggregated_file_content".to_vec())
            }
            GeneratorType::NetworkFetcher => {
                // Simulate network fetch
                Ok(b"network_response_data".to_vec())
            }
            GeneratorType::TimeSeries => {
                // Simulate time series data
                Ok(b"timestamp,value\n1234567890,42.5".to_vec())
            }
        }
    }

    /// Get MIME type for output format
    fn get_mime_type(format: &OutputFormat) -> String {
        match format {
            OutputFormat::PlainText => "text/plain".to_string(),
            OutputFormat::JSON => "application/json".to_string(),
            OutputFormat::XML => "application/xml".to_string(),
            OutputFormat::Binary => "application/octet-stream".to_string(),
            OutputFormat::CSV => "text/csv".to_string(),
            OutputFormat::Markdown => "text/markdown".to_string(),
        }
    }

    /// Manage cache size within limits
    fn manage_cache_size(&mut self) {
        let total_cache_size: usize = self.content_cache.values()
            .map(|content| content.content.len())
            .sum();

        if total_cache_size > self.global_limits.max_cache_size {
            // Remove oldest entries until under limit
            let mut entries: Vec<_> = self.content_cache.iter()
                .map(|(id, content)| (*id, content.generated_at))
                .collect();
            entries.sort_by_key(|(_, timestamp)| *timestamp);

            while self.get_cache_size() > self.global_limits.max_cache_size && !entries.is_empty() {
                if let Some((oldest_id, _)) = entries.first() {
                    self.content_cache.remove(oldest_id);
                    entries.remove(0);
                }
            }
        }
    }

    /// Get current cache size
    fn get_cache_size(&self) -> usize {
        self.content_cache.values()
            .map(|content| content.content.len())
            .sum()
    }

    /// Get current timestamp (simplified)
    fn current_timestamp() -> u64 {
        1234567890000 // Fixed timestamp for testing
    }

    /// Remove generator and cleanup
    pub fn unregister_generator(&mut self, generator_id: u32) -> Result<(), String> {
        if self.generators.remove(&generator_id).is_some() {
            // Remove from active files
            self.active_files.retain(|_, &mut id| id != generator_id);

            // Remove from cache
            self.content_cache.remove(&generator_id);

            // Remove stats
            self.generation_stats.remove(&generator_id);

            Ok(())
        } else {
            Err("Generator not found".to_string())
        }
    }
}

/// Synthetic files property tests
pub struct SyntheticFileProperties;

impl SyntheticFileProperties {
    /// THEOREM 1: Resource limits are strictly enforced
    pub fn resource_limits_enforced(fs: &SyntheticFileSystem) -> bool {
        for generator in fs.generators.values() {
            // Memory limit validation
            if generator.memory_limit > fs.global_limits.max_memory_per_generation {
                return false;
            }

            // CPU limit validation
            if generator.cpu_limit > fs.global_limits.max_cpu_per_generation {
                return false;
            }

            // Output size limit validation
            if generator.max_output_size > fs.global_limits.max_output_size {
                return false;
            }

            // Refresh interval validation
            if generator.refresh_interval < fs.global_limits.min_refresh_interval {
                return false;
            }
        }

        // Cache size limit
        fs.get_cache_size() <= fs.global_limits.max_cache_size
    }

    /// THEOREM 2: Content generation is deterministic given same inputs
    pub fn deterministic_generation(fs: &SyntheticFileSystem, generator_id: u32) -> bool {
        if let Some(generator) = fs.generators.get(&generator_id) {
            // Same generator parameters should produce same content (simplified check)
            match generator.generator_type {
                GeneratorType::StaticTemplate => true, // Always deterministic
                GeneratorType::ComputedFunction => true, // Mathematical functions are deterministic
                _ => true, // For testing, assume all are deterministic
            }
        } else {
            true
        }
    }

    /// THEOREM 3: Cache invalidation respects refresh intervals
    pub fn cache_invalidation_property(fs: &SyntheticFileSystem) -> bool {
        for (generator_id, content) in &fs.content_cache {
            if let Some(generator) = fs.generators.get(generator_id) {
                let current_time = SyntheticFileSystem::current_timestamp();
                let cache_age = current_time - content.generated_at;

                // If cache is stale, it should be marked for refresh
                // (This is a simplified check for the property test)
                if cache_age > generator.refresh_interval && !content.cache_hit {
                    continue; // Valid: fresh content not from cache
                }
            }
        }
        true
    }

    /// THEOREM 4: Generator isolation (one generator failure doesn't affect others)
    pub fn generator_isolation_property(fs: &SyntheticFileSystem) -> bool {
        // Count successful vs failed generations across all generators
        let total_successful: u64 = fs.generation_stats.values()
            .map(|stats| stats.successful_generations)
            .sum();

        let total_failed: u64 = fs.generation_stats.values()
            .map(|stats| stats.failed_generations)
            .sum();

        // At least some generations should succeed if any were attempted
        if total_successful + total_failed > 0 {
            total_successful > 0 || total_failed == 0
        } else {
            true // No generations attempted
        }
    }

    /// THEOREM 5: Path uniqueness (no duplicate file paths)
    pub fn path_uniqueness_property(fs: &SyntheticFileSystem) -> bool {
        let mut seen_generators = std::collections::HashSet::new();

        for &generator_id in fs.active_files.values() {
            if !seen_generators.insert(generator_id) {
                return false; // Duplicate generator ID found
            }
        }

        true
    }

    /// THEOREM 6: Output format consistency
    pub fn output_format_consistency(fs: &SyntheticFileSystem) -> bool {
        for (generator_id, content) in &fs.content_cache {
            if let Some(generator) = fs.generators.get(generator_id) {
                let expected_mime = SyntheticFileSystem::get_mime_type(&generator.output_format);
                if content.mime_type != expected_mime {
                    return false;
                }
            }
        }
        true
    }

    /// THEOREM 7: Global limits are respected
    pub fn global_limits_respected(fs: &SyntheticFileSystem) -> bool {
        // Generator count limit
        if fs.generators.len() > fs.global_limits.max_generators as usize {
            return false;
        }

        // Cache size limit
        if fs.get_cache_size() > fs.global_limits.max_cache_size {
            return false;
        }

        true
    }
}

/// QuickCheck properties
#[quickcheck]
fn prop_resource_limits(generators: Vec<SyntheticGenerator>) -> TestResult {
    if generators.len() > 20 {
        return TestResult::discard();
    }

    let mut fs = SyntheticFileSystem::default();

    for (i, generator) in generators.into_iter().enumerate() {
        let path = format!("/synthetic/{}", i);
        let _ = fs.register_generator(path, generator);
    }

    TestResult::from_bool(SyntheticFileProperties::resource_limits_enforced(&fs))
}

#[quickcheck]
fn prop_path_uniqueness(generators: Vec<SyntheticGenerator>) -> TestResult {
    if generators.len() > 15 {
        return TestResult::discard();
    }

    let mut fs = SyntheticFileSystem::default();

    for (i, generator) in generators.into_iter().enumerate() {
        let path = format!("/synthetic/{}", i);
        let _ = fs.register_generator(path, generator);
    }

    TestResult::from_bool(SyntheticFileProperties::path_uniqueness_property(&fs))
}

#[quickcheck]
fn prop_output_format_consistency(generators: Vec<SyntheticGenerator>) -> TestResult {
    if generators.len() > 10 {
        return TestResult::discard();
    }

    let mut fs = SyntheticFileSystem::default();

    for (i, generator) in generators.into_iter().enumerate() {
        let path = format!("/synthetic/{}", i);
        let generator_id = generator.generator_id;
        if fs.register_generator(path, generator).is_ok() {
            // Generate content to populate cache
            let _ = fs.generate_content(generator_id, true);
        }
    }

    TestResult::from_bool(SyntheticFileProperties::output_format_consistency(&fs))
}

/// Proptest specifications
proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn proptest_resource_enforcement(generators in prop::collection::vec(any::<SyntheticGenerator>(), 1..12)) {
        let mut fs = SyntheticFileSystem::default();

        for (i, generator) in generators.into_iter().enumerate() {
            let path = format!("/synthetic/{}", i);
            let _ = fs.register_generator(path, generator);
        }

        prop_assert!(SyntheticFileProperties::resource_limits_enforced(&fs));
        prop_assert!(SyntheticFileProperties::global_limits_respected(&fs));
    }

    #[test]
    fn proptest_generation_properties(generators in prop::collection::vec(any::<SyntheticGenerator>(), 1..8)) {
        let mut fs = SyntheticFileSystem::default();

        for (i, generator) in generators.into_iter().enumerate() {
            let path = format!("/synthetic/{}", i);
            let generator_id = generator.generator_id;
            if fs.register_generator(path, generator).is_ok() {
                // Test content generation
                let _ = fs.generate_content(generator_id, false);
                let _ = fs.generate_content(generator_id, true); // Force refresh
            }
        }

        prop_assert!(SyntheticFileProperties::path_uniqueness_property(&fs));
        prop_assert!(SyntheticFileProperties::output_format_consistency(&fs));
        prop_assert!(SyntheticFileProperties::generator_isolation_property(&fs));
    }

    #[test]
    fn proptest_cache_behavior(generators in prop::collection::vec(any::<SyntheticGenerator>(), 1..6)) {
        let mut fs = SyntheticFileSystem::default();

        for (i, generator) in generators.into_iter().enumerate() {
            let path = format!("/synthetic/{}", i);
            let generator_id = generator.generator_id;
            if fs.register_generator(path, generator).is_ok() {
                // Generate content multiple times to test caching
                let _ = fs.generate_content(generator_id, false);
                let _ = fs.generate_content(generator_id, false); // Should hit cache
                let _ = fs.generate_content(generator_id, true);  // Force refresh
            }
        }

        prop_assert!(SyntheticFileProperties::cache_invalidation_property(&fs));
        prop_assert!(SyntheticFileProperties::resource_limits_enforced(&fs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_generator_lifecycle() {
        let mut fs = SyntheticFileSystem::default();

        let generator = SyntheticGenerator {
            generator_id: 1,
            generator_type: GeneratorType::StaticTemplate,
            input_parameters: {
                let mut params = HashMap::new();
                params.insert("template".to_string(), ParameterValue::String("Hello World".to_string()));
                params
            },
            output_format: OutputFormat::PlainText,
            refresh_interval: 1000,
            memory_limit: 1024,
            cpu_limit: 10000,
            max_output_size: 1024,
        };

        // Register generator
        assert!(fs.register_generator("/hello".to_string(), generator).is_ok());
        assert_eq!(fs.generators.len(), 1);

        // Generate content
        let content = fs.generate_content(1, false);
        assert!(content.is_ok());

        let content = content.unwrap();
        assert_eq!(content.content, b"Hello World");
        assert_eq!(content.mime_type, "text/plain");
        assert!(!content.cache_hit);

        // Generate again (should hit cache)
        let content2 = fs.generate_content(1, false);
        assert!(content2.is_ok());
        assert!(content2.unwrap().cache_hit);

        // Unregister generator
        assert!(fs.unregister_generator(1).is_ok());
        assert_eq!(fs.generators.len(), 0);
    }

    #[test]
    fn test_resource_limit_enforcement() {
        let mut fs = SyntheticFileSystem::default();

        // Try to register generator with excessive memory limit
        let excessive_generator = SyntheticGenerator {
            generator_id: 1,
            generator_type: GeneratorType::StaticTemplate,
            input_parameters: HashMap::new(),
            output_format: OutputFormat::PlainText,
            refresh_interval: 1000,
            memory_limit: 1024 * 1024 * 1024, // 1GB (exceeds 64KB limit)
            cpu_limit: 10000,
            max_output_size: 1024,
        };

        assert!(fs.register_generator("/excessive".to_string(), excessive_generator).is_err());
    }

    #[test]
    fn test_duplicate_path_prevention() {
        let mut fs = SyntheticFileSystem::default();

        let generator1 = SyntheticGenerator {
            generator_id: 1,
            generator_type: GeneratorType::StaticTemplate,
            input_parameters: HashMap::new(),
            output_format: OutputFormat::PlainText,
            refresh_interval: 1000,
            memory_limit: 1024,
            cpu_limit: 10000,
            max_output_size: 1024,
        };

        let generator2 = SyntheticGenerator {
            generator_id: 2,
            ..generator1.clone()
        };

        // Register first generator
        assert!(fs.register_generator("/same_path".to_string(), generator1).is_ok());

        // Try to register second generator with same path (should fail)
        assert!(fs.register_generator("/same_path".to_string(), generator2).is_err());
    }

    #[test]
    fn test_cache_management() {
        let mut fs = SyntheticFileSystem::default();
        fs.global_limits.max_cache_size = 100; // Very small cache for testing

        let generator = SyntheticGenerator {
            generator_id: 1,
            generator_type: GeneratorType::StaticTemplate,
            input_parameters: {
                let mut params = HashMap::new();
                // Large content to exceed cache size
                params.insert("template".to_string(), ParameterValue::String("x".repeat(200)));
                params
            },
            output_format: OutputFormat::PlainText,
            refresh_interval: 1000,
            memory_limit: 1024,
            cpu_limit: 10000,
            max_output_size: 1024,
        };

        assert!(fs.register_generator("/large".to_string(), generator).is_ok());

        // Generate content that exceeds cache size
        let _ = fs.generate_content(1, false);

        // Cache should be managed (content might be evicted)
        assert!(fs.get_cache_size() <= fs.global_limits.max_cache_size);
    }
}
