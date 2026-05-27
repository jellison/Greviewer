//! Commit graph layout.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub sha: String,
    pub parent_shas: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphConnectorKind {
    Straight,
    BranchOut,
    MergeIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphConnector {
    pub from_lane: usize,
    pub to_lane: usize,
    pub kind: GraphConnectorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub sha: String,
    pub lane: usize,
    pub lane_count: usize,
    pub active_lanes: Vec<usize>,
    pub incoming_lanes: Vec<usize>,
    pub outgoing_lanes: Vec<usize>,
    pub parent_lanes: Vec<usize>,
    pub connector_lanes: Vec<usize>,
    pub connectors: Vec<GraphConnector>,
    pub lane_colors: Vec<Option<usize>>,
}

pub fn layout_graph(commits: &[GraphCommit]) -> Vec<GraphRow> {
    let top_first_parent_history = top_first_parent_history(commits);
    let mut active_shas = Vec::<Option<String>>::new();
    let mut active_colors = Vec::<Option<usize>>::new();
    let mut next_color = 0;
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let incoming_lanes = occupied_lanes(&active_shas);
        let is_top_first_parent = contains_sha(&top_first_parent_history, &commit.sha);
        let lane = commit_lane(
            commit,
            is_top_first_parent,
            &mut active_shas,
            &mut active_colors,
            &mut next_color,
        );

        let active_lanes = occupied_lanes(&active_shas);
        let parent_lanes = parent_lanes(
            lane,
            is_top_first_parent,
            &commit.parent_shas,
            &active_shas,
            &top_first_parent_history,
        );
        let connector_lanes = connector_lanes(lane, &parent_lanes);
        let connectors = connectors(lane, &parent_lanes);

        let mut next_active_shas = active_shas.clone();
        let mut next_active_colors = active_colors.clone();
        update_active_lanes(
            &mut next_active_shas,
            &mut next_active_colors,
            lane,
            &commit.parent_shas,
            &parent_lanes,
            &mut next_color,
        );
        let outgoing_lanes = occupied_lanes(&next_active_shas);
        let lane_count = occupied_lane_count(&active_shas)
            .max(occupied_lane_count(&next_active_shas))
            .max(lane + 1)
            .max(
                parent_lanes
                    .iter()
                    .copied()
                    .max()
                    .map_or(0, |lane| lane + 1),
            );
        let lane_colors = row_lane_colors(lane_count, &active_colors, &next_active_colors);

        rows.push(GraphRow {
            sha: commit.sha.clone(),
            lane,
            lane_count,
            active_lanes,
            incoming_lanes,
            outgoing_lanes,
            parent_lanes,
            connector_lanes,
            connectors,
            lane_colors,
        });

        active_shas = next_active_shas;
        active_colors = next_active_colors;
    }

    rows
}

fn top_first_parent_history(commits: &[GraphCommit]) -> Vec<String> {
    let mut history = Vec::new();
    let mut next_sha = commits.first().map(|commit| commit.sha.clone());

    while let Some(sha) = next_sha {
        if contains_sha(&history, &sha) {
            break;
        }

        let Some(commit) = commits.iter().find(|commit| commit.sha == sha) else {
            break;
        };
        next_sha = commit
            .parent_shas
            .first()
            .filter(|parent_sha| commits.iter().any(|commit| commit.sha == **parent_sha))
            .cloned();
        history.push(sha);
    }

    history
}

fn commit_lane(
    commit: &GraphCommit,
    is_top_first_parent: bool,
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    next_color: &mut usize,
) -> usize {
    if is_top_first_parent {
        ensure_commit_lane(
            active_shas,
            active_colors,
            0,
            commit.sha.clone(),
            next_color,
        );
        return 0;
    }

    if let Some(lane) = find_lane(active_shas, &commit.sha) {
        return lane;
    }

    let lane = first_available_lane_at_or_after(&occupied_lanes(active_shas), 1);
    set_new_lane(
        active_shas,
        active_colors,
        lane,
        commit.sha.clone(),
        next_color,
    );
    lane
}

fn occupied_lanes(active_shas: &[Option<String>]) -> Vec<usize> {
    active_shas
        .iter()
        .enumerate()
        .filter_map(|(lane, sha)| sha.as_ref().map(|_| lane))
        .collect()
}

fn occupied_lane_count(active_shas: &[Option<String>]) -> usize {
    active_shas
        .iter()
        .rposition(|sha| sha.is_some())
        .map_or(0, |lane| lane + 1)
}

fn parent_lanes(
    commit_lane: usize,
    commit_is_top_first_parent: bool,
    parent_shas: &[String],
    active_shas: &[Option<String>],
    top_first_parent_history: &[String],
) -> Vec<usize> {
    let mut lanes = Vec::with_capacity(parent_shas.len());
    let mut reserved_lanes = occupied_lanes(active_shas);

    for (index, parent_sha) in parent_shas.iter().enumerate() {
        let existing_lane = find_lane(active_shas, parent_sha)
            .filter(|existing_lane| *existing_lane != commit_lane);
        let lane = if index == 0
            && commit_is_top_first_parent
            && contains_sha(top_first_parent_history, parent_sha)
        {
            commit_lane
        } else if let Some(existing_lane) = existing_lane {
            existing_lane
        } else if index == 0 {
            commit_lane
        } else {
            let lane = first_available_lane_at_or_after(&reserved_lanes, commit_lane + 1);
            reserved_lanes.push(lane);
            lane
        };
        lanes.push(lane);
    }

    lanes
}

fn first_available_lane_at_or_after(reserved_lanes: &[usize], start_lane: usize) -> usize {
    let mut lane = start_lane;
    while reserved_lanes.contains(&lane) {
        lane += 1;
    }
    lane
}

fn contains_sha(shas: &[String], sha: &str) -> bool {
    shas.iter().any(|candidate| candidate == sha)
}

fn find_lane(active_shas: &[Option<String>], sha: &str) -> Option<usize> {
    active_shas
        .iter()
        .position(|active_sha| active_sha.as_deref() == Some(sha))
}

fn connector_lanes(commit_lane: usize, parent_lanes: &[usize]) -> Vec<usize> {
    let Some(first_parent_lane) = parent_lanes.first() else {
        return Vec::new();
    };

    let mut min_lane = commit_lane.min(*first_parent_lane);
    let mut max_lane = commit_lane.max(*first_parent_lane);
    for parent_lane in parent_lanes.iter().skip(1) {
        min_lane = min_lane.min(*parent_lane);
        max_lane = max_lane.max(*parent_lane);
    }

    (min_lane..=max_lane).collect()
}

fn connectors(commit_lane: usize, parent_lanes: &[usize]) -> Vec<GraphConnector> {
    parent_lanes
        .iter()
        .copied()
        .map(|parent_lane| {
            let kind = match parent_lane.cmp(&commit_lane) {
                std::cmp::Ordering::Less => GraphConnectorKind::MergeIn,
                std::cmp::Ordering::Equal => GraphConnectorKind::Straight,
                std::cmp::Ordering::Greater => GraphConnectorKind::BranchOut,
            };

            GraphConnector {
                from_lane: commit_lane,
                to_lane: parent_lane,
                kind,
            }
        })
        .collect()
}

fn row_lane_colors(
    lane_count: usize,
    active_colors: &[Option<usize>],
    next_active_colors: &[Option<usize>],
) -> Vec<Option<usize>> {
    let mut lane_colors = vec![None; lane_count];

    for (lane, color) in next_active_colors.iter().enumerate().take(lane_count) {
        if let Some(color) = color {
            lane_colors[lane] = Some(*color);
        }
    }

    for (lane, color) in active_colors.iter().enumerate().take(lane_count) {
        if let Some(color) = color {
            lane_colors[lane] = Some(*color);
        }
    }

    lane_colors
}

fn update_active_lanes(
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    commit_lane: usize,
    parent_shas: &[String],
    parent_lanes: &[usize],
    next_color: &mut usize,
) {
    let Some(first_parent) = parent_shas.first() else {
        clear_lane(active_shas, active_colors, commit_lane);
        trim_empty_lanes(active_shas, active_colors);
        return;
    };

    let first_parent_lane = parent_lanes[0];
    if first_parent_lane == commit_lane {
        if let Some(existing_lane) =
            find_lane(active_shas, first_parent).filter(|lane| *lane != commit_lane)
        {
            clear_lane(active_shas, active_colors, existing_lane);
        }
        active_shas[commit_lane] = Some(first_parent.clone());
    } else if find_lane(active_shas, first_parent) == Some(first_parent_lane) {
        clear_lane(active_shas, active_colors, commit_lane);
    } else {
        clear_lane(active_shas, active_colors, commit_lane);
        set_new_lane(
            active_shas,
            active_colors,
            first_parent_lane,
            first_parent.clone(),
            next_color,
        );
    }

    for (parent_sha, parent_lane) in parent_shas.iter().skip(1).zip(parent_lanes.iter().skip(1)) {
        if find_lane(active_shas, parent_sha).is_some() {
            continue;
        }

        set_new_lane(
            active_shas,
            active_colors,
            *parent_lane,
            parent_sha.clone(),
            next_color,
        );
    }

    trim_empty_lanes(active_shas, active_colors);
}

fn clear_lane(
    active_shas: &mut [Option<String>],
    active_colors: &mut [Option<usize>],
    lane: usize,
) {
    if let Some(active_sha) = active_shas.get_mut(lane) {
        *active_sha = None;
    }
    if let Some(active_color) = active_colors.get_mut(lane) {
        *active_color = None;
    }
}

fn ensure_commit_lane(
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    lane: usize,
    sha: String,
    next_color: &mut usize,
) {
    let existing_lane = find_lane(active_shas, &sha);
    let existing_color = existing_lane
        .and_then(|lane| active_colors.get(lane))
        .and_then(|color| *color);

    if let Some(existing_lane) = existing_lane.filter(|existing_lane| *existing_lane != lane) {
        clear_lane(active_shas, active_colors, existing_lane);
    }

    while active_shas.len() <= lane {
        active_shas.push(None);
        active_colors.push(None);
    }

    let color = active_colors[lane].or(existing_color).unwrap_or_else(|| {
        let color = *next_color;
        *next_color += 1;
        color
    });

    active_shas[lane] = Some(sha);
    active_colors[lane] = Some(color);
}

fn set_new_lane(
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    lane: usize,
    sha: String,
    next_color: &mut usize,
) {
    set_lane(active_shas, active_colors, lane, sha, *next_color);
    *next_color += 1;
}

fn set_lane(
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    lane: usize,
    sha: String,
    color: usize,
) {
    while active_shas.len() <= lane {
        active_shas.push(None);
        active_colors.push(None);
    }

    active_shas[lane] = Some(sha);
    active_colors[lane] = Some(color);
}

fn trim_empty_lanes(active_shas: &mut Vec<Option<String>>, active_colors: &mut Vec<Option<usize>>) {
    let len = occupied_lane_count(active_shas);
    active_shas.truncate(len);
    active_colors.truncate(len);
}

#[cfg(test)]
mod tests {
    use super::{layout_graph, parent_lanes, GraphCommit, GraphConnector, GraphConnectorKind};

    fn commit(sha: &str, parent_shas: &[&str]) -> GraphCommit {
        GraphCommit {
            sha: sha.to_string(),
            parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
        }
    }

    fn shas(shas: &[&str]) -> Vec<String> {
        shas.iter().map(|sha| sha.to_string()).collect()
    }

    #[test]
    fn linear_history_uses_one_lane() {
        let rows = layout_graph(&[
            commit("tip", &["middle"]),
            commit("middle", &["root"]),
            commit("root", &[]),
        ]);

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].sha, "tip");
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].lane_count, 1);
        assert_eq!(rows[0].active_lanes, vec![0]);
        assert_eq!(rows[0].incoming_lanes, Vec::<usize>::new());
        assert_eq!(rows[0].outgoing_lanes, vec![0]);
        assert_eq!(rows[0].parent_lanes, vec![0]);
        assert_eq!(rows[0].connector_lanes, vec![0]);
        assert_eq!(
            rows[0].connectors,
            vec![GraphConnector {
                from_lane: 0,
                to_lane: 0,
                kind: GraphConnectorKind::Straight,
            }]
        );
        assert_eq!(rows[0].lane_colors, vec![Some(0)]);
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[1].lane_count, 1);
        assert_eq!(rows[1].incoming_lanes, vec![0]);
        assert_eq!(rows[1].outgoing_lanes, vec![0]);
        assert_eq!(rows[1].lane_colors, vec![Some(0)]);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].incoming_lanes, vec![0]);
        assert_eq!(rows[2].outgoing_lanes, Vec::<usize>::new());
        assert_eq!(rows[2].parent_lanes, Vec::<usize>::new());
        assert_eq!(rows[2].connector_lanes, Vec::<usize>::new());
        assert_eq!(rows[2].lane_colors, vec![Some(0)]);
    }

    #[test]
    fn merge_history_keeps_parallel_lane_until_it_rejoins() {
        let rows = layout_graph(&[
            commit("merge", &["left", "right"]),
            commit("left", &["base"]),
            commit("right", &["base"]),
            commit("base", &[]),
        ]);

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].sha, "merge");
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].lane_count, 2);
        assert_eq!(rows[0].active_lanes, vec![0]);
        assert_eq!(rows[0].incoming_lanes, Vec::<usize>::new());
        assert_eq!(rows[0].outgoing_lanes, vec![0, 1]);
        assert_eq!(rows[0].parent_lanes, vec![0, 1]);
        assert_eq!(rows[0].connector_lanes, vec![0, 1]);
        assert_eq!(
            rows[0].connectors,
            vec![
                GraphConnector {
                    from_lane: 0,
                    to_lane: 0,
                    kind: GraphConnectorKind::Straight,
                },
                GraphConnector {
                    from_lane: 0,
                    to_lane: 1,
                    kind: GraphConnectorKind::BranchOut,
                },
            ]
        );
        assert_eq!(rows[0].lane_colors, vec![Some(0), Some(1)]);

        assert_eq!(rows[1].sha, "left");
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[1].lane_count, 2);
        assert_eq!(rows[1].active_lanes, vec![0, 1]);
        assert_eq!(rows[1].incoming_lanes, vec![0, 1]);
        assert_eq!(rows[1].outgoing_lanes, vec![0, 1]);
        assert_eq!(rows[1].parent_lanes, vec![0]);
        assert_eq!(rows[1].connector_lanes, vec![0]);
        assert_eq!(rows[1].lane_colors, vec![Some(0), Some(1)]);

        assert_eq!(rows[2].sha, "right");
        assert_eq!(rows[2].lane, 1);
        assert_eq!(rows[2].lane_count, 2);
        assert_eq!(rows[2].active_lanes, vec![0, 1]);
        assert_eq!(rows[2].incoming_lanes, vec![0, 1]);
        assert_eq!(rows[2].outgoing_lanes, vec![0]);
        assert_eq!(rows[2].parent_lanes, vec![0]);
        assert_eq!(rows[2].connector_lanes, vec![0, 1]);
        assert_eq!(
            rows[2].connectors,
            vec![GraphConnector {
                from_lane: 1,
                to_lane: 0,
                kind: GraphConnectorKind::MergeIn,
            }]
        );
        assert_eq!(rows[2].lane_colors, vec![Some(0), Some(1)]);

        assert_eq!(rows[3].sha, "base");
        assert_eq!(rows[3].lane, 0);
        assert_eq!(rows[3].lane_count, 1);
        assert_eq!(rows[3].incoming_lanes, vec![0]);
        assert_eq!(rows[3].outgoing_lanes, Vec::<usize>::new());
        assert_eq!(rows[3].parent_lanes, Vec::<usize>::new());
        assert_eq!(rows[3].connector_lanes, Vec::<usize>::new());
        assert_eq!(rows[3].lane_colors, vec![Some(0)]);
    }

    #[test]
    fn top_first_parent_history_stays_on_leftmost_lane() {
        let rows = layout_graph(&[
            commit("merge-tip", &["blue-parent", "teal-tip"]),
            commit("blue-parent", &["teal-tip"]),
            commit("teal-tip", &["teal-base", "purple-tip"]),
            commit("teal-base", &[]),
            commit("purple-tip", &["purple-base"]),
            commit("purple-base", &[]),
        ]);

        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].parent_lanes, vec![0, 1]);
        assert_eq!(rows[0].outgoing_lanes, vec![0, 1]);

        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[1].parent_lanes, vec![0]);
        assert_eq!(rows[1].outgoing_lanes, vec![0]);

        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].parent_lanes, vec![0, 1]);
        assert_eq!(rows[2].connector_lanes, vec![0, 1]);
        assert_eq!(rows[2].outgoing_lanes, vec![0, 1]);

        assert_eq!(rows[3].lane, 0);
    }

    #[test]
    fn new_side_parents_use_the_nearest_empty_lane() {
        let active_shas = vec![
            Some("tip".to_string()),
            None,
            Some("existing-side".to_string()),
        ];
        let parent_shas = shas(&["main-parent", "new-side"]);
        let top_first_parent_history = shas(&["tip", "main-parent"]);

        let lanes = parent_lanes(
            0,
            true,
            &parent_shas,
            &active_shas,
            &top_first_parent_history,
        );

        assert_eq!(lanes, vec![0, 1]);
    }

    #[test]
    fn active_branch_lane_stays_stable_when_intermediate_lane_ends() {
        let rows = layout_graph(&[
            commit("merge-tip", &["main-a", "ending-side", "stable-side"]),
            commit("main-a", &["main-base"]),
            commit("ending-side", &[]),
            commit("stable-side", &["main-base"]),
            commit("main-base", &[]),
        ]);

        assert_eq!(rows[0].parent_lanes, vec![0, 1, 2]);
        assert_eq!(rows[2].lane, 1);
        assert_eq!(rows[2].outgoing_lanes, vec![0, 2]);

        assert_eq!(rows[3].lane, 2);
        assert_eq!(rows[3].parent_lanes, vec![0]);
        assert_eq!(rows[3].connector_lanes, vec![0, 1, 2]);
        assert!(!rows[3].incoming_lanes.contains(&1));
        assert!(!rows[3].outgoing_lanes.contains(&1));
    }
}
