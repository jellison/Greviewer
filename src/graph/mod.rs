//! Commit graph layout.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub sha: String,
    pub authored_timestamp: i64,
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
    let side_branch_lanes = side_branch_lanes(commits, &top_first_parent_history);
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
            &commit.sha,
            &commit.parent_shas,
            &active_shas,
            &top_first_parent_history,
            &side_branch_lanes,
        );
        let connectors = connectors(
            lane,
            is_top_first_parent,
            &commit.sha,
            &commit.parent_shas,
            &parent_lanes,
            &active_shas,
            &top_first_parent_history,
        );
        let connector_lanes = connector_lanes(&connectors);

        let mut next_active_shas = active_shas.clone();
        let mut next_active_colors = active_colors.clone();
        update_active_lanes(
            &mut next_active_shas,
            &mut next_active_colors,
            &commit.sha,
            &commit.parent_shas,
            &parent_lanes,
            &mut next_color,
        );
        clear_shared_sibling_parent_lanes(
            &mut next_active_shas,
            &mut next_active_colors,
            lane,
            &parent_lanes,
            &connectors,
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
            )
            .max(
                connectors
                    .iter()
                    .map(|connector| connector.from_lane.max(connector.to_lane))
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

fn side_branch_lanes(
    commits: &[GraphCommit],
    top_first_parent_history: &[String],
) -> BTreeMap<String, usize> {
    let mut children_by_parent = BTreeMap::<String, Vec<(i64, String)>>::new();

    for commit in commits {
        if contains_sha(top_first_parent_history, &commit.sha) {
            continue;
        }

        let Some(parent_sha) = commit.parent_shas.first() else {
            continue;
        };
        if !contains_sha(top_first_parent_history, parent_sha) {
            continue;
        }

        children_by_parent
            .entry(parent_sha.clone())
            .or_default()
            .push((commit.authored_timestamp, commit.sha.clone()));
    }

    let mut lane_floors = BTreeMap::new();
    for children in children_by_parent.values() {
        if children.len() < 2 {
            continue;
        }

        let Some((first_timestamp, _)) = children.first() else {
            continue;
        };
        if children
            .iter()
            .all(|(authored_timestamp, _)| authored_timestamp == first_timestamp)
        {
            continue;
        }

        for (authored_timestamp, sha) in children {
            let earlier_sibling_count = children
                .iter()
                .filter(|(sibling_timestamp, _)| sibling_timestamp < authored_timestamp)
                .count();
            lane_floors.insert(sha.clone(), earlier_sibling_count + 1);
        }
    }

    lane_floors
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
    commit_sha: &str,
    parent_shas: &[String],
    active_shas: &[Option<String>],
    top_first_parent_history: &[String],
    side_branch_lanes: &BTreeMap<String, usize>,
) -> Vec<usize> {
    let mut lanes = Vec::with_capacity(parent_shas.len());
    let mut reserved_lanes = active_shas
        .iter()
        .enumerate()
        .filter_map(|(lane, sha)| {
            (sha.as_deref().is_some() && sha.as_deref() != Some(commit_sha)).then_some(lane)
        })
        .collect::<Vec<_>>();
    if !reserved_lanes.contains(&commit_lane) {
        reserved_lanes.push(commit_lane);
    }

    for (index, parent_sha) in parent_shas.iter().enumerate() {
        let preferred_parent_lane = side_branch_lanes.get(parent_sha).copied();
        let preferred_first_parent_lane = (index == 0)
            .then(|| side_branch_lanes.get(commit_sha).copied())
            .flatten()
            .filter(|preferred_lane| *preferred_lane > 1);
        let existing_lane = find_lane(active_shas, parent_sha)
            .filter(|existing_lane| *existing_lane != commit_lane);
        let lane = if index == 0
            && commit_is_top_first_parent
            && contains_sha(top_first_parent_history, parent_sha)
        {
            commit_lane
        } else if let Some(preferred_lane) = preferred_first_parent_lane.or(preferred_parent_lane) {
            if active_shas
                .get(preferred_lane)
                .and_then(|sha| sha.as_deref())
                == Some(commit_sha)
            {
                preferred_lane
            } else {
                find_lane_at_or_after(active_shas, parent_sha, preferred_lane).unwrap_or_else(
                    || {
                        let lane =
                            first_available_lane_at_or_after(&reserved_lanes, preferred_lane);
                        reserved_lanes.push(lane);
                        lane
                    },
                )
            }
        } else if index == 0 {
            if let Some(existing_lane) = existing_lane {
                existing_lane
            } else {
                commit_lane
            }
        } else if let Some(existing_lane) = existing_lane {
            existing_lane
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

fn find_lane_at_or_after(
    active_shas: &[Option<String>],
    sha: &str,
    start_lane: usize,
) -> Option<usize> {
    active_shas
        .iter()
        .enumerate()
        .skip(start_lane)
        .find_map(|(lane, active_sha)| (active_sha.as_deref() == Some(sha)).then_some(lane))
}

fn connector_lanes(connectors: &[GraphConnector]) -> Vec<usize> {
    let Some(first_connector) = connectors.first() else {
        return Vec::new();
    };

    let mut min_lane = first_connector.from_lane.min(first_connector.to_lane);
    let mut max_lane = first_connector.from_lane.max(first_connector.to_lane);
    for connector in connectors.iter().skip(1) {
        min_lane = min_lane.min(connector.from_lane.min(connector.to_lane));
        max_lane = max_lane.max(connector.from_lane.max(connector.to_lane));
    }

    (min_lane..=max_lane).collect()
}

fn connectors(
    commit_lane: usize,
    commit_is_top_first_parent: bool,
    commit_sha: &str,
    parent_shas: &[String],
    parent_lanes: &[usize],
    active_shas: &[Option<String>],
    top_first_parent_history: &[String],
) -> Vec<GraphConnector> {
    let mut connectors = parent_lanes
        .iter()
        .copied()
        .map(|parent_lane| GraphConnector {
            from_lane: commit_lane,
            to_lane: parent_lane,
            kind: connector_kind(commit_lane, parent_lane),
        })
        .collect::<Vec<_>>();

    connectors.extend(visible_sibling_parent_connectors(
        commit_lane,
        commit_is_top_first_parent,
        parent_shas,
        parent_lanes,
        active_shas,
        top_first_parent_history,
    ));
    connectors.extend(visible_current_commit_connectors(
        commit_lane,
        commit_sha,
        active_shas,
    ));

    connectors
}

fn visible_sibling_parent_connectors(
    commit_lane: usize,
    commit_is_top_first_parent: bool,
    parent_shas: &[String],
    parent_lanes: &[usize],
    active_shas: &[Option<String>],
    top_first_parent_history: &[String],
) -> Vec<GraphConnector> {
    let Some(first_parent_sha) = parent_shas.first() else {
        return Vec::new();
    };
    if commit_is_top_first_parent || !contains_sha(top_first_parent_history, first_parent_sha) {
        return Vec::new();
    }

    lanes_for_sha(active_shas, first_parent_sha)
        .into_iter()
        .filter(|lane| *lane > commit_lane && !parent_lanes.contains(lane))
        .map(|lane| GraphConnector {
            from_lane: commit_lane,
            to_lane: lane,
            kind: connector_kind(commit_lane, lane),
        })
        .collect()
}

fn visible_current_commit_connectors(
    commit_lane: usize,
    commit_sha: &str,
    active_shas: &[Option<String>],
) -> Vec<GraphConnector> {
    lanes_for_sha(active_shas, commit_sha)
        .into_iter()
        .filter(|lane| *lane != commit_lane)
        .map(|lane| GraphConnector {
            from_lane: lane,
            to_lane: commit_lane,
            kind: connector_kind(lane, commit_lane),
        })
        .collect()
}

fn connector_kind(from_lane: usize, to_lane: usize) -> GraphConnectorKind {
    match to_lane.cmp(&from_lane) {
        std::cmp::Ordering::Less => GraphConnectorKind::MergeIn,
        std::cmp::Ordering::Equal => GraphConnectorKind::Straight,
        std::cmp::Ordering::Greater => GraphConnectorKind::BranchOut,
    }
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
    commit_sha: &str,
    parent_shas: &[String],
    parent_lanes: &[usize],
    next_color: &mut usize,
) {
    let commit_color = lanes_for_sha(active_shas, commit_sha)
        .first()
        .and_then(|lane| active_colors.get(*lane))
        .and_then(|color| *color);

    if parent_shas.is_empty() {
        clear_commit_lanes(active_shas, active_colors, commit_sha);
        trim_empty_lanes(active_shas, active_colors);
        return;
    }

    for (index, (parent_sha, parent_lane)) in parent_shas.iter().zip(parent_lanes).enumerate() {
        ensure_lane_capacity(active_shas, active_colors, *parent_lane);
        if active_shas[*parent_lane].as_deref() == Some(parent_sha) {
            continue;
        }

        if active_shas[*parent_lane].as_deref() == Some(commit_sha) {
            active_shas[*parent_lane] = Some(parent_sha.clone());
        } else if index == 0 {
            if let Some(color) = commit_color {
                set_lane(
                    active_shas,
                    active_colors,
                    *parent_lane,
                    parent_sha.clone(),
                    color,
                );
            } else {
                set_new_lane(
                    active_shas,
                    active_colors,
                    *parent_lane,
                    parent_sha.clone(),
                    next_color,
                );
            }
        } else {
            set_new_lane(
                active_shas,
                active_colors,
                *parent_lane,
                parent_sha.clone(),
                next_color,
            );
        }
    }

    for lane in lanes_for_sha(active_shas, commit_sha) {
        if !parent_lanes.contains(&lane) {
            clear_lane(active_shas, active_colors, lane);
        }
    }

    trim_empty_lanes(active_shas, active_colors);
}

fn clear_shared_sibling_parent_lanes(
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    commit_lane: usize,
    parent_lanes: &[usize],
    connectors: &[GraphConnector],
) {
    for connector in connectors {
        if connector.kind == GraphConnectorKind::BranchOut
            && connector.from_lane == commit_lane
            && !parent_lanes.contains(&connector.to_lane)
        {
            clear_lane(active_shas, active_colors, connector.to_lane);
        }
    }

    trim_empty_lanes(active_shas, active_colors);
}

fn clear_commit_lanes(
    active_shas: &mut [Option<String>],
    active_colors: &mut [Option<usize>],
    sha: &str,
) {
    for lane in lanes_for_sha(active_shas, sha) {
        clear_lane(active_shas, active_colors, lane);
    }
}

fn lanes_for_sha(active_shas: &[Option<String>], sha: &str) -> Vec<usize> {
    active_shas
        .iter()
        .enumerate()
        .filter_map(|(lane, active_sha)| (active_sha.as_deref() == Some(sha)).then_some(lane))
        .collect()
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
    ensure_lane_capacity(active_shas, active_colors, lane);
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
    ensure_lane_capacity(active_shas, active_colors, lane);

    active_shas[lane] = Some(sha);
    active_colors[lane] = Some(color);
}

fn ensure_lane_capacity(
    active_shas: &mut Vec<Option<String>>,
    active_colors: &mut Vec<Option<usize>>,
    lane: usize,
) {
    while active_shas.len() <= lane {
        active_shas.push(None);
        active_colors.push(None);
    }
}

fn trim_empty_lanes(active_shas: &mut Vec<Option<String>>, active_colors: &mut Vec<Option<usize>>) {
    let len = occupied_lane_count(active_shas);
    active_shas.truncate(len);
    active_colors.truncate(len);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{layout_graph, parent_lanes, GraphCommit, GraphConnector, GraphConnectorKind};

    fn commit(sha: &str, parent_shas: &[&str]) -> GraphCommit {
        commit_at(sha, 0, parent_shas)
    }

    fn commit_at(sha: &str, authored_timestamp: i64, parent_shas: &[&str]) -> GraphCommit {
        GraphCommit {
            sha: sha.to_string(),
            authored_timestamp,
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
    fn top_first_parent_history_duplicates_existing_parent_edges_on_the_trunk() {
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
        assert_eq!(rows[1].outgoing_lanes, vec![0, 1]);

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
            "tip",
            &parent_shas,
            &active_shas,
            &top_first_parent_history,
            &BTreeMap::new(),
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

    #[test]
    fn first_parent_history_preserves_duplicate_side_edges_to_the_same_parent() {
        let rows = layout_graph(&[
            commit("merge-tip", &["main-a", "teal-tip"]),
            commit("main-a", &["teal-tip", "purple-tip"]),
            commit("teal-tip", &["main-base"]),
            commit("purple-tip", &["main-base"]),
            commit("main-base", &[]),
        ]);

        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[1].incoming_lanes, vec![0, 1]);
        assert_eq!(rows[1].outgoing_lanes, vec![0, 1, 2]);
        assert_eq!(rows[1].parent_lanes, vec![0, 2]);
        assert_eq!(rows[1].connector_lanes, vec![0, 1, 2]);
        assert!(
            rows[1].connectors.iter().any(|connector| {
                connector.from_lane == 0
                    && connector.to_lane == 0
                    && connector.kind == GraphConnectorKind::Straight
            }),
            "the first-parent edge should stay on the trunk",
        );
        assert!(
            rows[1].outgoing_lanes.contains(&1),
            "the pre-existing side edge to the same parent should remain active",
        );
        assert!(
            rows[1].connectors.iter().any(|connector| {
                connector.from_lane == 0
                    && connector.to_lane == 2
                    && connector.kind == GraphConnectorKind::BranchOut
            }),
            "the second parent should keep its independent trunk branch-out",
        );
        assert_eq!(rows[2].lane, 0);
        assert!(
            rows[2].incoming_lanes.contains(&1),
            "the duplicated side edge should merge into the shared parent commit row",
        );
    }

    #[test]
    fn successive_trunk_based_side_branches_reuse_the_nearest_lane() {
        let rows = layout_graph(&[
            commit("merge-pink", &["trunk-a", "pink-tip"]),
            commit("pink-tip", &["trunk-a"]),
            commit("trunk-a", &["trunk-b", "teal-tip"]),
            commit("teal-tip", &["trunk-b"]),
            commit("trunk-b", &["trunk-c", "purple-tip"]),
            commit("purple-tip", &["trunk-c"]),
            commit("trunk-c", &[]),
        ]);

        assert_eq!(rows[0].parent_lanes, vec![0, 1]);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].parent_lanes, vec![0]);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].parent_lanes, vec![0, 1]);
        assert_eq!(rows[3].lane, 1);
        assert_eq!(rows[3].parent_lanes, vec![0]);
        assert_eq!(rows[4].lane, 0);
        assert_eq!(rows[4].parent_lanes, vec![0, 1]);
        assert_eq!(rows[5].lane, 1);
        assert_eq!(rows[5].parent_lanes, vec![0]);
    }

    #[test]
    fn side_branch_and_trunk_merge_can_share_the_same_future_parent() {
        let rows = layout_graph(&[
            commit_at("merge-lfs", 50, &["merge-docs", "lfs-tip"]),
            commit_at("lfs-tip", 40, &["trunk-base"]),
            commit_at("merge-docs", 30, &["trunk-base", "docs-tip"]),
            commit_at("docs-tip", 20, &["trunk-base"]),
            commit_at("trunk-base", 10, &[]),
        ]);

        assert_eq!(
            rows[0].parent_lanes,
            vec![0, 2],
            "the later lfs side branch should start in its outward sibling lane",
        );
        assert_eq!(rows[0].connector_lanes, vec![0, 1, 2]);

        assert_eq!(rows[1].lane, 2);
        assert_eq!(
            rows[1].parent_lanes,
            vec![2],
            "the later side branch should stay in its outward lane until the shared parent",
        );
        assert_eq!(rows[1].outgoing_lanes, vec![0, 2]);

        assert_eq!(rows[2].lane, 0);
        assert_eq!(
            rows[2].incoming_lanes,
            vec![0, 2],
            "the lfs edge should pass the docs merge in the next outer lane",
        );
        assert_eq!(
            rows[2].parent_lanes,
            vec![0, 1],
            "the older docs branch should occupy the first side lane",
        );
        assert_eq!(
            rows[2].connector_lanes,
            vec![0, 1],
            "the docs merge row should not branch the lfs lane out directly",
        );
        assert!(
            !rows[2].connectors.iter().any(|connector| {
                connector.from_lane == 0
                    && connector.to_lane == 2
                    && connector.kind == GraphConnectorKind::BranchOut
            }),
            "the lfs side branch should not start on the docs merge row",
        );
        assert_eq!(
            rows[2].outgoing_lanes,
            vec![0, 1, 2],
            "the lfs side edge to trunk-base should stay active while merge-docs also targets trunk-base",
        );

        assert_eq!(rows[3].lane, 1);
        assert_eq!(rows[3].parent_lanes, vec![0]);
        assert_eq!(rows[3].connector_lanes, vec![0, 1, 2]);
        assert!(
            rows[3].connectors.iter().any(|connector| {
                connector.from_lane == 1
                    && connector.to_lane == 2
                    && connector.kind == GraphConnectorKind::BranchOut
            }),
            "the lfs side branch should extend from the earlier sibling branch row",
        );
        assert_eq!(
            rows[3].outgoing_lanes,
            vec![0],
            "the lfs side edge should end at the shared sibling branch row",
        );

        assert_eq!(rows[4].lane, 0);
        assert_eq!(rows[4].incoming_lanes, vec![0]);
        assert_eq!(rows[4].connector_lanes, Vec::<usize>::new());
        assert!(
            rows[4].connectors.is_empty(),
            "the shared parent row should not redraw the sibling branch merge",
        );
    }
}
