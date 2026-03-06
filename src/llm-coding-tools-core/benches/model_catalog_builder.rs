//! Benchmarks for batch model-catalog construction.

use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_coding_tools_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderInfo, ProviderModelSource, ProviderSource,
    ProviderType,
};

struct ProviderModelSpec {
    provider_idx: usize,
    model_key: String,
    model: ModelInfo,
}

struct Dataset {
    providers: Vec<ProviderSource>,
    provider_models: Vec<ProviderModelSpec>,
}

impl Dataset {
    fn provider_model_sources(&self) -> Vec<ProviderModelSource<'_>> {
        let mut sources = Vec::with_capacity(self.provider_models.len());
        for provider_model in &self.provider_models {
            let provider_key = self.providers[provider_model.provider_idx]
                .provider_key
                .as_str();
            sources.push(ProviderModelSource::new(
                provider_key,
                provider_model.model_key.as_str(),
                provider_model.model,
            ));
        }
        sources
    }
}

fn make_dataset(provider_count: usize, model_count: usize) -> Dataset {
    debug_assert!(provider_count > 0);

    let mut providers = Vec::with_capacity(provider_count);
    for i in 0..provider_count {
        providers.push(ProviderSource::new(
            format!("provider-{i}"),
            ProviderInfo {
                api_url: format!("https://provider-{i}.example/v1"),
                env_vars: vec![format!("PROVIDER_{i}_API_KEY")],
                api_type: if (i & 1) == 0 {
                    ProviderType::OpenAiCompletions
                } else {
                    ProviderType::Azure
                },
            },
        ));
    }

    let mut provider_models = Vec::with_capacity(model_count);
    let unique_cfg_count = (model_count / 5).max(1);
    for i in 0..model_count {
        let provider_idx = i % provider_count;
        let cfg = i % unique_cfg_count;
        let temperature = if (cfg & 1) == 0 {
            Some(1.0 + ((cfg % 5000) as f32 * 0.001))
        } else {
            None
        };
        let top_p = if cfg.is_multiple_of(3) {
            Some(0.9)
        } else {
            None
        };

        provider_models.push(ProviderModelSpec {
            provider_idx,
            model_key: format!("org-{}/model-{i}", i % 17),
            model: ModelInfo {
                modalities: Modality::TEXT,
                max_input: 4096 + ((cfg as u32) * 32),
                max_output: 512 + ((cfg as u32) * 8),
                temperature,
                top_p,
            },
        });
    }

    Dataset {
        providers,
        provider_models,
    }
}

fn construct_batch(providers: &[ProviderSource], provider_models: &[ProviderModelSource<'_>]) {
    let catalog = ModelCatalog::build(providers, provider_models).expect("batch build");

    black_box((
        catalog.provider_count(),
        catalog.provider_model_count(),
        catalog.model_config_count(),
    ));
}

fn benchmark_builder_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("model_catalog_builder_construct");

    for (name, provider_count, model_count) in [
        ("models_dev_snapshot", 96usize, 3031usize),
        ("max", 16384usize, 65535usize),
    ] {
        let dataset = make_dataset(provider_count, model_count);
        let provider_model_sources = dataset.provider_model_sources();
        group.throughput(Throughput::Elements(
            (provider_count + dataset.provider_models.len()) as u64,
        ));

        group.bench_with_input(BenchmarkId::new("batch", name), &dataset, |b, input| {
            b.iter(|| {
                construct_batch(
                    black_box(&input.providers),
                    black_box(&provider_model_sources),
                )
            })
        });
    }

    group.finish();
}

criterion_group!(benches, benchmark_builder_construction);
criterion_main!(benches);
