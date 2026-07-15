//! Tracing spans. One span per stage per unit of work, with the in-flight ids attached, so a log
//! line always says which device, session, stream, and frame it was about. Installing a
//! subscriber is the host's job (the apps and mav-replay pick their own sinks and formats); the
//! core only creates spans, and with no subscriber installed they cost almost nothing.

use crate::stage::Stage;
use crate::tap::Ids;
use tracing::Span;

/// Create the span a stage runs inside. Callers enter it for the duration of one unit of work.
pub fn stage_span(stage: Stage, ids: &Ids) -> Span {
    tracing::info_span!(
        "stage",
        stage = stage.name(),
        device = ids.device.map(|id| id.get()),
        session = ids.session.map(|id| id.get()),
        stream = ids.stream.map(|id| id.get()),
        frame = ids.frame.map(|id| id.get()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::span;

    // A minimal hand-rolled subscriber, so the test can assert span creation without pulling
    // tracing-subscriber in as a dependency of the crate.
    struct RecordingSubscriber {
        names: Arc<Mutex<Vec<String>>>,
    }

    impl tracing::Subscriber for RecordingSubscriber {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, span: &span::Attributes<'_>) -> span::Id {
            self.names
                .lock()
                .unwrap()
                .push(span.metadata().name().to_owned());
            span::Id::from_u64(1)
        }

        fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
        fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
        fn event(&self, _event: &tracing::Event<'_>) {}
        fn enter(&self, _span: &span::Id) {}
        fn exit(&self, _span: &span::Id) {}
    }

    #[test]
    fn stage_span_is_created_with_the_stage_name() {
        let names = Arc::new(Mutex::new(Vec::new()));
        let subscriber = RecordingSubscriber {
            names: names.clone(),
        };
        tracing::subscriber::with_default(subscriber, || {
            let span = stage_span(Stage::Decode, &Ids::default());
            let _guard = span.enter();
        });
        assert_eq!(*names.lock().unwrap(), vec!["stage".to_owned()]);
    }
}
