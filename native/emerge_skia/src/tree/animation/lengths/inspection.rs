//! Opt-in test observations of the real resolver, never a second sampler.
use super::*;
use crate::tree::layout::projection::ProjectionStats;

#[derive(Clone, Debug, Default)]
pub(crate) struct Inspection {
    pub fail_query: Option<QueryKind>,
    pub cached_targets: HashMap<(NodeId, Axis), AxisFootprint>,
    pub frozen: Option<Arc<Projection>>,
    pub context: Option<Arc<QueryContext>>,
    pub queries: Vec<QueryObservation>,
    pub candidate: Option<Projection>,
    pub releasing: HashSet<(NodeId, Axis)>,
    pub before: ProjectionStats,
    pub after: ProjectionStats,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryKind {
    ContentTarget,
    Source,
    Target,
    Release,
    PreviousViewport,
    PreviousModel,
    PreviousInputs,
    PriorTarget,
    IndependentContext,
    CurrentRelease,
    PixelContinuation,
    ForeignTarget,
    HistoricalTarget,
    Transport,
}
#[derive(Clone, Debug)]
pub(crate) struct QueryObservation {
    pub kind: QueryKind,
    pub projection: Arc<Projection>,
    pub result: Result<HashMap<(NodeId, Axis), AxisFootprint>, ProjectionError>,
}

/// Failure injection is opt-in and only changes the test observation's query result.
/// Private evaluator work is still charged; no live state is installed on this error.
pub(super) fn observe_query(
    tree: &ElementTree,
    kind: impl FnOnce() -> QueryKind,
    projection: &Arc<Projection>,
    result: Result<HashMap<(NodeId, Axis), AxisFootprint>, ProjectionError>,
) -> Result<HashMap<(NodeId, Axis), AxisFootprint>, ProjectionError> {
    let Some(cell) = tree.animation_inspection.as_ref() else {
        return result;
    };
    let mut trace = cell.borrow_mut();
    let kind = kind();
    let result = if trace.fail_query == Some(kind) && result.is_ok() {
        Err(ProjectionError::InjectedQuery)
    } else {
        result
    };
    trace.queries.push(QueryObservation {
        kind,
        projection: Arc::clone(projection),
        result: result.clone(),
    });
    result
}
