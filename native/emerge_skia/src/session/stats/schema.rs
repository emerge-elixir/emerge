/// Bump whenever the public `EmergeSkia.stats/2` payload shape changes.
const STATS_SCHEMA_VERSION: u64 = 25;

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsConfigureNif {
    enabled: bool,
}

#[derive(Clone, Copy, Debug, rustler::NifTaggedEnum)]
enum StatsCommandNif {
    Peek,
    Take,
    Reset,
    Configure(StatsConfigureNif),
}

#[derive(Clone, Debug, Default, rustler::NifMap)]
struct StatsSnapshotNif {
    version: u64,
    kind: String,
    enabled: bool,
    rendering_api: Option<RenderingApi>,
    window: StatsWindowNif,
    frames: StatsFrameSnapshotNif,
    timings: StatsTimingSnapshotNif,
    drm: StatsDrmSnapshotNif,
    counters: StatsCounterSnapshotNif,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsWindowNif {
    elapsed_ms: u64,
    reset_on_read: bool,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsFrameSnapshotNif {
    fps: f64,
    display_fps: f64,
    display_frame_ms: f64,
    frame_count: u64,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsTimingSnapshotNif {
    render: DurationStatsNif,
    render_draw: DurationStatsNif,
    render_flush: DurationStatsNif,
    render_gpu_flush: DurationStatsNif,
    render_submit: DurationStatsNif,
    present_submit: DurationStatsNif,
    pipeline: DurationStatsNif,
    pipeline_submit_to_tree_start: DurationStatsNif,
    pipeline_tree: DurationStatsNif,
    pipeline_render_queue: DurationStatsNif,
    pipeline_submit_to_swap: DurationStatsNif,
    pipeline_swap_to_frame_callback: DurationStatsNif,
    layout: DurationStatsNif,
    refresh: DurationStatsNif,
    event_resolve: DurationStatsNif,
    patch_tree_process: DurationStatsNif,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct DurationStatsNif {
    count: u64,
    avg_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsDrmSnapshotNif {
    forced_gpu_finish_before_swap: DurationStatsNif,
    forced_gpu_finish_after_swap: DurationStatsNif,
    gpu_render_elapsed: DurationStatsNif,
    gpu_render_elapsed_disjoint_discarded_samples: u64,
    gpu_render_elapsed_pool_saturated_sample_skips: u64,
    gpu_render_elapsed_stale_epoch_samples: u64,
    video_retired_gl_finish_fallbacks: u64,
    egl_swap_buffers: DurationStatsNif,
    gbm_lock_front_buffer: DurationStatsNif,
    framebuffer_lookup: DurationStatsNif,
    prepared_to_commit: DurationStatsNif,
    previous_flip_to_commit: DurationStatsNif,
    atomic_commit_ioctl: DurationStatsNif,
    commit_to_kernel_page_flip: DurationStatsNif,
    kernel_page_flip_interval: DurationStatsNif,
    page_flip_dispatch_delay: DurationStatsNif,
    commit_to_event_processed: DurationStatsNif,
    page_flip_events: u64,
    page_flip_sequence_steps: u64,
    missed_vblanks: u64,
}

#[derive(Clone, Debug, Default, rustler::NifMap)]
struct StatsCounterSnapshotNif {
    pipeline: StatsPipelineSnapshotNif,
    video: StatsVideoSnapshotNif,
    vulkan_video: StatsVulkanVideoSnapshotNif,
    layout_cache: LayoutCacheStatsNif,
    renderer_cache: RendererCacheStatsNif,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsPipelineSnapshotNif {
    scenes_constructed: u64,
    render_queue_overwrites: u64,
    drm_scenes_selected_for_draw: u64,
    drm_scenes_presented: u64,
}

#[derive(Clone, Debug, Default, rustler::NifMap)]
struct StatsVideoSnapshotNif {
    submitted: u64,
    inactive_dropped: u64,
    pending_replaced: u64,
    pending_taken: u64,
    imported: u64,
    leases_released: u64,
    retired_fences_created: u64,
    retired_fences_released: u64,
    retired_gl_finish_fallbacks: u64,
    acquire_fences_received: u64,
    acquire_server_waits_queued: u64,
    acquire_client_wait_fallbacks: u64,
    acquire_wait_timeouts: u64,
    acquire_wait_errors: u64,
    primary_prepared: u64,
    video_primary_prepared: u64,
    stale_prepared: u64,
    stale_video_prepared: u64,
    gbm_no_free: u64,
    primary_commit_attempts: u64,
    primary_commit_ebusy: u64,
    primary_committed: u64,
    primary_presented: u64,
    video_primary_presented: u64,
    video_primary_ever_presented: bool,
    last_presented_streams: Vec<PresentedVideoStreamNif>,
    page_flip_events: u64,
    page_flip_sequence_steps: u64,
    missed_vblanks: u64,
    current_pending: u64,
    current_direct_imports: u64,
    current_retired_imports: u64,
    max_retired_imports: u64,
    current_prepared: u64,
    current_in_flight: u64,
}

#[derive(Clone, Debug, Default, rustler::NifMap)]
struct PresentedVideoStreamNif {
    renderer_epoch: u64,
    target_id: String,
    target_incarnation: u64,
    stream_id: u64,
}


#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct StatsVulkanVideoSnapshotNif {
    acquire_sync_fd_imported: u64,
    temporary_semaphore_import_failures: u64,
    ownership_acquires_submitted: u64,
    acquire_submit_failures: u64,
    ganesh_waits_rejected: u64,
    releases_submitted: u64,
    release_submit_failures: u64,
    releases_completed: u64,
    release_fences_created: u64,
    release_fence_errors: u64,
    release_fence_completions: u64,
    retirement_timeouts: u64,
    import_cap_saturations: u64,
    quarantined: u64,
    global_quarantine_terminal: bool,
    device_lost: u64,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct LayoutCacheStatsNif {
    intrinsic_measure_hits: u64,
    intrinsic_measure_misses: u64,
    intrinsic_measure_stores: u64,
    subtree_measure_hits: u64,
    subtree_measure_misses: u64,
    subtree_measure_stores: u64,
    resolve_hits: u64,
    resolve_misses: u64,
    resolve_stores: u64,
}

#[derive(Clone, Debug, Default, rustler::NifMap)]
struct RendererCacheStatsNif {
    enabled: bool,
    disabled_reason: Option<String>,
    paint_layer: RendererCachePaintLayerStatsNif,
}

#[derive(Clone, Copy, Debug, Default, rustler::NifMap)]
struct RendererCachePaintLayerStatsNif {
    candidates: u64,
    visible_candidates: u64,
    suppressed_by_parent: u64,
    admitted: u64,
    hits: u64,
    misses: u64,
    stores: u64,
    evictions: u64,
    stale_evictions: u64,
    rejected: u64,
    current_entries: u64,
    current_bytes: u64,
    current_gpu_payloads: u64,
    current_cpu_payloads: u64,
    evicted_bytes: u64,
    stale_evicted_bytes: u64,
    gpu_payload_stores: u64,
    cpu_payload_stores: u64,
    prepare_successes: u64,
    prepare_failures: u64,
    direct_fallbacks_after_admission: u64,
    rejected_ineligible: u64,
    rejected_admission: u64,
    rejected_oversized: u64,
    rejected_payload_budget: u64,
    rejected_fractional_placement: u64,
    rejected_unsupported_transform: u64,
    prepare: DurationStatsNif,
    draw_hit: DurationStatsNif,
}

