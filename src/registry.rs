//! The process-global metrics registry.

use crate::egui::{text::LayoutJob, Color32, TextFormat};
use crate::{metric_kind_str, unit_str};
use bevy::{
    platform::collections::HashMap,
    prelude::{default, Res, Resource},
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use metrics::{Counter, Gauge, Histogram, KeyName, Metadata, Recorder, SharedString, Unit};
use metrics_util::{
    registry::{AtomicStorage, Registry},
    storage::AtomicBucket,
    MetricKind,
};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

/// Tracks all metrics in the current process.
///
/// You may never need to interact with this, unless you want to call
/// [`set_global_recorder`](metrics::set_global_recorder) manually and provide a
/// clone of that same registry to the [`RegistryPlugin`](crate::RegistryPlugin).
#[derive(Clone, Resource)]
pub struct MetricsRegistry {
    inner: Arc<Inner>,
}

struct Inner {
    registry: Registry<metrics::Key, AtomicStorage>,
    descriptions: RwLock<HashMap<DescriptionKey, MetricDescription>>,
}

/// A description of some metric, displayed when searching the registry or plotting.
#[allow(missing_docs)]
#[derive(Clone)]
pub struct MetricDescription {
    pub unit: Option<Unit>,
    pub text: SharedString,
}

impl Inner {
    fn new() -> Self {
        Self {
            registry: Registry::atomic(),
            descriptions: RwLock::new(Default::default()),
        }
    }
}

impl MetricsRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner::new()),
        }
    }

    #[allow(missing_docs)]
    pub fn get_or_create_counter(&self, key: &metrics::Key) -> Arc<AtomicU64> {
        self.inner.registry.get_or_create_counter(key, Arc::clone)
    }
    #[allow(missing_docs)]
    pub fn get_or_create_gauge(&self, key: &metrics::Key) -> Arc<AtomicU64> {
        self.inner.registry.get_or_create_gauge(key, Arc::clone)
    }
    #[allow(missing_docs)]
    pub fn get_or_create_histogram(&self, key: &metrics::Key) -> Arc<AtomicBucket<f64>> {
        self.inner.registry.get_or_create_histogram(key, Arc::clone)
    }
    #[allow(missing_docs)]
    pub fn get_description(&self, key: &DescriptionKey) -> Option<MetricDescription> {
        self.inner.descriptions.read().unwrap().get(key).cloned()
    }

    /// Search the registry for metrics whose name matches `input`.
    ///
    /// Empty `input` will match everything.
    ///
    /// Results are not returned in any particular order.
    pub fn fuzzy_search_by_name(&self, input: &str) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let matcher = SkimMatcherV2::default();
        let reg = &self.inner.registry;
        let descriptions = self.inner.descriptions.read().unwrap();
        reg.visit_counters(|key, _| {
            if matcher.fuzzy_match(key.name(), input).is_some() {
                results.push(make_search_result(MetricKind::Counter, key, &descriptions));
            }
        });
        reg.visit_gauges(|key, _| {
            if matcher.fuzzy_match(key.name(), input).is_some() {
                results.push(make_search_result(MetricKind::Gauge, key, &descriptions));
            }
        });
        reg.visit_histograms(|key, _| {
            if matcher.fuzzy_match(key.name(), input).is_some() {
                results.push(make_search_result(
                    MetricKind::Histogram,
                    key,
                    &descriptions,
                ));
            }
        });
        results
    }

    /// Get a search result for every registered metric.
    pub fn all_metrics(&self) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let reg = &self.inner.registry;
        let descriptions = self.inner.descriptions.read().unwrap();
        reg.visit_counters(|key, _| {
            results.push(make_search_result(MetricKind::Counter, key, &descriptions));
        });
        reg.visit_gauges(|key, _| {
            results.push(make_search_result(MetricKind::Gauge, key, &descriptions));
        });
        reg.visit_histograms(|key, _| {
            results.push(make_search_result(
                MetricKind::Histogram,
                key,
                &descriptions,
            ));
        });
        results
    }

    fn add_description_if_missing(&self, key: DescriptionKey, description: MetricDescription) {
        let mut descriptions = self.inner.descriptions.write().unwrap();
        descriptions.entry(key).or_insert(description);
    }

    /// Clear all atomic buckets used for storing histogram data.
    pub fn clear_atomic_buckets(&self) {
        self.inner.registry.visit_histograms(|_, h| {
            h.clear();
        });
    }

    pub(crate) fn clear_atomic_buckets_system(registry: Res<Self>) {
        registry.clear_atomic_buckets();
    }
}

fn make_search_result(
    kind: MetricKind,
    key: &metrics::Key,
    descriptions: &HashMap<DescriptionKey, MetricDescription>,
) -> SearchResult {
    let key = MetricKey::new(key.clone(), kind);
    let desc_key = DescriptionKey::from(&key);
    let description = descriptions.get(&desc_key).cloned();
    SearchResult { key, description }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Identifies some metric in the registry.
#[allow(missing_docs)]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MetricKey {
    pub key: metrics::Key,
    pub kind: MetricKind,
}

impl MetricKey {
    #[allow(missing_docs)]
    pub fn new(key: metrics::Key, kind: MetricKind) -> Self {
        Self { key, kind }
    }

    /// The text used when displaying search results and assigning a title to a plot.
    pub fn title(&self, display_path: Option<&str>, n_duplicates: usize) -> String {
        let name = if let Some(path) = display_path {
            path
        } else {
            self.key.name()
        };
        if n_duplicates > 0 {
            format!("{} ({}) {n_duplicates}", name, metric_kind_str(self.kind))
        } else {
            format!("{} ({})", name, metric_kind_str(self.kind))
        }
    }
}

/// Key used for storing metric descriptions.
#[allow(missing_docs)]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DescriptionKey {
    pub name: KeyName,
    pub kind: MetricKind,
}

impl From<&MetricKey> for DescriptionKey {
    fn from(value: &MetricKey) -> Self {
        Self {
            name: KeyName::from(value.key.name().to_owned()),
            kind: value.kind,
        }
    }
}

/// Metadata for a metric.
#[allow(missing_docs)]
#[derive(Clone)]
pub struct SearchResult {
    pub key: MetricKey,
    pub description: Option<MetricDescription>,
}

impl SearchResult {
    /// Display the complete information of a search result.
    ///
    /// `display_path` will override the key's name, which is used for removing
    /// layers of namespacing.
    pub fn detailed_text(&self, display_path: Option<&str>) -> LayoutJob {
        let mut job = LayoutJob::default();
        job.append(
            &self.key.title(display_path, 0),
            0.0,
            TextFormat {
                // underline: Stroke::new(1.0, Color32::WHITE),
                color: Color32::WHITE,
                ..default()
            },
        );
        if let Some(unit) = self.description.as_ref().and_then(|d| d.unit) {
            job.append(
                &format!(" [{}]", unit_str(unit)),
                0.0,
                TextFormat {
                    color: Color32::LIGHT_BLUE,
                    ..default()
                },
            );
        }
        for label in self.key.key.labels() {
            job.append("\n", 0.0, default());
            job.append(
                &format!("{}={}", label.key(), label.value()),
                0.0,
                TextFormat {
                    color: Color32::YELLOW,
                    ..default()
                },
            );
        }
        if let Some(description) = &self.description {
            job.append("\n", 0.0, default());
            job.append(
                &description.text,
                0.0,
                TextFormat {
                    color: Color32::GRAY,
                    italics: true,
                    ..default()
                },
            );
        }
        job
    }
}

impl Recorder for MetricsRegistry {
    fn describe_counter(&self, key_name: KeyName, unit: Option<Unit>, description: SharedString) {
        self.add_description_if_missing(
            DescriptionKey {
                name: key_name,
                kind: MetricKind::Counter,
            },
            MetricDescription {
                unit,
                text: description,
            },
        );
    }

    fn describe_gauge(&self, key_name: KeyName, unit: Option<Unit>, description: SharedString) {
        self.add_description_if_missing(
            DescriptionKey {
                name: key_name,
                kind: MetricKind::Gauge,
            },
            MetricDescription {
                unit,
                text: description,
            },
        );
    }

    fn describe_histogram(&self, key_name: KeyName, unit: Option<Unit>, description: SharedString) {
        self.add_description_if_missing(
            DescriptionKey {
                name: key_name,
                kind: MetricKind::Histogram,
            },
            MetricDescription {
                unit,
                text: description,
            },
        );
    }

    fn register_counter(&self, key: &metrics::Key, _metadata: &Metadata<'_>) -> Counter {
        self.inner
            .registry
            .get_or_create_counter(key, |c| c.clone().into())
    }

    fn register_gauge(&self, key: &metrics::Key, _metadata: &Metadata<'_>) -> Gauge {
        self.inner
            .registry
            .get_or_create_gauge(key, |c| c.clone().into())
    }

    fn register_histogram(&self, key: &metrics::Key, _metadata: &Metadata<'_>) -> Histogram {
        self.inner
            .registry
            .get_or_create_histogram(key, |c| c.clone().into())
    }
}
