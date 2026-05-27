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
    let mut active_shas = Vec::<String>::new();
    let mut active_colors = Vec::<usize>::new();
    let mut next_color = 0;
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let incoming_lanes = (0..active_shas.len()).collect::<Vec<_>>();
        let lane = match active_shas.iter().position(|sha| sha == &commit.sha) {
            Some(lane) => lane,
            None => {
                active_shas.push(commit.sha.clone());
                active_colors.push(next_color);
                next_color += 1;
                active_shas.len() - 1
            }
        };

        let active_lanes = (0..active_shas.len()).collect::<Vec<_>>();
        let parent_lanes = parent_lanes(lane, &commit.parent_shas, &active_shas);
        let connector_lanes = connector_lanes(lane, &parent_lanes);
        let connectors = connectors(lane, &parent_lanes);
        let lane_count = active_shas.len().max(lane + 1).max(
            parent_lanes
                .iter()
                .copied()
                .max()
                .map_or(0, |lane| lane + 1),
        );

        let mut next_active_shas = active_shas.clone();
        let mut next_active_colors = active_colors.clone();
        update_active_lanes(
            &mut next_active_shas,
            &mut next_active_colors,
            lane,
            &commit.parent_shas,
            &mut next_color,
        );
        let outgoing_lanes = (0..next_active_shas.len()).collect::<Vec<_>>();
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

fn parent_lanes(commit_lane: usize, parent_shas: &[String], active_shas: &[String]) -> Vec<usize> {
    let mut lanes = Vec::with_capacity(parent_shas.len());
    let mut next_new_lane = active_shas.len();

    for (index, parent_sha) in parent_shas.iter().enumerate() {
        let existing_lane = active_shas
            .iter()
            .position(|active_sha| active_sha == parent_sha)
            .filter(|existing_lane| *existing_lane != commit_lane);
        let lane = if let Some(existing_lane) = existing_lane {
            existing_lane
        } else if index == 0 {
            commit_lane
        } else {
            let lane = next_new_lane;
            next_new_lane += 1;
            lane
        };
        lanes.push(lane);
    }

    lanes
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
    active_colors: &[usize],
    next_active_colors: &[usize],
) -> Vec<Option<usize>> {
    let mut lane_colors = vec![None; lane_count];

    for (lane, color) in next_active_colors.iter().enumerate().take(lane_count) {
        lane_colors[lane] = Some(*color);
    }

    for (lane, color) in active_colors.iter().enumerate().take(lane_count) {
        lane_colors[lane] = Some(*color);
    }

    lane_colors
}

fn update_active_lanes(
    active_shas: &mut Vec<String>,
    active_colors: &mut Vec<usize>,
    commit_lane: usize,
    parent_shas: &[String],
    next_color: &mut usize,
) {
    let Some(first_parent) = parent_shas.first() else {
        active_shas.remove(commit_lane);
        active_colors.remove(commit_lane);
        return;
    };

    if active_shas
        .iter()
        .enumerate()
        .any(|(lane, active_sha)| lane != commit_lane && active_sha == first_parent)
    {
        active_shas.remove(commit_lane);
        active_colors.remove(commit_lane);
    } else {
        active_shas[commit_lane] = first_parent.clone();
    }

    let mut insert_lane = commit_lane + 1;
    for parent_sha in parent_shas.iter().skip(1) {
        if active_shas
            .iter()
            .any(|active_sha| active_sha == parent_sha)
        {
            continue;
        }

        let lane = insert_lane.min(active_shas.len());
        active_shas.insert(lane, parent_sha.clone());
        active_colors.insert(lane, *next_color);
        *next_color += 1;
        insert_lane = lane + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{layout_graph, GraphCommit, GraphConnector, GraphConnectorKind};

    fn commit(sha: &str, parent_shas: &[&str]) -> GraphCommit {
        GraphCommit {
            sha: sha.to_string(),
            parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
        }
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
}
