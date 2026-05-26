//! Commit graph layout.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub sha: String,
    pub parent_shas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub sha: String,
    pub lane: usize,
    pub lane_count: usize,
    pub active_lanes: Vec<usize>,
    pub parent_lanes: Vec<usize>,
}

pub fn layout_graph(commits: &[GraphCommit]) -> Vec<GraphRow> {
    let mut active_shas = Vec::<String>::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let lane = match active_shas.iter().position(|sha| sha == &commit.sha) {
            Some(lane) => lane,
            None => {
                active_shas.push(commit.sha.clone());
                active_shas.len() - 1
            }
        };

        let active_lanes = (0..active_shas.len()).collect::<Vec<_>>();
        let parent_lanes = parent_lanes(lane, &commit.parent_shas, &active_shas);
        let lane_count = active_shas.len().max(lane + 1).max(
            parent_lanes
                .iter()
                .copied()
                .max()
                .map_or(0, |lane| lane + 1),
        );

        rows.push(GraphRow {
            sha: commit.sha.clone(),
            lane,
            lane_count,
            active_lanes,
            parent_lanes,
        });

        update_active_lanes(&mut active_shas, lane, &commit.parent_shas);
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

fn update_active_lanes(active_shas: &mut Vec<String>, commit_lane: usize, parent_shas: &[String]) {
    let Some(first_parent) = parent_shas.first() else {
        active_shas.remove(commit_lane);
        return;
    };

    if active_shas
        .iter()
        .enumerate()
        .any(|(lane, active_sha)| lane != commit_lane && active_sha == first_parent)
    {
        active_shas.remove(commit_lane);
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
        insert_lane = lane + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{layout_graph, GraphCommit};

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
        assert_eq!(rows[0].parent_lanes, vec![0]);
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[1].lane_count, 1);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].parent_lanes, Vec::<usize>::new());
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
        assert_eq!(rows[0].parent_lanes, vec![0, 1]);

        assert_eq!(rows[1].sha, "left");
        assert_eq!(rows[1].lane, 0);
        assert_eq!(rows[1].lane_count, 2);
        assert_eq!(rows[1].active_lanes, vec![0, 1]);
        assert_eq!(rows[1].parent_lanes, vec![0]);

        assert_eq!(rows[2].sha, "right");
        assert_eq!(rows[2].lane, 1);
        assert_eq!(rows[2].lane_count, 2);
        assert_eq!(rows[2].active_lanes, vec![0, 1]);
        assert_eq!(rows[2].parent_lanes, vec![0]);

        assert_eq!(rows[3].sha, "base");
        assert_eq!(rows[3].lane, 0);
        assert_eq!(rows[3].lane_count, 1);
        assert_eq!(rows[3].parent_lanes, Vec::<usize>::new());
    }
}
