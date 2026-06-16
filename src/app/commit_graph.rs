//! Commit-graph gutter rendering: lane geometry, connectors, rounded bends,
//! and the ref-label pills drawn beside each commit row. These are pure view
//! helpers driven by `graph::GraphRow` data, extracted from `app.rs` to keep
//! the root view module focused. See docs/adr/0002-project-layout.md.

use super::*;

pub(crate) fn commit_row_separator_width() -> f32 {
    0.
}

pub(crate) fn commit_row_separator_color(selected: bool) -> gpui::Rgba {
    if selected {
        rgb(0x3b82f6)
    } else {
        rgb(0x242424)
    }
}

pub(crate) fn render_commit_ref_labels(
    row_index: usize,
    commit: &repo::CommitInfo,
    hidden_branches: &BTreeSet<String>,
) -> gpui::Div {
    let labels = commit_ref_labels(commit, hidden_branches);

    div()
        .flex()
        .items_center()
        .gap_1()
        .w(px(COMMIT_REF_LABELS_WIDTH))
        .overflow_hidden()
        .flex_shrink_0()
        .debug_selector(move || format!("commit-ref-labels-{row_index}"))
        .children(
            labels
                .into_iter()
                .map(|label| render_commit_ref_label(row_index, label))
                .collect::<Vec<_>>(),
        )
}

pub(crate) const COMMIT_REF_LABELS_WIDTH: f32 = 156.;
pub(crate) const COMMIT_REF_LABEL_MAX_WIDTH: f32 = COMMIT_REF_LABELS_WIDTH - 8.;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommitRefLabel {
    pub(crate) name: String,
    /// Namespaced key used for the debug selector, so same-named local and
    /// remote labels stay distinguishable (`heads/main` vs `remotes/origin/main`).
    pub(crate) selector_key: String,
    pub(crate) kind: CommitRefLabelKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitRefLabelKind {
    Head,
    Branch,
    RemoteBranch,
}

/// The ref label pills a commit row renders: HEAD first when checked out,
/// then one pill per branch label whose namespaced key (see [`branch_key`])
/// the user has not hidden.
pub(crate) fn commit_ref_labels(
    commit: &repo::CommitInfo,
    hidden_branches: &BTreeSet<String>,
) -> Vec<CommitRefLabel> {
    let mut labels = Vec::new();
    if commit.is_head {
        labels.push(CommitRefLabel {
            name: "HEAD".to_string(),
            selector_key: "HEAD".to_string(),
            kind: CommitRefLabelKind::Head,
        });
    }
    labels.extend(
        commit
            .branch_labels
            .iter()
            .filter(|label| !hidden_branches.contains(&branch_key(&label.name, &label.kind)))
            .map(|label| CommitRefLabel {
                name: label.name.clone(),
                selector_key: branch_key(&label.name, &label.kind),
                kind: match label.kind {
                    repo::BranchKind::Local => CommitRefLabelKind::Branch,
                    repo::BranchKind::Remote { .. } => CommitRefLabelKind::RemoteBranch,
                },
            }),
    );
    labels
}

pub(crate) fn render_commit_ref_label(row_index: usize, label: CommitRefLabel) -> gpui::Div {
    let selector = format!(
        "commit-ref-label-{row_index}-{}",
        debug_ref_label_fragment(&label.selector_key)
    );
    let (border_color, background, text_color) = match label.kind {
        CommitRefLabelKind::Head => (rgb(0x0ea5e9), rgb(0x102536), rgb(0x7dd3fc)),
        CommitRefLabelKind::Branch => (rgb(0x3f6212), rgb(0x17230f), rgb(0xa3e635)),
        CommitRefLabelKind::RemoteBranch => (rgb(0x475569), rgb(0x1b2430), rgb(REMOTE_BRANCH_TINT)),
    };

    div()
        .px_1()
        .py_0p5()
        .border_1()
        .border_color(border_color)
        .bg(background)
        .text_color(text_color)
        .text_size(px(10.))
        .font_family("monospace")
        .max_w(px(COMMIT_REF_LABEL_MAX_WIDTH))
        .truncate()
        .debug_selector(move || selector.clone())
        .child(label.name)
}

/// The set of loaded commits reachable from HEAD or from any branch whose
/// key (see [`branch_key`]) is not in `hidden_branches`. Parents beyond the
/// loaded page simply terminate the walk: a commit that is not loaded cannot
/// be rendered anyway, and paging in more history re-runs this computation
/// over the larger list.
pub(crate) fn visible_commit_shas(
    commits: &[repo::CommitInfo],
    branches: &[repo::Branch],
    head_sha: Option<&str>,
    hidden_branches: &BTreeSet<String>,
) -> HashSet<String> {
    let commits_by_sha: HashMap<&str, &repo::CommitInfo> = commits
        .iter()
        .map(|commit| (commit.sha.as_str(), commit))
        .collect();

    let mut worklist: Vec<&str> = Vec::new();
    worklist.extend(head_sha);
    worklist.extend(
        branches
            .iter()
            .filter(|branch| !hidden_branches.contains(&branch_key(&branch.name, &branch.kind)))
            .map(|branch| branch.tip_sha.as_str()),
    );

    let mut visible = HashSet::new();
    while let Some(sha) = worklist.pop() {
        let Some(commit) = commits_by_sha.get(sha) else {
            continue;
        };
        if !visible.insert(commit.sha.clone()) {
            continue;
        }
        worklist.extend(commit.parent_shas.iter().map(String::as_str));
    }
    visible
}

/// The loaded commits that survive branch-visibility filtering, in history
/// order. Render and focus paths must both use this so row indices agree.
///
/// The empty-set fast-path is the identity over loaded commits. For real
/// repositories that equals the reachability walk, because
/// `repo::read_commit_page` seeds its revwalk from every local and remote
/// branch tip plus HEAD — every loaded commit is reachable from at least one
/// of those seeds. Synthetic test fixtures seed commits without `branches`
/// and rely on the fast-path to render at all; if the revwalk ever loads
/// unreachable commits, hide-then-show would no longer round-trip and this
/// fast-path must be revisited.
pub(crate) fn visible_commits<'a>(
    repo: &'a repo::OpenRepository,
    hidden_branches: &BTreeSet<String>,
) -> Vec<&'a repo::CommitInfo> {
    if hidden_branches.is_empty() {
        return repo.commits.iter().collect();
    }
    let head_sha = repo
        .commits
        .iter()
        .find(|commit| commit.is_head)
        .map(|commit| commit.sha.as_str());
    let visible = visible_commit_shas(&repo.commits, &repo.branches, head_sha, hidden_branches);
    repo.commits
        .iter()
        .filter(|commit| visible.contains(&commit.sha))
        .collect()
}

pub(crate) fn render_commit_graph_gutter(
    row_index: usize,
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    next_row: Option<&graph::GraphRow>,
    max_lanes: usize,
) -> impl IntoElement {
    let lane_count = max_lanes.max(1);
    let debug_selector = format!("commit-graph-gutter-{row_index}");

    // Lanes paint right to left so the edges in lower lanes draw above the
    // branches that join them.
    div()
        .relative()
        .w(px(commit_graph_gutter_width(lane_count)))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .font_family("monospace")
        .id(("commit-graph-gutter", row_index))
        .debug_selector(move || debug_selector.clone())
        .children(
            (0..lane_count)
                .rev()
                .map(|lane| {
                    div()
                        .absolute()
                        .left(px(lane as f32 * COMMIT_GRAPH_LANE_WIDTH))
                        .top_0()
                        .child(render_commit_graph_lane(
                            row_index,
                            lane,
                            row,
                            CommitGraphNeighborRows {
                                previous: previous_row,
                                next: next_row,
                            },
                        ))
                })
                .collect::<Vec<_>>(),
        )
}

pub(crate) fn render_commit_graph_history_overlay(
    rows: &[graph::GraphRow],
    max_lanes: usize,
) -> impl IntoElement {
    let lane_count = max_lanes.max(1);
    let height = rows.len() as f32 * COMMIT_ROW_HEIGHT;

    div()
        .absolute()
        .left(px(COMMIT_ROW_HORIZONTAL_PADDING))
        .top_0()
        .w(px(commit_graph_gutter_width(lane_count)))
        .h(px(height))
        .debug_selector(|| "commit-graph-overlay".to_string())
        .child(
            div().relative().w_full().h(px(height)).children(
                commit_graph_overlay_row_indices(rows.len())
                    .into_iter()
                    .map(|row_index| {
                        div()
                            .absolute()
                            .left_0()
                            .top(px(row_index as f32 * COMMIT_ROW_HEIGHT))
                            .child(render_commit_graph_gutter(
                                row_index,
                                &rows[row_index],
                                row_index
                                    .checked_sub(1)
                                    .and_then(|previous_row| rows.get(previous_row)),
                                rows.get(row_index + 1),
                                lane_count,
                            ))
                    })
                    .collect::<Vec<_>>(),
            ),
        )
}

pub(crate) fn commit_graph_overlay_row_indices(row_count: usize) -> Vec<usize> {
    (0..row_count).rev().collect()
}

pub(crate) fn render_commit_graph_gutter_spacer(max_lanes: usize) -> impl IntoElement {
    div()
        .w(px(commit_graph_gutter_width(max_lanes.max(1))))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .flex_shrink_0()
}

pub(crate) fn commit_graph_gutter_width(lane_count: usize) -> f32 {
    (lane_count as f32 * COMMIT_GRAPH_LANE_WIDTH).max(COMMIT_GRAPH_LANE_WIDTH * 2.)
}

pub(crate) const COMMIT_GRAPH_LANE_WIDTH: f32 = 22.;
pub(crate) const COMMIT_GRAPH_LANE_HEIGHT: f32 = COMMIT_ROW_HEIGHT;
pub(crate) const COMMIT_GRAPH_MIDDLE_HEIGHT: f32 = 10.;
pub(crate) const COMMIT_GRAPH_VERTICAL_HEIGHT: f32 =
    (COMMIT_GRAPH_LANE_HEIGHT - COMMIT_GRAPH_MIDDLE_HEIGHT) / 2.;
pub(crate) const COMMIT_GRAPH_LINE_WIDTH: f32 = 2.;
pub(crate) const COMMIT_GRAPH_DOT_SIZE: f32 = 8.;
pub(crate) const COMMIT_GRAPH_BEND_RADIUS: f32 = 8.;
pub(crate) const COMMIT_GRAPH_BEND_CUBIC_CONTROL: f32 = 0.552_284_8;

pub(crate) fn commit_graph_line_x() -> f32 {
    (COMMIT_GRAPH_LANE_WIDTH - COMMIT_GRAPH_LINE_WIDTH) / 2.
}

pub(crate) fn commit_graph_right_line_x() -> f32 {
    commit_graph_line_x() + COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_right_line_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH - commit_graph_right_line_x()
}

pub(crate) fn commit_graph_middle_line_y() -> f32 {
    (commit_graph_middle_height() - COMMIT_GRAPH_LINE_WIDTH) / 2.
}

pub(crate) fn commit_graph_middle_line_bottom_y() -> f32 {
    commit_graph_middle_line_y() + COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_bend_radius() -> f32 {
    COMMIT_GRAPH_BEND_RADIUS
}

pub(crate) fn commit_graph_middle_height() -> f32 {
    COMMIT_GRAPH_MIDDLE_HEIGHT
}

pub(crate) fn commit_graph_line_width() -> f32 {
    COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_bend_overlay_height() -> f32 {
    COMMIT_GRAPH_LANE_HEIGHT
}

pub(crate) fn commit_graph_bend_overlay_top() -> f32 {
    -COMMIT_GRAPH_VERTICAL_HEIGHT
}

pub(crate) fn commit_graph_bend_overlay_x() -> f32 {
    -COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_bend_overlay_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH + COMMIT_GRAPH_LINE_WIDTH * 2.
}

pub(crate) fn commit_graph_bend_overlay_lane_offset_x() -> f32 {
    COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_commit_bend_overlay_x() -> f32 {
    -COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_commit_bend_overlay_width() -> f32 {
    commit_graph_commit_bend_overlay_dot_center_x() + COMMIT_GRAPH_LINE_WIDTH
}

pub(crate) fn commit_graph_commit_bend_overlay_dot_center_x() -> f32 {
    -commit_graph_commit_bend_overlay_x()
        + commit_graph_dot_side_line_width()
        + COMMIT_GRAPH_DOT_SIZE / 2.
}

pub(crate) fn commit_graph_merge_target_commit_bend_overlay_x() -> f32 {
    0.
}

pub(crate) fn commit_graph_merge_target_commit_bend_overlay_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH
}

pub(crate) fn commit_graph_merge_target_commit_bend_overlay_dot_center_x() -> f32 {
    -commit_graph_merge_target_commit_bend_overlay_x()
        + commit_graph_dot_side_line_width()
        + COMMIT_GRAPH_DOT_SIZE / 2.
}

pub(crate) fn commit_graph_merge_in_commit_bend_end_y() -> f32 {
    -commit_graph_bend_overlay_top()
        + commit_graph_dot_bottom_gap_y()
        + commit_graph_line_width() / 2.
}

pub(crate) fn commit_graph_merge_in_commit_line_y() -> f32 {
    commit_graph_merge_in_commit_bend_end_y() + commit_graph_bend_radius()
}

pub(crate) fn commit_graph_merge_in_commit_line_y_in_middle() -> f32 {
    commit_graph_merge_in_commit_line_y() - COMMIT_GRAPH_VERTICAL_HEIGHT
}

pub(crate) fn commit_graph_dot_gap_height() -> f32 {
    (COMMIT_GRAPH_MIDDLE_HEIGHT - COMMIT_GRAPH_DOT_SIZE) / 2.
}

pub(crate) fn commit_graph_dot_bottom_gap_y() -> f32 {
    commit_graph_dot_gap_height() + COMMIT_GRAPH_DOT_SIZE
}

pub(crate) fn commit_graph_dot_side_line_width() -> f32 {
    (COMMIT_GRAPH_LANE_WIDTH - COMMIT_GRAPH_DOT_SIZE) / 2.
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommitGraphPoint {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommitGraphCubicBend {
    pub(crate) start: CommitGraphPoint,
    pub(crate) first_control: CommitGraphPoint,
    pub(crate) second_control: CommitGraphPoint,
    pub(crate) end: CommitGraphPoint,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommitGraphRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommitGraphBranchOffSourceBend {
    pub(crate) curve: CommitGraphCubicBend,
    pub(crate) horizontal_end: Option<CommitGraphPoint>,
}

pub(crate) fn commit_graph_lower_merge_in_horizontal_top_in_middle() -> f32 {
    commit_graph_merge_in_commit_line_y_in_middle() - commit_graph_line_width() / 2.
}

pub(crate) fn commit_graph_lower_connector_vertical_shift() -> f32 {
    COMMIT_GRAPH_LANE_HEIGHT
        - COMMIT_GRAPH_VERTICAL_HEIGHT
        - commit_graph_merge_in_commit_line_y_in_middle()
}

pub(crate) fn commit_graph_shifted_lower_merge_in_horizontal_top_in_middle() -> f32 {
    commit_graph_lower_merge_in_horizontal_top_in_middle()
        + commit_graph_lower_connector_vertical_shift()
}

pub(crate) fn commit_graph_shifted_bend_overlay_height() -> f32 {
    commit_graph_bend_overlay_height() + commit_graph_lower_connector_vertical_shift()
}

pub(crate) fn commit_graph_branch_off_source_bend_geometry(
    _spans_occupied_lanes: bool,
) -> CommitGraphBranchOffSourceBend {
    let lane_offset_x = commit_graph_bend_overlay_lane_offset_x();
    let center_x = lane_offset_x + commit_graph_line_x() + commit_graph_line_width() / 2.;
    let lower_line_y = commit_graph_merge_in_commit_line_y();
    let radius = commit_graph_bend_radius();
    let curve_end_x = center_x + radius;
    let horizontal_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let vertical_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let curve_end = CommitGraphPoint {
        x: curve_end_x,
        y: lower_line_y,
    };

    CommitGraphBranchOffSourceBend {
        curve: CommitGraphCubicBend {
            start: CommitGraphPoint {
                x: center_x,
                y: lower_line_y + radius,
            },
            first_control: CommitGraphPoint {
                x: center_x,
                y: lower_line_y + radius - vertical_control,
            },
            second_control: CommitGraphPoint {
                x: curve_end_x - horizontal_control,
                y: lower_line_y,
            },
            end: curve_end,
        },
        horizontal_end: Some(CommitGraphPoint {
            x: COMMIT_GRAPH_LANE_WIDTH - commit_graph_bend_overlay_x(),
            y: lower_line_y,
        }),
    }
}

pub(crate) fn commit_graph_merge_in_commit_bend_geometry() -> CommitGraphCubicBend {
    let end_x = commit_graph_commit_bend_overlay_dot_center_x();
    let end_y = commit_graph_merge_in_commit_bend_end_y();
    let radius = commit_graph_bend_radius();
    let start_x = end_x - radius;
    let lower_line_y = commit_graph_merge_in_commit_line_y();
    let horizontal_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let vertical_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;

    CommitGraphCubicBend {
        start: CommitGraphPoint {
            x: start_x,
            y: lower_line_y,
        },
        first_control: CommitGraphPoint {
            x: start_x + horizontal_control,
            y: lower_line_y,
        },
        second_control: CommitGraphPoint {
            x: end_x,
            y: end_y + vertical_control,
        },
        end: CommitGraphPoint { x: end_x, y: end_y },
    }
}

pub(crate) fn commit_graph_merge_target_commit_bend_geometry() -> CommitGraphCubicBend {
    let end_x = commit_graph_merge_target_commit_bend_overlay_dot_center_x();
    let end_y = commit_graph_merge_in_commit_bend_end_y();
    let radius = commit_graph_bend_radius();
    let start_x = end_x + radius;
    let lower_line_y = commit_graph_merge_in_commit_line_y();
    let horizontal_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let vertical_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;

    CommitGraphCubicBend {
        start: CommitGraphPoint {
            x: start_x,
            y: lower_line_y,
        },
        first_control: CommitGraphPoint {
            x: start_x - horizontal_control,
            y: lower_line_y,
        },
        second_control: CommitGraphPoint {
            x: end_x,
            y: end_y + vertical_control,
        },
        end: CommitGraphPoint { x: end_x, y: end_y },
    }
}

pub(crate) fn commit_graph_merge_in_commit_dot_connector_geometry() -> CommitGraphRect {
    let bend = commit_graph_merge_in_commit_bend_geometry();
    let width = commit_graph_line_width();
    CommitGraphRect {
        x: bend.end.x - width / 2.,
        y: -commit_graph_bend_overlay_top() + commit_graph_dot_bottom_gap_y(),
        width,
        height: width,
    }
}

pub(crate) fn commit_graph_shifted_merge_in_commit_dot_connector_geometry() -> CommitGraphRect {
    let bend = commit_graph_merge_in_commit_bend_geometry();
    let connector = commit_graph_merge_in_commit_dot_connector_geometry();

    CommitGraphRect {
        height: bend.end.y + commit_graph_lower_connector_vertical_shift() - connector.y
            + commit_graph_line_width() / 2.,
        ..connector
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CommitGraphNeighborRows<'a> {
    previous: Option<&'a graph::GraphRow>,
    next: Option<&'a graph::GraphRow>,
}

pub(crate) fn render_commit_graph_lane(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    neighbors: CommitGraphNeighborRows<'_>,
) -> gpui::Div {
    let has_incoming = row.incoming_lanes.contains(&lane);
    let has_outgoing = row.outgoing_lanes.contains(&lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let lane_selector = format!("commit-graph-lane-{row_index}-{lane}");
    // The upper merge curve into this commit paints first so the commit
    // lane's own vertical and dot stay on top of the joining branch.
    let upper_merge_target_elbow = (lane == row.lane)
        .then(|| {
            commit_graph_upper_merge_in_connectors(row)
                .into_iter()
                .min_by_key(|connector| connector.from_lane)
        })
        .flatten();

    div()
        .relative()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .debug_selector(move || lane_selector.clone())
        .when_some(upper_merge_target_elbow, |lane_div, connector| {
            lane_div.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(COMMIT_GRAPH_VERTICAL_HEIGHT))
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(commit_graph_middle_height()))
                    .child(render_commit_graph_rounded_elbow(
                        format!("commit-graph-rounded-upper-merge-target-elbow-{row_index}-{lane}"),
                        graph::GraphConnectorKind::MergeIn,
                        false,
                        commit_graph_upper_merge_in_horizontal_top_in_middle(),
                        commit_graph_connector_color(row, connector),
                    )),
            )
        })
        .child(render_commit_graph_vertical_segment(
            row_index,
            lane,
            row,
            neighbors,
            "top",
            has_incoming,
            lane_color,
        ))
        .child(render_commit_graph_middle_segment(
            row_index, lane, row, lane_color,
        ))
        .child(render_commit_graph_vertical_segment(
            row_index,
            lane,
            row,
            neighbors,
            "bottom",
            has_outgoing,
            lane_color,
        ))
}

pub(crate) fn commit_graph_lane_color(row: &graph::GraphRow, lane: usize) -> gpui::Rgba {
    const PALETTE: [u32; 6] = [0x60a5fa, 0xa3e635, 0xfbbf24, 0xf472b6, 0x2dd4bf, 0xc084fc];

    row.lane_colors
        .get(lane)
        .and_then(|color| *color)
        .map(|color| rgb(PALETTE[color % PALETTE.len()]))
        .unwrap_or_else(|| rgb(0x555555))
}

pub(crate) fn render_commit_graph_vertical_segment(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    neighbors: CommitGraphNeighborRows<'_>,
    position: &'static str,
    visible: bool,
    color: gpui::Rgba,
) -> gpui::Div {
    let selector = format!("commit-graph-vertical-{row_index}-{lane}-{position}");
    let (top, height) = commit_graph_vertical_segment_geometry(row, neighbors, lane, position);
    let segment = div()
        .absolute()
        .left(px(commit_graph_line_x()))
        .top(px(top))
        .w(px(COMMIT_GRAPH_LINE_WIDTH))
        .h(px(height))
        .when(visible, |segment| {
            segment.bg(color).debug_selector(move || selector.clone())
        });

    div()
        .relative()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_VERTICAL_HEIGHT))
        .child(segment)
}

pub(crate) fn commit_graph_vertical_segment_geometry(
    row: &graph::GraphRow,
    neighbors: CommitGraphNeighborRows<'_>,
    lane: usize,
    position: &'static str,
) -> (f32, f32) {
    if position == "top" {
        if let Some(top) = commit_graph_top_vertical_inset_after_previous_row_branch_out(
            row,
            neighbors.previous,
            lane,
        ) {
            return (top, COMMIT_GRAPH_VERTICAL_HEIGHT - top);
        }
    }

    if position == "bottom" {
        if let Some(height) =
            commit_graph_bottom_vertical_inset_before_next_row_merge_in(neighbors.next, lane)
        {
            return (0., height);
        }
    }

    if commit_graph_rounded_elbow_preserves_target_vertical(row, lane) {
        return (0., COMMIT_GRAPH_VERTICAL_HEIGHT);
    }

    let Some(tangent_y) = commit_graph_rounded_elbow_tangent_y(row, lane) else {
        return (0., COMMIT_GRAPH_VERTICAL_HEIGHT);
    };

    let middle_top = COMMIT_GRAPH_VERTICAL_HEIGHT;
    let middle_bottom = COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_middle_height();

    match position {
        "top" if tangent_y < middle_top => (
            0.,
            (tangent_y + commit_graph_line_width()).clamp(0., COMMIT_GRAPH_VERTICAL_HEIGHT),
        ),
        "bottom" if tangent_y > middle_bottom => {
            let top = (tangent_y - middle_bottom - commit_graph_line_width())
                .clamp(0., COMMIT_GRAPH_VERTICAL_HEIGHT);
            (top, COMMIT_GRAPH_VERTICAL_HEIGHT - top)
        }
        _ => (0., COMMIT_GRAPH_VERTICAL_HEIGHT),
    }
}

pub(crate) fn commit_graph_top_vertical_inset_after_previous_row_branch_out(
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    lane: usize,
) -> Option<f32> {
    if !row.incoming_lanes.contains(&lane) {
        return None;
    }

    let previous_row = previous_row?;
    let connector = commit_graph_target_connector_for_lane(previous_row, lane)?;
    if connector.kind != graph::GraphConnectorKind::BranchOut
        || !commit_graph_connector_uses_lower_branch_out_line(previous_row, connector)
        || commit_graph_rounded_elbow_turns_up(previous_row, lane)
    {
        return None;
    }

    Some(commit_graph_bend_radius() - commit_graph_line_width())
}

/// When the next row merges this lane into its commit along the upper border
/// line, this row's outgoing vertical stops at the curve tangent instead of
/// running into the bend.
pub(crate) fn commit_graph_bottom_vertical_inset_before_next_row_merge_in(
    next_row: Option<&graph::GraphRow>,
    lane: usize,
) -> Option<f32> {
    let next_row = next_row?;
    commit_graph_upper_merge_in_connectors(next_row)
        .iter()
        .any(|connector| connector.from_lane == lane)
        .then(|| {
            COMMIT_GRAPH_VERTICAL_HEIGHT - commit_graph_bend_radius() + commit_graph_line_width()
        })
}

/// Merge connectors that terminate at this row's commit while the commit lane
/// is also fed from above. These render along the row's upper border: the
/// branch verticals curve to horizontal and join the commit lane's vertical
/// just above the dot.
pub(crate) fn commit_graph_upper_merge_in_connectors(
    row: &graph::GraphRow,
) -> Vec<graph::GraphConnector> {
    row.connectors
        .iter()
        .copied()
        .filter(|connector| commit_graph_connector_uses_upper_merge_in_line(row, *connector))
        .collect()
}

pub(crate) fn commit_graph_connector_uses_upper_merge_in_line(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> bool {
    connector.kind == graph::GraphConnectorKind::MergeIn
        && connector.to_lane == row.lane
        && row.incoming_lanes.contains(&connector.to_lane)
}

pub(crate) fn commit_graph_uses_upper_merge_in_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    commit_graph_connector_uses_upper_merge_in_line(row, connector)
}

pub(crate) fn commit_graph_upper_merge_in_horizontal_top_in_middle() -> f32 {
    -(COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_line_width() / 2.)
}

pub(crate) fn commit_graph_rounded_elbow_preserves_target_vertical(
    row: &graph::GraphRow,
    lane: usize,
) -> bool {
    matches!(
        commit_graph_target_connector_for_lane(row, lane).map(|connector| connector.kind),
        Some(graph::GraphConnectorKind::BranchOut | graph::GraphConnectorKind::MergeIn)
    ) && row.incoming_lanes.contains(&lane)
        && row.outgoing_lanes.contains(&lane)
}

pub(crate) fn commit_graph_rounded_elbow_tangent_y(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<f32> {
    let Some(connector) = commit_graph_target_connector_for_lane(row, lane) else {
        // A branch lane merging along the upper border leaves this row
        // entirely: its curve sits above the row, so no vertical remains.
        let source_connector = commit_graph_source_connector_for_lane(row, lane)?;
        if !commit_graph_connector_uses_upper_merge_in_line(row, source_connector) {
            return None;
        }

        let middle_center_y = COMMIT_GRAPH_VERTICAL_HEIGHT
            + commit_graph_upper_merge_in_horizontal_top_in_middle()
            + commit_graph_line_width() / 2.;
        return Some(middle_center_y - commit_graph_bend_radius());
    };

    match connector.kind {
        graph::GraphConnectorKind::BranchOut | graph::GraphConnectorKind::MergeIn => {}
        graph::GraphConnectorKind::Straight => return None,
    }

    let middle_center_y = if commit_graph_uses_lower_branch_out_line(row, lane) {
        COMMIT_GRAPH_VERTICAL_HEIGHT
            + commit_graph_merge_in_commit_line_y_in_middle()
            + commit_graph_lower_connector_vertical_shift()
    } else {
        COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_middle_line_y() + commit_graph_line_width() / 2.
    };

    Some(if commit_graph_rounded_elbow_turns_up(row, lane) {
        middle_center_y - commit_graph_bend_radius()
    } else {
        middle_center_y + commit_graph_bend_radius()
    })
}

pub(crate) fn render_commit_graph_middle_segment(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    color: gpui::Rgba,
) -> gpui::Div {
    let is_commit = lane == row.lane;
    let has_connector = row.connector_lanes.contains(&lane);
    let has_middle_vertical =
        row.incoming_lanes.contains(&lane) || row.outgoing_lanes.contains(&lane);
    let connector_selector = format!("commit-graph-connector-{row_index}-{lane}");
    let middle_vertical_selector = format!("commit-graph-middle-vertical-{row_index}-{lane}");
    let dot_selector = format!("commit-graph-dot-{row_index}");
    let dot_top_gap_selector = format!("commit-graph-dot-top-gap-{row_index}-{lane}");
    let dot_bottom_gap_selector = format!("commit-graph-dot-bottom-gap-{row_index}-{lane}");
    let non_commit_connector_selector = connector_selector.clone();
    let commit_connector_selector = connector_selector;

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .when(has_connector && !is_commit, |middle| {
            middle.child(render_commit_graph_non_commit_connector(
                row_index,
                lane,
                row,
                non_commit_connector_selector.clone(),
            ))
        })
        .when(
            has_middle_vertical && !has_connector && !is_commit,
            |middle| {
                middle.child(
                    div()
                        .w(px(COMMIT_GRAPH_LINE_WIDTH))
                        .h(px(commit_graph_middle_height()))
                        .bg(color)
                        .debug_selector(move || middle_vertical_selector.clone()),
                )
            },
        )
        .when(is_commit, |middle| {
            let lane_span = row.connector_lanes.iter().copied();
            let min_lane = lane_span.clone().min().unwrap_or(lane);
            let max_lane = lane_span.max().unwrap_or(lane);
            let has_left_connector = has_connector && lane > min_lane;
            // Merges drawn along the upper border join the commit lane's
            // vertical above the dot, so they take no dot-height stub.
            let right_target_connector = commit_graph_target_connector_from_side(row, lane, true)
                .filter(|connector| !commit_graph_connector_uses_upper_merge_in_line(row, *connector));
            let right_side_has_non_upper_connector = row.connectors.iter().any(|connector| {
                connector.from_lane.max(connector.to_lane) > lane
                    && !commit_graph_connector_uses_upper_merge_in_line(row, *connector)
            });
            let has_right_connector = (has_connector
                && lane < max_lane
                && right_side_has_non_upper_connector)
                || right_target_connector.is_some();
            let left_connector = commit_graph_connector_on_side(row, lane, false);
            let right_connectors = commit_graph_connectors_on_side(row, lane, true);
            let right_connector =
                commit_graph_connector_on_side(row, lane, true).or(right_target_connector);
            let rounded_left_connector =
                commit_graph_commit_side_rounded_connector(row, lane, false);
            let rounded_right_connector =
                commit_graph_commit_side_rounded_connector(row, lane, true);
            let right_connector_is_rounded = right_connector
                .zip(rounded_right_connector)
                .is_some_and(|(right_connector, rounded_right_connector)| {
                    right_connector == rounded_right_connector
                });
            let left_connector_color = left_connector
                .map(|connector| commit_graph_connector_color(row, connector))
                .unwrap_or(color);
            let right_connector_color = right_connector
                .map(|connector| commit_graph_connector_color(row, connector))
                .unwrap_or(color);
            let right_connector_selector = right_target_connector
                .filter(|connector| connector.kind == graph::GraphConnectorKind::MergeIn)
                .map(|_| format!("commit-graph-merge-in-horizontal-{row_index}-{lane}"))
                .unwrap_or(commit_connector_selector);

            middle.child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(commit_graph_middle_height()))
                    .when(has_middle_vertical, |commit| {
                        commit
                            .when(row.incoming_lanes.contains(&lane), |commit| {
                                commit.child(
                                    div()
                                        .absolute()
                                        .left(px(commit_graph_line_x()))
                                        .top_0()
                                        .w(px(COMMIT_GRAPH_LINE_WIDTH))
                                        .h(px(commit_graph_dot_gap_height()))
                                        .bg(color)
                                        .debug_selector(move || dot_top_gap_selector.clone()),
                                )
                            })
                            .when(row.outgoing_lanes.contains(&lane), |commit| {
                                commit.child(
                                    div()
                                        .absolute()
                                        .left(px(commit_graph_line_x()))
                                        .top(px(commit_graph_dot_bottom_gap_y()))
                                        .w(px(COMMIT_GRAPH_LINE_WIDTH))
                                        .h(px(commit_graph_dot_gap_height()))
                                        .bg(color)
                                        .debug_selector(move || dot_bottom_gap_selector.clone()),
                                )
                            })
                    })
                    .child(
                        div()
                            .w(px(commit_graph_dot_side_line_width()))
                            .h(px(COMMIT_GRAPH_LINE_WIDTH))
                            .when(
                                has_left_connector && rounded_left_connector.is_none(),
                                |line| line.bg(left_connector_color),
                            ),
                    )
                    .when_some(rounded_left_connector, |commit, connector| {
                        let selector = match connector.kind {
                            graph::GraphConnectorKind::MergeIn => {
                                format!(
                                    "commit-graph-rounded-merge-in-commit-elbow-{row_index}-{lane}"
                                )
                            }
                            graph::GraphConnectorKind::BranchOut
                            | graph::GraphConnectorKind::Straight => {
                                format!("commit-graph-rounded-commit-elbow-{row_index}-{lane}")
                            }
                        };

                        commit.child(render_commit_graph_rounded_merge_in_commit_bend(
                            selector,
                            left_connector_color,
                        ))
                    })
                    .when_some(rounded_right_connector, |commit, connector| {
                        let selector = match connector.kind {
                            graph::GraphConnectorKind::BranchOut if right_connectors.len() > 1 => {
                                format!(
                                    "commit-graph-rounded-merge-target-commit-elbow-{row_index}-{lane}-{}",
                                    connector.to_lane
                                )
                            }
                            graph::GraphConnectorKind::BranchOut => {
                                format!(
                                    "commit-graph-rounded-merge-target-commit-elbow-{row_index}-{lane}"
                                )
                            }
                            graph::GraphConnectorKind::MergeIn
                            | graph::GraphConnectorKind::Straight => {
                                format!("commit-graph-rounded-commit-elbow-{row_index}-{lane}")
                            }
                        };

                        commit.child(render_commit_graph_rounded_merge_target_commit_bend(
                            selector,
                            commit_graph_connector_color(row, connector),
                        ))
                    })
                    .child(
                        div()
                            .w(px(COMMIT_GRAPH_DOT_SIZE))
                            .h(px(COMMIT_GRAPH_DOT_SIZE))
                            .rounded_full()
                            .bg(color)
                            .debug_selector(move || dot_selector.clone()),
                    )
                    .child(
                        div()
                            .w(px(commit_graph_dot_side_line_width()))
                            .h(px(COMMIT_GRAPH_LINE_WIDTH))
                            .when(
                                has_right_connector && !right_connector_is_rounded,
                                |line| {
                                    line.bg(right_connector_color)
                                        .debug_selector(move || right_connector_selector.clone())
                                },
                            ),
                    ),
            )
        })
}

pub(crate) fn commit_graph_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    commit_graph_target_connector_for_lane(row, lane)
        .or_else(|| commit_graph_source_connector_for_lane(row, lane))
        .or_else(|| commit_graph_spanning_connector_for_lane(row, lane))
}

pub(crate) fn commit_graph_target_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    row.connectors
        .iter()
        .copied()
        .find(|connector| connector.to_lane == lane)
}

pub(crate) fn commit_graph_target_connector_from_side(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    row.connectors.iter().copied().find(|connector| {
        connector.to_lane == lane
            && ((right && connector.from_lane > lane) || (!right && connector.from_lane < lane))
    })
}

pub(crate) fn commit_graph_source_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    row.connectors.iter().copied().find(|connector| {
        connector.from_lane == lane && connector.kind != graph::GraphConnectorKind::Straight
    })
}

pub(crate) fn commit_graph_spanning_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    row.connectors.iter().copied().find(|connector| {
        if connector.kind == graph::GraphConnectorKind::Straight {
            return false;
        }

        let min_lane = connector.from_lane.min(connector.to_lane);
        let max_lane = connector.from_lane.max(connector.to_lane);
        lane > min_lane && lane < max_lane
    })
}

pub(crate) fn commit_graph_spanning_connector_requires_center_fill(
    row: &graph::GraphRow,
    lane: usize,
) -> bool {
    commit_graph_target_connector_for_lane(row, lane).is_none()
        && commit_graph_spanning_connector_for_lane(row, lane).is_some()
        && !row.incoming_lanes.contains(&lane)
        && !row.outgoing_lanes.contains(&lane)
}

pub(crate) fn commit_graph_connector_on_side(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    commit_graph_connectors_on_side(row, lane, right)
        .into_iter()
        .next()
}

pub(crate) fn commit_graph_connectors_on_side(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Vec<graph::GraphConnector> {
    let mut connectors = row
        .connectors
        .iter()
        .copied()
        .filter(|connector| {
            (right && connector.to_lane > lane) || (!right && connector.to_lane < lane)
        })
        .collect::<Vec<_>>();
    connectors.sort_by_key(|connector| connector.to_lane.abs_diff(lane));
    connectors
}

pub(crate) fn commit_graph_commit_side_rounded_connector(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    if right {
        return commit_graph_connectors_on_side(row, lane, true)
            .into_iter()
            .find(|connector| commit_graph_connector_uses_lower_branch_out_line(row, *connector));
    }

    if row.outgoing_lanes.contains(&lane) || !row.incoming_lanes.contains(&lane) {
        return None;
    }

    row.connectors
        .iter()
        .copied()
        .filter(|connector| {
            connector.to_lane < lane && connector.kind == graph::GraphConnectorKind::MergeIn
        })
        .min_by_key(|connector| connector.to_lane.abs_diff(lane))
}

pub(crate) fn commit_graph_rounded_elbow_turns_up(row: &graph::GraphRow, lane: usize) -> bool {
    row.incoming_lanes.contains(&lane) && !row.outgoing_lanes.contains(&lane)
}

pub(crate) fn commit_graph_uses_lower_branch_out_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    commit_graph_connector_uses_lower_branch_out_line(row, connector)
}

pub(crate) fn commit_graph_connector_uses_lower_branch_out_line(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> bool {
    connector.kind == graph::GraphConnectorKind::BranchOut
        && row.outgoing_lanes.contains(&connector.to_lane)
}

pub(crate) fn commit_graph_uses_lower_merge_in_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    connector.kind == graph::GraphConnectorKind::MergeIn
        && connector.to_lane != row.lane
        && row.incoming_lanes.contains(&connector.to_lane)
        && row.outgoing_lanes.contains(&connector.to_lane)
        && row.incoming_lanes.contains(&connector.from_lane)
        && !row.outgoing_lanes.contains(&connector.from_lane)
}

pub(crate) fn commit_graph_connector_color(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> gpui::Rgba {
    commit_graph_lane_color(row, commit_graph_connector_color_lane(connector))
}

pub(crate) fn commit_graph_connector_color_lane(connector: graph::GraphConnector) -> usize {
    match connector.kind {
        graph::GraphConnectorKind::BranchOut => connector.to_lane,
        graph::GraphConnectorKind::MergeIn => connector.from_lane,
        graph::GraphConnectorKind::Straight => connector.to_lane,
    }
}

pub(crate) fn render_commit_graph_rounded_elbow(
    selector: String,
    kind: graph::GraphConnectorKind,
    turns_up: bool,
    horizontal_top_y: f32,
    connector_color: gpui::Rgba,
) -> gpui::Div {
    let overlay_height = commit_graph_bend_overlay_height()
        + (horizontal_top_y - commit_graph_lower_merge_in_horizontal_top_in_middle()).max(0.);

    div()
        .absolute()
        .left(px(commit_graph_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_bend_overlay_width()))
        .h(px(overlay_height))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let lane_offset_x = commit_graph_bend_overlay_lane_offset_x();
                    let center_x = bounds.origin.x
                        + px(lane_offset_x + commit_graph_line_x() + line_width / 2.);
                    let center_y = bounds.origin.y
                        + px(COMMIT_GRAPH_VERTICAL_HEIGHT + horizontal_top_y + line_width / 2.);
                    let left_x = bounds.origin.x;
                    let right_x = bounds.origin.x + px(commit_graph_bend_overlay_width());
                    let radius = px(commit_graph_bend_radius());
                    let control = px(commit_graph_bend_radius() * COMMIT_GRAPH_BEND_CUBIC_CONTROL);

                    let mut connector = PathBuilder::stroke(px(line_width));
                    match kind {
                        graph::GraphConnectorKind::BranchOut => {
                            connector.move_to(point(left_x, center_y));
                            connector.line_to(point(center_x - radius, center_y));
                            if turns_up {
                                connector.cubic_bezier_to(
                                    point(center_x, center_y - radius),
                                    point(center_x - radius + control, center_y),
                                    point(center_x, center_y - radius + control),
                                );
                            } else {
                                connector.cubic_bezier_to(
                                    point(center_x, center_y + radius),
                                    point(center_x - radius + control, center_y),
                                    point(center_x, center_y + radius - control),
                                );
                            }
                        }
                        graph::GraphConnectorKind::MergeIn => {
                            if turns_up {
                                connector.move_to(point(center_x, center_y - radius));
                                connector.cubic_bezier_to(
                                    point(center_x + radius, center_y),
                                    point(center_x, center_y - radius + control),
                                    point(center_x + radius - control, center_y),
                                );
                            } else {
                                connector.move_to(point(center_x, center_y + radius));
                                connector.cubic_bezier_to(
                                    point(center_x + radius, center_y),
                                    point(center_x, center_y + radius - control),
                                    point(center_x + radius - control, center_y),
                                );
                            }
                            connector.line_to(point(right_x, center_y));
                        }
                        graph::GraphConnectorKind::Straight => {}
                    }

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top_0()
            .w(px(commit_graph_bend_overlay_width()))
            .h(px(overlay_height)),
        )
}

pub(crate) fn render_commit_graph_rounded_branch_off_source_bend(
    selector: String,
    connector_color: gpui::Rgba,
    spans_occupied_lanes: bool,
    vertical_offset: f32,
) -> gpui::Div {
    div()
        .absolute()
        .left(px(commit_graph_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_bend_overlay_width()))
        .h(px(commit_graph_bend_overlay_height() + vertical_offset))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let bend = commit_graph_branch_off_source_bend_geometry(spans_occupied_lanes);

                    let mut connector = PathBuilder::stroke(px(line_width));
                    connector.move_to(point(
                        bounds.origin.x + px(bend.curve.start.x),
                        bounds.origin.y + px(bend.curve.start.y),
                    ));
                    connector.cubic_bezier_to(
                        point(
                            bounds.origin.x + px(bend.curve.end.x),
                            bounds.origin.y + px(bend.curve.end.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.curve.first_control.x),
                            bounds.origin.y + px(bend.curve.first_control.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.curve.second_control.x),
                            bounds.origin.y + px(bend.curve.second_control.y),
                        ),
                    );
                    if let Some(horizontal_end) = bend.horizontal_end {
                        connector.line_to(point(
                            bounds.origin.x + px(horizontal_end.x),
                            bounds.origin.y + px(horizontal_end.y),
                        ));
                    }

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top(px(vertical_offset))
            .w(px(commit_graph_bend_overlay_width()))
            .h(px(commit_graph_bend_overlay_height())),
        )
}

pub(crate) fn render_commit_graph_rounded_merge_in_commit_bend(
    selector: String,
    connector_color: gpui::Rgba,
) -> gpui::Div {
    let dot_connector_selector = format!("{selector}-dot-connector");
    let vertical_offset = commit_graph_lower_connector_vertical_shift();

    div()
        .absolute()
        .left(px(commit_graph_commit_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_commit_bend_overlay_width()))
        .h(px(commit_graph_shifted_bend_overlay_height()))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let bend = commit_graph_merge_in_commit_bend_geometry();
                    let horizontal_start_x = -commit_graph_commit_bend_overlay_x();

                    let mut connector = PathBuilder::stroke(px(line_width));
                    connector.move_to(point(
                        bounds.origin.x + px(horizontal_start_x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.line_to(point(
                        bounds.origin.x + px(bend.start.x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.cubic_bezier_to(
                        point(
                            bounds.origin.x + px(bend.end.x),
                            bounds.origin.y + px(bend.end.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.first_control.x),
                            bounds.origin.y + px(bend.first_control.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.second_control.x),
                            bounds.origin.y + px(bend.second_control.y),
                        ),
                    );

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top(px(vertical_offset))
            .w(px(commit_graph_commit_bend_overlay_width()))
            .h(px(commit_graph_bend_overlay_height())),
        )
        .child({
            let connector = commit_graph_shifted_merge_in_commit_dot_connector_geometry();
            div()
                .absolute()
                .left(px(connector.x))
                .top(px(connector.y))
                .w(px(connector.width))
                .h(px(connector.height))
                .bg(connector_color)
                .debug_selector(move || dot_connector_selector.clone())
        })
}

pub(crate) fn render_commit_graph_rounded_merge_target_commit_bend(
    selector: String,
    connector_color: gpui::Rgba,
) -> gpui::Div {
    let vertical_offset = commit_graph_lower_connector_vertical_shift();

    div()
        .absolute()
        .left(px(commit_graph_merge_target_commit_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_merge_target_commit_bend_overlay_width()))
        .h(px(commit_graph_shifted_bend_overlay_height()))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let bend = commit_graph_merge_target_commit_bend_geometry();
                    let horizontal_start_x = commit_graph_merge_target_commit_bend_overlay_width();

                    let mut connector = PathBuilder::stroke(px(line_width));
                    connector.move_to(point(
                        bounds.origin.x + px(horizontal_start_x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.line_to(point(
                        bounds.origin.x + px(bend.start.x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.cubic_bezier_to(
                        point(
                            bounds.origin.x + px(bend.end.x),
                            bounds.origin.y + px(bend.end.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.first_control.x),
                            bounds.origin.y + px(bend.first_control.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.second_control.x),
                            bounds.origin.y + px(bend.second_control.y),
                        ),
                    );

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top(px(vertical_offset))
            .w(px(commit_graph_merge_target_commit_bend_overlay_width()))
            .h(px(commit_graph_bend_overlay_height())),
        )
}

pub(crate) fn render_commit_graph_non_commit_connector(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    connector_selector: String,
) -> gpui::Div {
    let target_connector = commit_graph_target_connector_for_lane(row, lane);
    let source_connector = commit_graph_source_connector_for_lane(row, lane);
    let connector = commit_graph_connector_for_lane(row, lane);
    let has_incoming = row.incoming_lanes.contains(&lane);
    let has_outgoing = row.outgoing_lanes.contains(&lane);
    let preserve_target_vertical = commit_graph_rounded_elbow_preserves_target_vertical(row, lane);
    let uses_lower_merge_in_line = commit_graph_uses_lower_merge_in_line(row, lane);
    let uses_lower_branch_out_line = commit_graph_uses_lower_branch_out_line(row, lane);
    let uses_upper_merge_in_line = commit_graph_uses_upper_merge_in_line(row, lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let color = connector
        .map(|connector| commit_graph_connector_color(row, connector))
        .unwrap_or(lane_color);
    // An outer edge crossing this merging lane along the upper border keeps
    // its own color underneath this lane's bend.
    let upper_merge_crossing = source_connector
        .filter(|connector| commit_graph_connector_uses_upper_merge_in_line(row, *connector))
        .and_then(|_| commit_graph_spanning_connector_for_lane(row, lane))
        .filter(|connector| commit_graph_connector_uses_upper_merge_in_line(row, *connector));
    let (left_visible, right_visible) = match (target_connector, source_connector) {
        (Some(connector), _) => match connector.kind {
            graph::GraphConnectorKind::BranchOut => (true, false),
            graph::GraphConnectorKind::MergeIn => (false, true),
            graph::GraphConnectorKind::Straight => (true, true),
        },
        (None, Some(connector))
            if connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane =>
        {
            // Along the upper border a crossing edge is drawn as a full-width
            // underlay instead; on the middle line another edge merging across
            // this lane still needs the right half of the horizontal.
            (
                true,
                !uses_upper_merge_in_line
                    && commit_graph_spanning_connector_for_lane(row, lane).is_some(),
            )
        }
        _ => (true, true),
    };
    let kind_selector = target_connector.and_then(|connector| match connector.kind {
        graph::GraphConnectorKind::BranchOut => {
            Some(format!("commit-graph-branch-out-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::MergeIn => {
            Some(format!("commit-graph-merge-in-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::Straight => None,
    });
    let elbow_selector = target_connector.and_then(|connector| match connector.kind {
        graph::GraphConnectorKind::BranchOut => {
            Some(format!("commit-graph-branch-out-elbow-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::MergeIn => {
            Some(format!("commit-graph-merge-in-elbow-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::Straight => None,
    });
    let rounded_elbow = target_connector.and_then(|connector| match connector.kind {
        graph::GraphConnectorKind::BranchOut => Some((
            format!("commit-graph-rounded-branch-out-elbow-{row_index}-{lane}"),
            connector.kind,
        )),
        graph::GraphConnectorKind::MergeIn if !uses_lower_merge_in_line => Some((
            format!("commit-graph-rounded-merge-in-elbow-{row_index}-{lane}"),
            connector.kind,
        )),
        graph::GraphConnectorKind::MergeIn => None,
        graph::GraphConnectorKind::Straight => None,
    });
    let lower_merge_in_source_bend = target_connector.and_then(|connector| {
        (connector.kind == graph::GraphConnectorKind::MergeIn && uses_lower_merge_in_line).then(
            || {
                (
                    format!("commit-graph-rounded-branch-off-source-elbow-{row_index}-{lane}"),
                    connector.from_lane > lane + 1,
                )
            },
        )
    });
    let source_merge_in_bend = source_connector.and_then(|connector| {
        (connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane).then(
            || {
                (
                    format!("commit-graph-rounded-merge-in-source-elbow-{row_index}-{lane}"),
                    graph::GraphConnectorKind::BranchOut,
                )
            },
        )
    });
    let spanning_through_target_connector = target_connector
        .and_then(|_| commit_graph_spanning_connector_for_lane(row, lane))
        .filter(|connector| commit_graph_connector_uses_lower_branch_out_line(row, *connector));
    let left_horizontal_is_rounded = rounded_elbow
        .as_ref()
        .is_some_and(|(_, kind)| *kind == graph::GraphConnectorKind::BranchOut)
        || source_merge_in_bend.is_some();
    let right_horizontal_is_rounded = rounded_elbow
        .as_ref()
        .is_some_and(|(_, kind)| *kind == graph::GraphConnectorKind::MergeIn)
        || lower_merge_in_source_bend.is_some();
    let has_rounded_elbow = left_horizontal_is_rounded || right_horizontal_is_rounded;
    let horizontal_top_y = if uses_upper_merge_in_line {
        commit_graph_upper_merge_in_horizontal_top_in_middle()
    } else if uses_lower_merge_in_line || uses_lower_branch_out_line {
        commit_graph_shifted_lower_merge_in_horizontal_top_in_middle()
    } else {
        commit_graph_middle_line_y()
    };
    let elbow_top = if has_incoming { 0. } else { horizontal_top_y };
    let elbow_bottom = if uses_lower_branch_out_line && has_outgoing {
        horizontal_top_y + commit_graph_line_width()
    } else if has_outgoing {
        commit_graph_middle_height()
    } else {
        commit_graph_middle_line_bottom_y()
    };
    let elbow_height = elbow_bottom - elbow_top;
    let middle_vertical_selector = format!("commit-graph-middle-vertical-{row_index}-{lane}");
    let has_middle_vertical = has_incoming || has_outgoing;
    let center_fill_selector =
        format!("commit-graph-spanning-horizontal-center-{row_index}-{lane}");
    let spanning_left_selector =
        format!("commit-graph-spanning-horizontal-left-{row_index}-{lane}");
    let spanning_right_selector =
        format!("commit-graph-spanning-horizontal-right-{row_index}-{lane}");
    let spanning_through_target_selector =
        format!("commit-graph-spanning-horizontal-through-target-{row_index}-{lane}");
    let source_merge_in_right_selector = source_connector.and_then(|connector| {
        (connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane)
            .then(|| format!("commit-graph-merge-in-source-horizontal-right-{row_index}-{lane}"))
    });
    let incoming_vertical_bridge = target_connector.and_then(|connector| {
        if connector.kind != graph::GraphConnectorKind::BranchOut
            || !commit_graph_rounded_elbow_turns_up(row, lane)
        {
            return None;
        }

        let tangent_y = commit_graph_rounded_elbow_tangent_y(row, lane)?;
        (tangent_y > COMMIT_GRAPH_VERTICAL_HEIGHT).then(|| {
            (
                format!("commit-graph-rounded-branch-out-vertical-bridge-{row_index}-{lane}"),
                tangent_y - COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_line_width(),
            )
        })
    });
    let fill_spanning_center = commit_graph_spanning_connector_requires_center_fill(row, lane);

    let upper_merge_crossing_selector =
        format!("commit-graph-upper-merge-crossing-{row_index}-{lane}");
    let mut connector_shape = div()
        .relative()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .when_some(upper_merge_crossing, |shape, crossing| {
            shape.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(horizontal_top_y))
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(COMMIT_GRAPH_LINE_WIDTH))
                    .bg(commit_graph_connector_color(row, crossing))
                    .debug_selector(move || upper_merge_crossing_selector.clone()),
            )
        })
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(horizontal_top_y))
                .w(px(commit_graph_line_x()))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .when(left_visible, |line| {
                    line.when(!left_horizontal_is_rounded, |line| line.bg(color))
                        .when_some(
                            target_connector.and_then(|connector| {
                                (connector.kind == graph::GraphConnectorKind::BranchOut).then(
                                    || {
                                        format!(
                                            "commit-graph-branch-out-horizontal-{row_index}-{lane}"
                                        )
                                    },
                                )
                            }),
                            |line, selector| line.debug_selector(move || selector.clone()),
                        )
                        .when(
                            target_connector.is_none()
                                && commit_graph_spanning_connector_for_lane(row, lane).is_some(),
                            |line| line.debug_selector(move || spanning_left_selector.clone()),
                        )
                }),
        )
        .child(
            div()
                .absolute()
                .left(px(commit_graph_right_line_x()))
                .top(px(horizontal_top_y))
                .w(px(commit_graph_right_line_width()))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .when(right_visible, |line| {
                    line.when(!right_horizontal_is_rounded, |line| line.bg(color))
                        .when_some(
                            target_connector.and_then(|connector| {
                                (connector.kind == graph::GraphConnectorKind::MergeIn).then(|| {
                                    format!("commit-graph-merge-in-horizontal-{row_index}-{lane}")
                                })
                            }),
                            |line, selector| line.debug_selector(move || selector.clone()),
                        )
                        .when(
                            target_connector.is_none()
                                && commit_graph_spanning_connector_for_lane(row, lane).is_some(),
                            |line| line.debug_selector(move || spanning_right_selector.clone()),
                        )
                        .when_some(source_merge_in_right_selector, |line, selector| {
                            line.debug_selector(move || selector.clone())
                        })
                }),
        );

    if fill_spanning_center {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(horizontal_top_y))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .bg(color)
                .debug_selector(move || center_fill_selector.clone()),
        );
    }

    if let Some(spanning_connector) = spanning_through_target_connector {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(
                    commit_graph_shifted_lower_merge_in_horizontal_top_in_middle(),
                ))
                .w(px(COMMIT_GRAPH_LANE_WIDTH))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .bg(commit_graph_connector_color(row, spanning_connector))
                .debug_selector(move || spanning_through_target_selector.clone()),
        );
    }

    if let Some(kind_selector) = kind_selector {
        connector_shape = connector_shape.debug_selector(move || kind_selector.clone());
    }

    if let Some(elbow_selector) = elbow_selector {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(elbow_top))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(elbow_height))
                .when(!has_rounded_elbow, |elbow| {
                    elbow.bg(if has_middle_vertical {
                        lane_color
                    } else {
                        color
                    })
                })
                .debug_selector(move || elbow_selector.clone()),
        );
    }

    if has_middle_vertical {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(elbow_top))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(elbow_height))
                .when(!has_rounded_elbow || preserve_target_vertical, |vertical| {
                    vertical.bg(lane_color)
                })
                .debug_selector(move || middle_vertical_selector.clone()),
        );
    }

    if let Some((incoming_vertical_bridge_selector, incoming_vertical_bridge_height)) =
        incoming_vertical_bridge
    {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top_0()
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(incoming_vertical_bridge_height))
                .bg(lane_color)
                .debug_selector(move || incoming_vertical_bridge_selector.clone()),
        );
    }

    if let Some((source_bend_selector, source_bend_spans_occupied_lanes)) =
        lower_merge_in_source_bend
    {
        connector_shape =
            connector_shape.child(render_commit_graph_rounded_branch_off_source_bend(
                source_bend_selector,
                color,
                source_bend_spans_occupied_lanes,
                commit_graph_lower_connector_vertical_shift(),
            ));
    }

    if let Some((source_bend_selector, source_bend_kind)) = source_merge_in_bend {
        connector_shape = connector_shape.child(render_commit_graph_rounded_elbow(
            source_bend_selector,
            source_bend_kind,
            has_incoming && !has_outgoing,
            horizontal_top_y,
            color,
        ));
    }

    if let Some((rounded_elbow_selector, rounded_elbow_kind)) = rounded_elbow {
        connector_shape = connector_shape.child(render_commit_graph_rounded_elbow(
            rounded_elbow_selector,
            rounded_elbow_kind,
            target_connector
                .map(|_| commit_graph_rounded_elbow_turns_up(row, lane))
                .unwrap_or(false),
            horizontal_top_y,
            color,
        ));
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .debug_selector(move || connector_selector.clone())
        .child(connector_shape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::graph::{self, GraphConnectorKind};
    use crate::repo::{self, INITIAL_COMMIT_LIMIT};
    use gpui::{px, TestAppContext, VisualTestContext};

    #[test]
    fn hiding_a_remote_branch_leaves_a_same_named_local_branch_visible() {
        // A local branch literally named "origin/main" and the remote-tracking
        // origin/main key independently: hiding one never hides the other.
        let commits = vec![
            commit_info("local-tip", &["root"]),
            commit_info("remote-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("origin/main", "local-tip"),
            remote_branch("origin", "main", "remote-tip"),
        ];

        let visible =
            visible_commit_shas(&commits, &branches, None, &hidden(&["remotes/origin/main"]));

        assert!(visible.contains("local-tip"));
        assert!(!visible.contains("remote-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn commit_ref_labels_mark_remote_branches_and_show_both_at_a_shared_tip() {
        let mut commit = commit_info("tip", &[]);
        commit.branch_labels = vec![
            repo::BranchLabel {
                name: "main".to_string(),
                kind: repo::BranchKind::Local,
            },
            repo::BranchLabel {
                name: "origin/main".to_string(),
                kind: repo::BranchKind::Remote {
                    remote: "origin".to_string(),
                },
            },
        ];

        let labels = commit_ref_labels(&commit, &BTreeSet::new());

        assert_eq!(
            labels,
            vec![
                CommitRefLabel {
                    name: "main".to_string(),
                    selector_key: "heads/main".to_string(),
                    kind: CommitRefLabelKind::Branch,
                },
                CommitRefLabel {
                    name: "origin/main".to_string(),
                    selector_key: "remotes/origin/main".to_string(),
                    kind: CommitRefLabelKind::RemoteBranch,
                },
            ],
            "a shared tip shows both labels, the remote one marked as remote",
        );
    }

    #[test]
    fn commit_ref_labels_hide_only_the_hidden_namespace() {
        let mut commit = commit_info("tip", &[]);
        commit.branch_labels = vec![
            repo::BranchLabel {
                name: "origin/main".to_string(),
                kind: repo::BranchKind::Local,
            },
            repo::BranchLabel {
                name: "origin/main".to_string(),
                kind: repo::BranchKind::Remote {
                    remote: "origin".to_string(),
                },
            },
        ];
        let hidden = ["remotes/origin/main".to_string()].into_iter().collect();

        let labels = commit_ref_labels(&commit, &hidden);

        assert_eq!(
            labels.len(),
            1,
            "hiding the remote ref leaves the same-named local label",
        );
        assert_eq!(labels[0].kind, CommitRefLabelKind::Branch);
    }

    #[test]
    fn hiding_a_branch_removes_its_exclusive_commits() {
        // feature-tip -> root <- main-tip; hiding feature drops feature-tip only.
        let commits = vec![
            commit_info("feature-tip", &["root"]),
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "feature-tip"),
            local_branch("master", "main-tip"),
        ];

        let visible = visible_commit_shas(
            &commits,
            &branches,
            Some("main-tip"),
            &hidden(&["heads/feature"]),
        );

        assert!(!visible.contains("feature-tip"));
        assert!(visible.contains("main-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn shared_ancestry_survives_hiding_a_branch() {
        // feature points at root, which master also reaches: root stays.
        let commits = vec![commit_info("main-tip", &["root"]), commit_info("root", &[])];
        let branches = vec![
            local_branch("feature", "root"),
            local_branch("master", "main-tip"),
        ];

        let visible = visible_commit_shas(
            &commits,
            &branches,
            Some("main-tip"),
            &hidden(&["heads/feature"]),
        );

        assert!(visible.contains("root"));
        assert!(visible.contains("main-tip"));
    }

    #[test]
    fn head_chain_is_visible_even_with_no_visible_branches() {
        let commits = vec![commit_info("head-tip", &["root"]), commit_info("root", &[])];
        let branches = vec![local_branch("feature", "head-tip")];

        let visible = visible_commit_shas(
            &commits,
            &branches,
            Some("head-tip"),
            &hidden(&["heads/feature"]),
        );

        assert!(visible.contains("head-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn missing_head_walks_from_branch_tips_only() {
        let commits = vec![
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
            commit_info("orphan", &[]),
        ];
        let branches = vec![local_branch("master", "main-tip")];

        let visible = visible_commit_shas(&commits, &branches, None, &BTreeSet::new());

        assert!(visible.contains("main-tip"));
        assert!(visible.contains("root"));
        assert!(!visible.contains("orphan"));
    }

    #[test]
    fn empty_hidden_set_keeps_every_loaded_commit() {
        let commits = vec![
            commit_info("feature-tip", &["root"]),
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "feature-tip"),
            local_branch("master", "main-tip"),
        ];

        let visible = visible_commit_shas(&commits, &branches, Some("main-tip"), &BTreeSet::new());

        // Every commit here is reachable from a branch tip; the function
        // returns reachable commits, not loaded commits, so the counts only
        // match because this topology has no orphans.
        assert_eq!(visible.len(), commits.len());
    }

    #[test]
    fn parents_beyond_the_loaded_boundary_are_ignored() {
        // root's parent is not loaded; the walk must terminate, not panic.
        let commits = vec![commit_info("root", &["unloaded-parent"])];
        let branches = vec![local_branch("master", "root")];

        let visible = visible_commit_shas(&commits, &branches, Some("root"), &BTreeSet::new());

        assert!(visible.contains("root"));
        assert!(!visible.contains("unloaded-parent"));
    }

    #[test]
    fn commit_graph_horizontal_connectors_use_branch_lane_color() {
        let rows = graph::layout_graph(&[
            graph_commit("merge", &["left", "right"]),
            graph_commit("left", &["base"]),
            graph_commit("right", &["base"]),
            graph_commit("base", &[]),
        ]);

        let branch_out = rows[0]
            .connectors
            .iter()
            .copied()
            .find(|connector| connector.kind == GraphConnectorKind::BranchOut)
            .expect("branch-out connector");
        assert_eq!(commit_graph_connector_color_lane(branch_out), 1);

        let merge_in = rows[3]
            .connectors
            .iter()
            .copied()
            .find(|connector| connector.kind == GraphConnectorKind::MergeIn)
            .expect("merge-in connector");
        assert_eq!(commit_graph_connector_color_lane(merge_in), 1);
    }

    #[test]
    fn commit_graph_connectors_span_intermediate_lanes() {
        let connector = graph::GraphConnector {
            from_lane: 0,
            to_lane: 2,
            kind: GraphConnectorKind::BranchOut,
        };
        let row = graph::GraphRow {
            sha: "wide-branch".to_string(),
            lane: 0,
            lane_count: 3,
            active_lanes: vec![0],
            incoming_lanes: Vec::new(),
            outgoing_lanes: vec![0, 2],
            parent_lanes: vec![0, 2],
            connector_lanes: vec![0, 1, 2],
            connectors: vec![connector],
            lane_colors: vec![Some(0), None, Some(1)],
        };

        assert_eq!(commit_graph_connector_for_lane(&row, 1), Some(connector));
        assert_eq!(commit_graph_connector_color_lane(connector), 2);
    }

    #[test]
    fn commit_graph_empty_spanning_connectors_fill_the_center_gap() {
        let connector = graph::GraphConnector {
            from_lane: 0,
            to_lane: 2,
            kind: GraphConnectorKind::BranchOut,
        };
        let row = graph::GraphRow {
            sha: "wide-branch".to_string(),
            lane: 0,
            lane_count: 3,
            active_lanes: vec![0],
            incoming_lanes: Vec::new(),
            outgoing_lanes: vec![0, 2],
            parent_lanes: vec![0, 2],
            connector_lanes: vec![0, 1, 2],
            connectors: vec![connector],
            lane_colors: vec![Some(0), None, Some(1)],
        };

        assert!(commit_graph_spanning_connector_requires_center_fill(
            &row, 1
        ));
    }

    #[test]
    fn commit_rows_do_not_draw_separators_between_graph_segments() {
        assert_eq!(commit_row_separator_width(), 0.);
    }

    #[test]
    fn commit_graph_bend_radius_is_large_enough_for_smooth_elbows() {
        let bend_radius = super::commit_graph_bend_radius();
        let middle_height = super::commit_graph_middle_height();
        let overlay_height = super::commit_graph_bend_overlay_height();
        let overlay_top = super::commit_graph_bend_overlay_top();
        let line_width = super::commit_graph_line_width();

        assert_eq!(bend_radius, 8.);
        assert_eq!(middle_height, 10.);
        assert!(
            overlay_top < 0.,
            "rounded bends should draw outside the compact middle band instead of stretching it",
        );
        assert!(
            overlay_height >= bend_radius * 2. + line_width,
            "rounded bend overlay should fit the full rounded bend radius",
        );
    }

    #[test]
    fn commit_side_branch_bend_turns_from_horizontal_into_vertical() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();

        assert!(
            bend.first_control.x > bend.start.x,
            "component 3 should start right-first from the horizontal segment",
        );
        assert_eq!(
            bend.first_control.y, bend.start.y,
            "component 3 should have a horizontal tangent at the start",
        );
        assert_eq!(
            bend.second_control.x, bend.end.x,
            "component 3 should end on the branch lane vertical",
        );
        assert!(
            bend.second_control.y > bend.end.y,
            "component 3 should curve upward into the branch lane vertical",
        );
    }

    #[test]
    fn commit_side_branch_bend_keeps_original_shape_before_row_boundary_shift() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();
        let radius = super::commit_graph_bend_radius();
        let control = radius * super::COMMIT_GRAPH_BEND_CUBIC_CONTROL;
        let bend_end_x_in_commit = super::commit_graph_commit_bend_overlay_x() + bend.end.x;
        let dot_center_x =
            super::commit_graph_dot_side_line_width() + super::COMMIT_GRAPH_DOT_SIZE / 2.;
        let bend_end_y_in_middle = super::commit_graph_bend_overlay_top() + bend.end.y;
        let dot_bottom_y = super::commit_graph_dot_bottom_gap_y();

        assert_eq!(
            bend_end_x_in_commit, dot_center_x,
            "component 3 should end on the commit dot's vertical centerline",
        );
        assert_eq!(
            bend_end_y_in_middle,
            dot_bottom_y + super::commit_graph_line_width() / 2.,
            "component 3's local shape should stay just below the commit dot before paint-time shifting",
        );
        assert_eq!(
            bend.end.x - bend.start.x,
            radius,
            "component 3 should use a circular horizontal radius",
        );
        assert_eq!(
            bend.start.y - bend.end.y,
            radius,
            "component 3 should use the same vertical radius as a circular quadrant",
        );
        assert_eq!(
            bend.first_control.x,
            bend.start.x + control,
            "component 3 first control should preserve circular quadrant geometry",
        );
        assert_eq!(
            bend.second_control.y,
            bend.end.y + control,
            "component 3 second control should preserve circular quadrant geometry",
        );
    }

    #[test]
    fn commit_side_branch_dot_connector_bridges_bend_endpoint() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();
        let connector = super::commit_graph_merge_in_commit_dot_connector_geometry();
        let dot_bottom_y =
            -super::commit_graph_bend_overlay_top() + super::commit_graph_dot_bottom_gap_y();

        assert_eq!(
            connector.x + connector.width / 2.,
            bend.end.x,
            "dot connector should be centered on component 3's vertical tangent",
        );
        assert_eq!(
            connector.width,
            super::commit_graph_line_width(),
            "dot connector should match the graph stroke width",
        );
        assert_eq!(
            connector.y, dot_bottom_y,
            "dot connector should start exactly at the commit dot bottom edge",
        );
        assert!(
            connector.y <= bend.end.y && connector.y + connector.height >= bend.end.y,
            "dot connector should cover the component 3 endpoint seam",
        );
    }

    #[test]
    fn shifted_commit_side_branch_dot_connector_bridges_translated_bend_endpoint() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();
        let connector = super::commit_graph_shifted_merge_in_commit_dot_connector_geometry();
        let original_connector = super::commit_graph_merge_in_commit_dot_connector_geometry();
        let shifted_bend_endpoint_y =
            bend.end.y + super::commit_graph_lower_connector_vertical_shift();

        assert_eq!(
            connector.y, original_connector.y,
            "moving the bend should not move the dot-side filler away from the commit dot",
        );
        assert_eq!(
            connector.height,
            original_connector.height + super::commit_graph_lower_connector_vertical_shift(),
            "dot-side filler should lengthen by the same amount as the bend moved",
        );
        assert!(
            connector.y <= shifted_bend_endpoint_y
                && connector.y + connector.height >= shifted_bend_endpoint_y,
            "dot-side filler should cover the translated component 3 endpoint",
        );
    }

    #[test]
    fn commit_side_merge_target_bend_turns_from_horizontal_into_vertical() {
        let bend = super::commit_graph_merge_target_commit_bend_geometry();

        assert!(
            bend.first_control.x < bend.start.x,
            "merge target component 3 should start left-first from the horizontal segment",
        );
        assert_eq!(
            bend.first_control.y, bend.start.y,
            "merge target component 3 should have a horizontal tangent at the start",
        );
        assert_eq!(
            bend.second_control.x, bend.end.x,
            "merge target component 3 should end on the target commit vertical",
        );
        assert!(
            bend.second_control.y > bend.end.y,
            "merge target component 3 should curve upward into the target commit vertical",
        );
    }

    #[test]
    fn commit_side_merge_target_bend_keeps_original_shape_before_row_boundary_shift() {
        let bend = super::commit_graph_merge_target_commit_bend_geometry();
        let radius = super::commit_graph_bend_radius();
        let control = radius * super::COMMIT_GRAPH_BEND_CUBIC_CONTROL;
        let bend_end_x_in_commit =
            super::commit_graph_merge_target_commit_bend_overlay_x() + bend.end.x;
        let dot_center_x =
            super::commit_graph_dot_side_line_width() + super::COMMIT_GRAPH_DOT_SIZE / 2.;
        let bend_end_y_in_middle = super::commit_graph_bend_overlay_top() + bend.end.y;
        let dot_bottom_y = super::commit_graph_dot_bottom_gap_y();

        assert_eq!(
            bend_end_x_in_commit, dot_center_x,
            "merge target component 3 should end on the commit dot's vertical centerline",
        );
        assert_eq!(
            bend_end_y_in_middle,
            dot_bottom_y + super::commit_graph_line_width() / 2.,
            "merge target component 3's local shape should stay just below the commit dot before paint-time shifting",
        );
        assert_eq!(
            bend.start.x - bend.end.x,
            radius,
            "merge target component 3 should use a circular horizontal radius",
        );
        assert_eq!(
            bend.start.y - bend.end.y,
            radius,
            "merge target component 3 should use the same vertical radius as a circular quadrant",
        );
        assert_eq!(
            bend.first_control.x,
            bend.start.x - control,
            "merge target component 3 first control should preserve circular quadrant geometry",
        );
        assert_eq!(
            bend.second_control.y,
            bend.end.y + control,
            "merge target component 3 second control should preserve circular quadrant geometry",
        );
    }

    #[test]
    fn branch_off_horizontal_component_uses_tangent_bounds_and_baseline() {
        let adjacent = super::commit_graph_branch_off_source_bend_geometry(false);
        assert!(
            adjacent.horizontal_end.is_some(),
            "adjacent branch-off bends need a short horizontal component between circular bends",
        );

        let spanning = super::commit_graph_branch_off_source_bend_geometry(true);
        let horizontal_end = spanning
            .horizontal_end
            .expect("spanning branch-off should draw component 2");
        assert_eq!(
            horizontal_end.y, spanning.curve.end.y,
            "component 2 should share the source bend tangent baseline",
        );
        assert!(
            horizontal_end.x > spanning.curve.end.x,
            "component 2 should begin after component 1 reaches its tangent",
        );
        assert_eq!(
            super::commit_graph_lower_merge_in_horizontal_top_in_middle()
                + super::commit_graph_line_width() / 2.,
            super::commit_graph_merge_in_commit_line_y_in_middle(),
            "component 2 filled segments should be centered on the bend baseline",
        );
    }

    #[test]
    fn lane_change_horizontal_baseline_is_centered_on_the_row_boundary() {
        let row_boundary_center_y_in_middle =
            super::COMMIT_GRAPH_LANE_HEIGHT - super::COMMIT_GRAPH_VERTICAL_HEIGHT;

        assert_eq!(
            super::commit_graph_shifted_lower_merge_in_horizontal_top_in_middle()
                + super::commit_graph_line_width() / 2.,
            row_boundary_center_y_in_middle,
            "horizontal lane-change strokes should be centered on the border between graph rows",
        );
    }

    #[test]
    fn commit_graph_overlay_paints_lower_rows_first() {
        assert_eq!(
            super::commit_graph_overlay_row_indices(4),
            vec![3, 2, 1, 0],
            "lower graph rows should paint first so row-boundary branch turns cover the next row's vertical continuation",
        );
    }

    #[test]
    fn adjacent_branch_off_horizontal_component_connects_circular_bends() {
        let source_bend = super::commit_graph_branch_off_source_bend_geometry(false);
        let commit_bend = super::commit_graph_merge_in_commit_bend_geometry();
        let radius = super::commit_graph_bend_radius();

        let source_horizontal_end = source_bend
            .horizontal_end
            .expect("adjacent branch-offs should draw a short horizontal component");
        let source_horizontal_end_x =
            super::commit_graph_bend_overlay_x() + source_horizontal_end.x;
        let commit_horizontal_start_x = super::COMMIT_GRAPH_LANE_WIDTH;
        let commit_curve_start_x = super::COMMIT_GRAPH_LANE_WIDTH
            + super::commit_graph_commit_bend_overlay_x()
            + commit_bend.start.x;

        assert_eq!(
            source_bend.curve.end.x - source_bend.curve.start.x,
            radius,
            "component 1 should remain a circular quadrant",
        );
        assert_eq!(
            source_bend.curve.start.y - source_bend.curve.end.y,
            radius,
            "component 1 should use the same vertical radius as a circular quadrant",
        );
        assert_eq!(
            source_horizontal_end_x, commit_horizontal_start_x,
            "component 2 should reach the adjacent commit lane boundary",
        );
        assert!(
            commit_curve_start_x > commit_horizontal_start_x,
            "component 2 should continue inside the commit lane until component 3 starts",
        );
        assert_eq!(
            source_bend.curve.end.y, commit_bend.start.y,
            "adjacent branch-off bends should share the same lower baseline",
        );
    }

    #[gpui::test]
    async fn commit_graph_renders_merge_lanes(cx: &mut TestAppContext) {
        let (dir, _left_sha, _right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                assert_eq!(repo.commits[0].parent_shas.len(), 2);
            })
            .expect("read merge commit");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("commit-graph-gutter-0")
            .expect("merge commit graph gutter debug bounds");
        visual
            .debug_bounds("commit-graph-dot-0")
            .expect("merge commit graph dot debug bounds");
        let merge_commit_dot = visual
            .debug_bounds("commit-graph-dot-0")
            .expect("merge commit graph dot debug bounds");
        let merge_commit_bottom_gap = visual
            .debug_bounds("commit-graph-dot-bottom-gap-0-0")
            .expect("merge commit bottom dot gap debug bounds");
        visual
            .debug_bounds("commit-graph-lane-0-1")
            .expect("merge commit second parent lane debug bounds");
        let branch_out_connector_bounds = visual
            .debug_bounds("commit-graph-connector-0-1")
            .expect("merge commit second parent connector debug bounds");
        visual
            .debug_bounds("commit-graph-branch-out-0-1")
            .expect("merge commit branch-out connector debug bounds");
        let branch_out_elbow_bounds = visual
            .debug_bounds("commit-graph-branch-out-elbow-0-1")
            .expect("merge commit branch-out elbow debug bounds");
        let rounded_branch_out_elbow_bounds = visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-0-1")
            .expect("merge commit rounded branch-out elbow debug bounds");
        let merge_target_commit_bend_bounds = visual
            .debug_bounds("commit-graph-rounded-merge-target-commit-elbow-0-0")
            .expect("merge target rounded commit bend debug bounds");
        assert!(
            rounded_branch_out_elbow_bounds.origin.y < branch_out_connector_bounds.origin.y,
            "rounded branch-out elbow should draw outside the compact middle band above the connector",
        );
        assert!(
            rounded_branch_out_elbow_bounds.origin.y
                + rounded_branch_out_elbow_bounds.size.height
                > branch_out_connector_bounds.origin.y + branch_out_connector_bounds.size.height,
            "rounded branch-out elbow should draw outside the compact middle band below the connector",
        );
        let branch_out_middle_vertical_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-0-1")
            .expect("merge commit branch-out middle vertical debug bounds");
        let branch_out_horizontal_bounds = visual
            .debug_bounds("commit-graph-branch-out-horizontal-0-1")
            .expect("merge commit branch-out horizontal debug bounds");
        let merge_commit_row = visual
            .debug_bounds("commit-row-0")
            .expect("merge commit row debug bounds");
        assert_eq!(
            branch_out_horizontal_bounds.origin.y + px(commit_graph_line_width() / 2.),
            merge_target_commit_bend_bounds.origin.y
                + px(super::commit_graph_lower_connector_vertical_shift()
                    + commit_graph_merge_in_commit_line_y()),
            "branch-out horizontal should meet the merge target bend on the lower baseline",
        );
        assert_eq!(
            branch_out_horizontal_bounds.origin.y + px(commit_graph_line_width() / 2.),
            merge_commit_row.origin.y + merge_commit_row.size.height,
            "branch-out horizontal should be centered on the border below the merge row",
        );
        assert_eq!(
            branch_out_middle_vertical_bounds.origin.y, branch_out_horizontal_bounds.origin.y,
            "branch-out middle vertical should not protrude above the horizontal turn",
        );
        let branch_out_vertical_bounds = visual
            .debug_bounds("commit-graph-vertical-0-1-bottom")
            .expect("merge commit second parent outgoing vertical debug bounds");
        assert_eq!(
            branch_out_elbow_bounds.origin.x, branch_out_vertical_bounds.origin.x,
            "branch-out elbow should align with the outgoing lane",
        );
        assert!(
            branch_out_vertical_bounds.origin.y
                >= merge_commit_row.origin.y + merge_commit_row.size.height
                    - px(commit_graph_line_width()),
            "branch-out outgoing vertical should not pull the branch turn above the row border",
        );
        let merge_commit_bottom_bounds = visual
            .debug_bounds("commit-graph-vertical-0-0-bottom")
            .expect("merge commit trunk outgoing vertical debug bounds");
        assert_eq!(
            merge_commit_dot.origin.y + merge_commit_dot.size.height,
            merge_commit_bottom_gap.origin.y,
            "trunk dot gap fill should start at the commit dot edge",
        );
        assert_eq!(
            merge_commit_bottom_gap.origin.y + merge_commit_bottom_gap.size.height,
            merge_commit_bottom_bounds.origin.y,
            "trunk dot gap fill should connect to the outgoing trunk vertical",
        );
        assert!(
            merge_target_commit_bend_bounds.origin.y + merge_target_commit_bend_bounds.size.height
                > merge_commit_dot.origin.y + merge_commit_dot.size.height,
            "merge target commit bend should have room below the trunk commit dot",
        );
        visual
            .debug_bounds("commit-graph-vertical-0-1-bottom")
            .expect("merge commit second parent outgoing vertical debug bounds");
        let continued_lane_top_bounds = visual
            .debug_bounds("commit-graph-vertical-1-1-top")
            .expect("continued second lane incoming vertical debug bounds");
        let continued_lane_row = visual
            .debug_bounds("commit-row-1")
            .expect("continued second lane row debug bounds");
        let continued_lane_middle_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-1-1")
            .expect("continued second lane middle vertical debug bounds");
        let continued_lane_bottom_bounds = visual
            .debug_bounds("commit-graph-vertical-1-1-bottom")
            .expect("continued second lane outgoing vertical debug bounds");
        assert_eq!(
            continued_lane_top_bounds.origin.y,
            continued_lane_row.origin.y
                + px(super::commit_graph_bend_radius() - super::commit_graph_line_width()),
            "continued vertical should start at the previous row's branch-out curve tangent, not at the row border",
        );
        assert_eq!(
            continued_lane_middle_bounds.origin.x, continued_lane_top_bounds.origin.x,
            "continued lane middle vertical should align with the incoming vertical",
        );
        assert_eq!(
            continued_lane_top_bounds.origin.y + continued_lane_top_bounds.size.height,
            continued_lane_middle_bounds.origin.y,
            "continued lane middle vertical should connect to the incoming vertical",
        );
        assert_eq!(
            continued_lane_middle_bounds.origin.y + continued_lane_middle_bounds.size.height,
            continued_lane_bottom_bounds.origin.y,
            "continued lane middle vertical should connect to the outgoing vertical",
        );
        assert!(
            visual.debug_bounds("commit-graph-merge-in-2-0").is_none()
                && visual
                    .debug_bounds("commit-graph-rounded-merge-in-commit-elbow-2-1")
                    .is_none(),
            "the right branch commit should sit on a straight vertical instead of bending on its own row",
        );
        let right_branch_top_vertical = visual
            .debug_bounds("commit-graph-vertical-2-1-top")
            .expect("right branch incoming vertical debug bounds");
        let right_branch_bottom_vertical = visual
            .debug_bounds("commit-graph-vertical-2-1-bottom")
            .expect("right branch outgoing vertical debug bounds");
        assert_eq!(
            right_branch_top_vertical.origin.x, right_branch_bottom_vertical.origin.x,
            "right branch edge should continue straight through its commit row",
        );
        let merge_in_source_elbow_bounds = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-3-1")
            .expect("base row merge-in source elbow debug bounds");
        let upper_merge_target_elbow_bounds = visual
            .debug_bounds("commit-graph-rounded-upper-merge-target-elbow-3-0")
            .expect("base row upper merge target elbow debug bounds");
        let base_dot_bounds = visual
            .debug_bounds("commit-graph-dot-3")
            .expect("base commit dot debug bounds");
        let base_row_bounds = visual
            .debug_bounds("commit-row-3")
            .expect("base commit row debug bounds");
        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-3-0")
                .is_none(),
            "the merge should join the trunk vertical above the dot, not tee into the dot",
        );
        assert_eq!(
            upper_merge_target_elbow_bounds.origin.y, base_row_bounds.origin.y,
            "the merge target curve should sit on the base row's upper border",
        );
        assert_eq!(
            merge_in_source_elbow_bounds.origin.y, base_row_bounds.origin.y,
            "the merge source curve should sit on the base row's upper border",
        );
        assert!(
            merge_in_source_elbow_bounds.origin.x
                > base_dot_bounds.origin.x + base_dot_bounds.size.width,
            "merge-in source elbow should curve up in the branch lane right of the base dot",
        );
        assert_eq!(
            right_branch_bottom_vertical.size.height,
            px(
                super::COMMIT_GRAPH_VERTICAL_HEIGHT - super::commit_graph_bend_radius()
                    + commit_graph_line_width()
            ),
            "the branch edge should stop at the merge curve tangent above the base row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-vertical-3-1-bottom")
                .is_none(),
            "the branch edge should end at the base row",
        );
    }

    #[gpui::test]
    async fn commit_graph_keeps_side_parent_lane_active_when_trunk_merge_shares_parent(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph_at("merge-lfs", 50, &["merge-docs", "lfs-tip"]),
                        commit_info_for_graph_at("lfs-tip", 40, &["trunk-base"]),
                        commit_info_for_graph_at("merge-docs", 30, &["trunk-base", "docs-tip"]),
                        commit_info_for_graph_at("docs-tip", 20, &["trunk-base"]),
                        commit_info_for_graph_at("trunk-base", 10, &[]),
                    ],
                );
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let graph_commits = repo
                    .commits
                    .iter()
                    .map(|commit| graph::GraphCommit {
                        sha: commit.sha.clone(),
                        authored_timestamp: commit.authored_timestamp,
                        parent_shas: commit.parent_shas.clone(),
                    })
                    .collect::<Vec<_>>();
                let rows = graph::layout_graph(&graph_commits);

                assert_eq!(rows[0].parent_lanes, vec![0, 2]);
                assert_eq!(rows[0].connector_lanes, vec![0, 1, 2]);

                assert_eq!(rows[1].lane, 2);
                assert_eq!(rows[1].parent_lanes, vec![2]);
                assert_eq!(rows[1].connector_lanes, vec![2]);
                assert_eq!(rows[1].outgoing_lanes, vec![0, 2]);

                assert_eq!(rows[2].lane, 0);
                assert_eq!(rows[2].incoming_lanes, vec![0, 2]);
                assert_eq!(rows[2].outgoing_lanes, vec![0, 1, 2]);
                assert_eq!(rows[2].parent_lanes, vec![0, 1]);
                assert_eq!(rows[2].connector_lanes, vec![0, 1]);
                assert!(
                    rows[2].connectors.iter().any(|connector| {
                        connector.from_lane == 0
                            && connector.to_lane == 0
                            && connector.kind == GraphConnectorKind::Straight
                    }),
                    "the trunk merge should keep its first-parent edge on the trunk lane",
                );
                assert!(
                    !rows[2].connectors.iter().any(|connector| {
                        connector.from_lane == 0
                            && connector.to_lane == 2
                            && connector.kind == GraphConnectorKind::BranchOut
                    }),
                    "the lfs side edge should not branch from the docs merge row",
                );
                assert!(
                    rows[2].connectors.iter().any(|connector| {
                        connector.from_lane == 0
                            && connector.to_lane == 1
                            && connector.kind == GraphConnectorKind::BranchOut
                    }),
                    "the docs side branch should branch independently from the merge row",
                );
                assert_eq!(rows[3].connector_lanes, vec![1]);
                assert!(
                    rows[3]
                        .connectors
                        .iter()
                        .all(|connector| { connector.from_lane == 1 && connector.to_lane == 1 }),
                    "the docs branch edge should run straight down its own lane",
                );
                assert_eq!(
                    rows[3].outgoing_lanes,
                    vec![0, 1, 2],
                    "both side edges should stay active until the shared parent row",
                );
                assert_eq!(rows[4].connector_lanes, vec![0, 1, 2]);
                assert!(
                    rows[4].connectors.contains(&graph::GraphConnector {
                        from_lane: 1,
                        to_lane: 0,
                        kind: GraphConnectorKind::MergeIn,
                    }) && rows[4].connectors.contains(&graph::GraphConnector {
                        from_lane: 2,
                        to_lane: 0,
                        kind: GraphConnectorKind::MergeIn,
                    }),
                    "both side edges should merge into the shared parent on its own row",
                );
            })
            .expect("inspect graph layout");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let side_top = visual
            .debug_bounds("commit-graph-vertical-2-2-top")
            .expect("side lane top vertical through merge row");
        let side_middle = visual
            .debug_bounds("commit-graph-middle-vertical-2-2")
            .expect("side lane middle vertical through merge row");
        let side_bottom = visual
            .debug_bounds("commit-graph-vertical-2-2-bottom")
            .expect("side lane bottom vertical through merge row");

        assert!(
            visual
                .debug_bounds("commit-graph-rounded-spanning-branch-end-elbow-2-2")
                .is_none(),
            "the side lane should pass through the merge row instead of joining the docs branch",
        );
        visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-0-2")
            .expect("lfs side branch should open directly into lane 2");
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-branch-out-elbow-1-2")
                .is_none(),
            "lfs side branch should not hop from lane 1 to lane 2 at its commit row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-spanning-horizontal-through-target-2-1")
                .is_none(),
            "lfs side branch should not draw a horizontal connector on the docs merge row",
        );
        visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-2-1")
            .expect("docs side branch should occupy the first side lane");
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-merge-in-commit-elbow-3-1")
                .is_none()
                && visual
                    .debug_bounds("commit-graph-rounded-merge-target-commit-elbow-3-1")
                    .is_none(),
            "the docs commit should sit on a straight vertical instead of bending on its own row",
        );
        let docs_row_lfs_top = visual
            .debug_bounds("commit-graph-vertical-3-2-top")
            .expect("lfs edge incoming vertical through the docs row");
        let docs_row_lfs_middle = visual
            .debug_bounds("commit-graph-middle-vertical-3-2")
            .expect("lfs edge middle vertical through the docs row");
        let docs_row_lfs_bottom = visual
            .debug_bounds("commit-graph-vertical-3-2-bottom")
            .expect("lfs edge outgoing vertical through the docs row");
        assert_eq!(
            docs_row_lfs_top.origin.y + docs_row_lfs_top.size.height,
            docs_row_lfs_middle.origin.y,
            "lfs edge should pass the docs row without a gap above the middle segment",
        );
        assert_eq!(
            docs_row_lfs_middle.origin.y + docs_row_lfs_middle.size.height,
            docs_row_lfs_bottom.origin.y,
            "lfs edge should pass the docs row without a gap below the middle segment",
        );
        let docs_merge_source_elbow = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-4-1")
            .expect("docs edge should curve into the shared parent on its row");
        let lfs_merge_source_elbow = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-4-2")
            .expect("lfs edge should curve into the shared parent on its row");
        let upper_merge_target_elbow = visual
            .debug_bounds("commit-graph-rounded-upper-merge-target-elbow-4-0")
            .expect("shared parent should curve the merge into its trunk vertical");
        let lfs_crossing_underlay = visual
            .debug_bounds("commit-graph-upper-merge-crossing-4-1")
            .expect("lfs edge should keep its own horizontal underneath the docs bend");
        let parent_row = visual
            .debug_bounds("commit-row-4")
            .expect("shared parent commit row debug bounds");
        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-4-0")
                .is_none(),
            "the merges should join the trunk vertical above the dot, not tee into the dot",
        );
        assert_eq!(
            lfs_crossing_underlay.origin.y + px(super::commit_graph_line_width() / 2.),
            parent_row.origin.y,
            "the crossing merge horizontal should be centered on the shared parent row's upper border",
        );
        assert_eq!(
            docs_merge_source_elbow.origin.y, lfs_merge_source_elbow.origin.y,
            "both merge source elbows should share the same vertical placement",
        );
        assert_eq!(
            upper_merge_target_elbow.origin.y, docs_merge_source_elbow.origin.y,
            "the trunk-side merge curve should align with the branch-side curves",
        );
        assert!(
            docs_merge_source_elbow.origin.x < lfs_merge_source_elbow.origin.x,
            "the docs elbow should sit in the inner lane, the lfs elbow in the outer lane",
        );
        let docs_row_lfs_bottom_inset = visual
            .debug_bounds("commit-graph-vertical-3-2-bottom")
            .expect("lfs edge outgoing vertical above the shared parent row");
        assert_eq!(
            docs_row_lfs_bottom_inset.size.height,
            px(
                super::COMMIT_GRAPH_VERTICAL_HEIGHT - super::commit_graph_bend_radius()
                    + super::commit_graph_line_width()
            ),
            "the lfs edge should stop at its merge curve tangent above the shared parent row",
        );
        assert_eq!(
            side_top.origin.y + side_top.size.height,
            side_middle.origin.y,
            "side lane top segment should connect to the middle segment",
        );
        assert!(
            side_bottom.origin.y <= side_middle.origin.y + side_middle.size.height,
            "side lane middle segment should not leave a gap before the bottom segment",
        );
    }

    #[gpui::test]
    async fn commit_graph_renders_unmerged_branch_tip_above_head(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                let mut head_commit = commit_info_for_graph_at("head-tip", 20, &["fork"]);
                head_commit.is_head = true;
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph_at("feature-tip", 30, &["fork"]),
                        head_commit,
                        commit_info_for_graph_at("fork", 10, &[]),
                    ],
                );
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let feature_dot = visual
            .debug_bounds("commit-graph-dot-0")
            .expect("unmerged branch tip renders a commit dot");
        let head_dot = visual
            .debug_bounds("commit-graph-dot-1")
            .expect("HEAD commit renders a commit dot");
        let fork_dot = visual
            .debug_bounds("commit-graph-dot-2")
            .expect("fork commit renders a commit dot");

        assert!(
            feature_dot.origin.x > head_dot.origin.x,
            "the unmerged branch tip should sit in a side lane right of HEAD's trunk lane",
        );
        assert_eq!(
            head_dot.origin.x, fork_dot.origin.x,
            "HEAD's first-parent history should keep the trunk lane",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-vertical-0-0-top")
                .is_none()
                && visual
                    .debug_bounds("commit-graph-middle-vertical-0-0")
                    .is_none(),
            "the trunk lane should stay empty above the HEAD row",
        );
        visual
            .debug_bounds("commit-graph-vertical-2-1-top")
            .expect("the branch lane should run into its fork row");
    }

    #[gpui::test]
    async fn commit_graph_keeps_merge_in_horizontal_aligned_across_occupied_lanes(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        // feature-x passes through trunk-mid's row in lane 1 while feature-y
        // merges into trunk-mid from lane 2, crossing the occupied lane.
        window
            .update(cx, |app, _window, cx| {
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph_at("trunk-tip", 60, &["trunk-mid"]),
                        commit_info_for_graph_at("feature-x", 50, &["trunk-base"]),
                        commit_info_for_graph_at("feature-y", 40, &["trunk-mid"]),
                        commit_info_for_graph_at("trunk-mid", 30, &["trunk-base"]),
                        commit_info_for_graph_at("trunk-base", 20, &[]),
                    ],
                );
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let graph_commits = repo
                    .commits
                    .iter()
                    .map(|commit| graph::GraphCommit {
                        sha: commit.sha.clone(),
                        authored_timestamp: commit.authored_timestamp,
                        parent_shas: commit.parent_shas.clone(),
                    })
                    .collect::<Vec<_>>();
                let rows = graph::layout_graph(&graph_commits);
                assert_eq!(rows[1].lane, 1);
                assert_eq!(rows[2].lane, 2);
                assert_eq!(rows[3].lane, 0);
                assert_eq!(rows[3].incoming_lanes, vec![0, 1, 2]);
                assert_eq!(rows[3].outgoing_lanes, vec![0, 1]);
                assert_eq!(rows[3].connector_lanes, vec![0, 1, 2]);
                assert!(rows[3].connectors.contains(&graph::GraphConnector {
                    from_lane: 2,
                    to_lane: 0,
                    kind: GraphConnectorKind::MergeIn,
                }));
            })
            .expect("inspect graph layout");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let spanning_left = visual
            .debug_bounds("commit-graph-spanning-horizontal-left-3-1")
            .expect("occupied intermediate lane left merge-in horizontal debug bounds");
        let spanning_right = visual
            .debug_bounds("commit-graph-spanning-horizontal-right-3-1")
            .expect("occupied intermediate lane right merge-in horizontal debug bounds");
        let source_elbow = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-3-2")
            .expect("source-side merge-in elbow debug bounds");
        let target_elbow = visual
            .debug_bounds("commit-graph-rounded-upper-merge-target-elbow-3-0")
            .expect("trunk-side merge curve debug bounds");
        let trunk_mid_dot = visual
            .debug_bounds("commit-graph-dot-3")
            .expect("trunk-mid commit dot debug bounds");
        let trunk_mid_row = visual
            .debug_bounds("commit-row-3")
            .expect("trunk-mid commit row debug bounds");
        let occupied_lane_above = visual
            .debug_bounds("commit-graph-vertical-2-1-bottom")
            .expect("occupied lane outgoing vertical above the merge row");
        let occupied_lane_top = visual
            .debug_bounds("commit-graph-vertical-3-1-top")
            .expect("occupied lane incoming vertical through the merge row");

        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-3-0")
                .is_none(),
            "the merge should join the trunk vertical above the dot, not tee into the dot",
        );
        assert_eq!(
            spanning_left.origin.y, spanning_right.origin.y,
            "merge-in horizontal should stay aligned on both sides of the occupied lane",
        );
        assert_eq!(
            spanning_left.origin.y + px(commit_graph_line_width() / 2.),
            trunk_mid_row.origin.y,
            "merge-in horizontal should be centered on the merge row's upper border",
        );
        assert_eq!(
            target_elbow.origin.y, source_elbow.origin.y,
            "the trunk-side merge curve should align with the branch-side curve",
        );
        assert_eq!(
            occupied_lane_above.size.height,
            px(super::COMMIT_GRAPH_VERTICAL_HEIGHT),
            "the occupied pass-through lane should keep its full vertical above the merge row",
        );
        assert_eq!(
            occupied_lane_above.origin.y + occupied_lane_above.size.height,
            occupied_lane_top.origin.y,
            "the occupied lane vertical should cross the merge horizontal without interruption",
        );
        assert!(
            source_elbow.origin.x > trunk_mid_dot.origin.x,
            "the merge source elbow should curve up in the outer branch lane",
        );
    }

    #[gpui::test]
    async fn commit_graph_vertical_segments_connect_between_rows(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let first_row_bottom = visual
            .debug_bounds("commit-graph-vertical-0-0-bottom")
            .expect("first row outgoing vertical debug bounds");
        let second_row_top = visual
            .debug_bounds("commit-graph-vertical-1-0-top")
            .expect("second row incoming vertical debug bounds");
        let first_row = visual
            .debug_bounds("commit-row-0")
            .expect("first commit row debug bounds");
        let second_row = visual
            .debug_bounds("commit-row-1")
            .expect("second commit row debug bounds");
        let first_dot = visual
            .debug_bounds("commit-graph-dot-0")
            .expect("first commit dot debug bounds");
        let second_dot = visual
            .debug_bounds("commit-graph-dot-1")
            .expect("second commit dot debug bounds");
        let first_bottom_gap = visual
            .debug_bounds("commit-graph-dot-bottom-gap-0-0")
            .expect("first commit bottom dot gap debug bounds");
        let second_top_gap = visual
            .debug_bounds("commit-graph-dot-top-gap-1-0")
            .expect("second commit top dot gap debug bounds");

        assert_eq!(
            first_row_bottom.origin.y + first_row_bottom.size.height,
            second_row_top.origin.y,
            "commit graph vertical segments should connect across adjacent rows; first row: {first_row:?}, second row: {second_row:?}, first bottom: {first_row_bottom:?}, second top: {second_row_top:?}",
        );
        assert_eq!(
            first_dot.origin.y + first_dot.size.height,
            first_bottom_gap.origin.y,
            "bottom dot gap fill should start at the dot edge",
        );
        assert_eq!(
            first_bottom_gap.origin.y + first_bottom_gap.size.height,
            first_row_bottom.origin.y,
            "bottom dot gap fill should connect to the outgoing vertical",
        );
        assert_eq!(
            second_row_top.origin.y + second_row_top.size.height,
            second_top_gap.origin.y,
            "top dot gap fill should connect to the incoming vertical",
        );
        assert_eq!(
            second_top_gap.origin.y + second_top_gap.size.height,
            second_dot.origin.y,
            "top dot gap fill should end at the dot edge",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-commit-vertical-0-0")
                .is_none()
                && visual
                    .debug_bounds("commit-graph-commit-vertical-1-0")
                    .is_none(),
            "commit dots should not get full-height through-lines that protrude beyond the dot",
        );
    }

    #[gpui::test]
    async fn commit_rows_render_as_single_line_columns_in_requested_order(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                app.mode = Mode::RepoOpen {
                    repo: crate::repo::OpenRepository {
                        path: dir.path().to_path_buf(),
                        head: Some(crate::repo::HeadInfo {
                            short_sha: "abcdef0".to_string(),
                            summary: "Compact row".to_string(),
                        }),
                        commits: vec![crate::repo::CommitInfo {
                            sha: "abcdef0123456789abcdef0123456789abcdef01".to_string(),
                            short_sha: "abcdef0".to_string(),
                            summary: "Collapse graph row into columns".to_string(),
                            author: "Greviewer Tests".to_string(),
                            authored_timestamp: 0,
                            authored_date: "1970-01-01".to_string(),
                            parent_shas: Vec::new(),
                            branch_labels: vec![repo::BranchLabel {
                                name: "main".to_string(),
                                kind: repo::BranchKind::Local,
                            }],
                            parent_count: 0,
                            is_head: true,
                        }],
                        has_more_commits: false,
                        branches: Vec::new(),
                    },
                };
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("commit-row-0")
            .expect("commit row debug bounds");
        let graph = visual
            .debug_bounds("commit-graph-gutter-0")
            .expect("graph gutter debug bounds");
        let hash = visual
            .debug_bounds("commit-hash-0")
            .expect("commit hash debug bounds");
        let summary = visual
            .debug_bounds("commit-summary-0")
            .expect("commit summary debug bounds");
        let author = visual
            .debug_bounds("commit-author-0")
            .expect("commit author debug bounds");
        let time = visual
            .debug_bounds("commit-time-0")
            .expect("commit time debug bounds");
        let labels = visual
            .debug_bounds("commit-ref-labels-0")
            .expect("commit labels debug bounds");

        assert!(
            row.size.height <= px(44.),
            "commit row should be compact enough for a single-line layout: {row:?}"
        );
        assert!(graph.origin.x < hash.origin.x, "graph should be first");
        assert!(hash.origin.x < summary.origin.x, "hash should follow graph");
        assert!(
            summary.origin.x < author.origin.x,
            "summary should precede author"
        );
        assert!(
            author.origin.x < time.origin.x,
            "author should precede time"
        );
        assert!(
            time.origin.x < labels.origin.x,
            "time should precede labels"
        );
    }

    #[gpui::test]
    async fn commit_rows_render_head_and_branch_labels(cx: &mut TestAppContext) {
        let (dir, _left_sha, _right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let (head_row, master_row, left_row, right_row) = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let row_for_branch = |branch_name: &str| {
                    repo.commits
                        .iter()
                        .position(|commit| {
                            commit
                                .branch_labels
                                .iter()
                                .any(|label| label.name == branch_name)
                        })
                        .expect("branch row")
                };

                (
                    repo.commits
                        .iter()
                        .position(|commit| commit.is_head)
                        .expect("head row"),
                    row_for_branch("master"),
                    row_for_branch("left"),
                    row_for_branch("right"),
                )
            })
            .expect("read branch label rows");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let label_selector = |row: usize, label: &str| {
            Box::leak(format!("commit-ref-label-{row}-{label}").into_boxed_str()) as &'static str
        };
        visual
            .debug_bounds(label_selector(head_row, "head"))
            .expect("head label on merge commit");
        visual
            .debug_bounds(label_selector(master_row, "heads-master"))
            .expect("master label on merge commit");
        visual
            .debug_bounds(label_selector(left_row, "heads-left"))
            .expect("left branch label on left commit");
        visual
            .debug_bounds(label_selector(right_row, "heads-right"))
            .expect("right branch label on right commit");
    }

    #[gpui::test]
    async fn detached_head_repositories_render_without_a_head_marker(cx: &mut TestAppContext) {
        let (dir, tip_sha) = init_repo_with_detached_head();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open detached HEAD repo");

        cx.run_until_parked();

        let master_row = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };

                assert_eq!(repo.commits.len(), 2);
                assert_eq!(repo.commits[0].sha, tip_sha);
                assert!(
                    repo.commits.iter().all(|commit| !commit.is_head),
                    "detached HEAD should not mark a checked-out branch tip"
                );

                repo.commits
                    .iter()
                    .position(|commit| {
                        commit
                            .branch_labels
                            .iter()
                            .any(|label| label.name == "master")
                    })
                    .expect("master branch row")
            })
            .expect("read detached HEAD repo");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("commit-row-0")
            .expect("tip commit row debug bounds");
        visual
            .debug_bounds(test_debug_selector(format!(
                "commit-ref-label-{master_row}-heads-master"
            )))
            .expect("master branch label debug bounds");
        assert!(
            visual.debug_bounds("commit-ref-label-0-head").is_none(),
            "detached HEAD should not render a HEAD label"
        );
    }

    #[gpui::test]
    async fn long_branch_labels_do_not_cover_the_commit_graph(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let branch_name = "not-merged-branch-with-a-name-that-would-cover-the-graph".to_string();
        let label_selector = Box::leak(
            format!(
                "commit-ref-label-0-heads-{}",
                debug_ref_label_fragment(&branch_name)
            )
            .into_boxed_str(),
        ) as &'static str;
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                app.mode = Mode::RepoOpen {
                    repo: crate::repo::OpenRepository {
                        path: dir.path().to_path_buf(),
                        head: Some(crate::repo::HeadInfo {
                            short_sha: "abcdef0".to_string(),
                            summary: "Long branch label".to_string(),
                        }),
                        commits: vec![crate::repo::CommitInfo {
                            sha: "abcdef0123456789abcdef0123456789abcdef01".to_string(),
                            short_sha: "abcdef0".to_string(),
                            summary: "Long branch label".to_string(),
                            author: "Greviewer Tests".to_string(),
                            authored_timestamp: 0,
                            authored_date: "1970-01-01".to_string(),
                            parent_shas: Vec::new(),
                            branch_labels: vec![repo::BranchLabel {
                                name: branch_name,
                                kind: repo::BranchKind::Local,
                            }],
                            parent_count: 0,
                            is_head: false,
                        }],
                        has_more_commits: false,
                        branches: Vec::new(),
                    },
                };
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let label_bounds = visual
            .debug_bounds(label_selector)
            .expect("long branch label debug bounds");
        let graph_bounds = visual
            .debug_bounds("commit-graph-gutter-0")
            .expect("commit graph gutter debug bounds");

        assert!(
            label_bounds.origin.x >= graph_bounds.origin.x + graph_bounds.size.width,
            "branch label should not cover the graph gutter; label: {label_bounds:?}, graph: {graph_bounds:?}"
        );
    }

    #[gpui::test]
    async fn scrolling_commit_history_loads_older_commits(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, shas) = init_repo_with_linear_history(INITIAL_COMMIT_LIMIT + 2);
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                assert_eq!(repo.commits.len(), INITIAL_COMMIT_LIMIT);
                assert!(repo.has_more_commits);
            })
            .expect("read initial commit page");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        let first_row_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("first commit row debug bounds");
        let before_scroll = window
            .read_with(cx, |app, _cx| app.commit_history_scroll.offset())
            .expect("read commit history offset before wheel");
        visual.simulate_event(ScrollWheelEvent {
            position: first_row_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
            ..Default::default()
        });
        cx.run_until_parked();
        let (after_scroll, max_scroll) = window
            .read_with(cx, |app, _cx| {
                (
                    app.commit_history_scroll.offset(),
                    app.commit_history_scroll.max_offset(),
                )
            })
            .expect("read commit history offset after wheel");
        assert!(
            max_scroll.height > px(0.),
            "long commit history should exceed the visible graph area"
        );
        assert!(
            after_scroll.y < before_scroll.y,
            "wheel scroll should move the commit history upward; before: {before_scroll:?}, after: {after_scroll:?}, max: {max_scroll:?}"
        );

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                assert_eq!(repo.commits.len(), INITIAL_COMMIT_LIMIT + 2);
                assert!(!repo.has_more_commits);
                assert_eq!(
                    repo.commits.last().expect("oldest loaded commit").sha,
                    shas[INITIAL_COMMIT_LIMIT + 1]
                );
            })
            .expect("read loaded commit page");

        let oldest_row_selector =
            Box::leak(format!("commit-row-{}", INITIAL_COMMIT_LIMIT + 1).into_boxed_str())
                as &'static str;
        visual
            .debug_bounds(oldest_row_selector)
            .expect("oldest loaded commit row debug bounds");
    }
}
