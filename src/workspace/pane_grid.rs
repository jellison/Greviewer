//! Recursive renderer for the workspace pane tree.
//!
//! Axis nodes render as ratio-weighted flex rows/columns with draggable
//! dividers; pane leaves render a tab bar above the diff content. Exactly one
//! pane is active; the others render dimmed tab bars. Behavior contract lives
//! in `docs/specs/review/workflow.md`.
//!
//! Dividers use gpui's drag primitives instead of `gpui-component`'s
//! resizable panels: `ResizableState` exposes no public way to write sizes,
//! so ratio-weighted splits (and the persisted ratios of the layout slice)
//! could not be expressed through it.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, relative, AnyElement, AppContext, Context, DragMoveEvent, Empty, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};

use super::tab_bar::DraggedTab;
use super::{AxisNode, PaneGroup, PaneId, SplitAxis, SplitDirection};
use crate::app::menu::DIFF_PANE_CONTEXT;
use crate::app::{
    App, DiffCancelSelection, DiffCopy, DiffMoveDocEnd, DiffMoveDocStart, DiffMoveDown,
    DiffMoveLeft, DiffMoveLineEnd, DiffMoveLineStart, DiffMoveRight, DiffMoveUp, DiffMoveWordLeft,
    DiffMoveWordRight, DiffSelectAll, DiffSelectDocEnd, DiffSelectDocStart, DiffSelectDown,
    DiffSelectLeft, DiffSelectLineEnd, DiffSelectLineStart, DiffSelectRight, DiffSelectUp,
    DiffSelectWordLeft, DiffSelectWordRight,
};
use crate::repo;
use crate::theme::palette;

const DIVIDER_THICKNESS: f32 = 4.;

/// Dragging the divider after child `divider` of axis node `axis_id`.
#[derive(Clone)]
pub(crate) struct DraggedDivider {
    pub axis_id: usize,
    pub divider: usize,
}

/// Invisible drag preview: divider drags give feedback by live-resizing.
pub(crate) struct EmptyDragPreview;

impl Render for EmptyDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub fn render_pane_group(
    app: &App,
    node: &PaneGroup,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    match node {
        PaneGroup::Pane(pane) => render_pane(app, *pane, repo, changeset, cx),
        PaneGroup::Axis(axis) => render_axis(app, axis, repo, changeset, cx),
    }
}

fn render_pane(
    app: &App,
    pane: PaneId,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    let scrolls = app.pane_scroll(pane, cx);

    div()
        .id(("workspace-pane", pane))
        .debug_selector(move || format!("workspace-pane-{pane}"))
        .flex()
        .flex_col()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, _event: &MouseDownEvent, _window, cx| {
                app.activate_workspace_pane(pane, cx);
            }),
        )
        .child(super::tab_bar::render_tab_bar(
            &app.workspace,
            pane,
            changeset,
            &scrolls.tab_bar,
            cx,
        ))
        .child(render_pane_content(app, pane, repo, changeset, cx))
        .into_any_element()
}

/// Fraction of the content area's width/height that counts as an edge band
/// for drag-to-split.
const EDGE_ZONE_FRACTION: f32 = 0.25;

/// A pane's diff content plus tab-drag edge zones. While a tab drag hovers
/// the left/right/top/bottom band of the content area, the corresponding
/// half of the pane highlights; dropping there splits the pane in that
/// direction with the dragged tab.
fn render_pane_content(
    app: &App,
    pane: PaneId,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    let scrolls = app.pane_scroll(pane, cx);
    let pane_is_empty = app.workspace.tabs(pane).is_empty();
    let active_path = app
        .workspace
        .active_item(pane)
        .map(|item| item.path().to_string());
    let highlight = app
        .tab_drop_zone
        .filter(|(zone_pane, _)| *zone_pane == pane)
        .map(|(_, direction)| direction)
        .filter(|_| cx.has_active_drag());

    div()
        .id(("workspace-pane-content", pane))
        .key_context(DIFF_PANE_CONTEXT)
        .track_focus(&scrolls.focus)
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .on_action(cx.listener(|app, _: &DiffMoveLeft, window, cx| {
            app.diff_motion(false, window, cx, crate::app::diff_selection::move_left);
        }))
        .on_action(cx.listener(|app, _: &DiffMoveRight, window, cx| {
            app.diff_motion(false, window, cx, crate::app::diff_selection::move_right);
        }))
        .on_action(cx.listener(|app, _: &DiffMoveUp, window, cx| {
            app.diff_vertical_motion(false, false, window, cx);
        }))
        .on_action(cx.listener(|app, _: &DiffMoveDown, window, cx| {
            app.diff_vertical_motion(false, true, window, cx);
        }))
        .on_action(cx.listener(|app, _: &DiffMoveWordLeft, window, cx| {
            app.diff_motion(
                false,
                window,
                cx,
                crate::app::diff_selection::move_word_left,
            );
        }))
        .on_action(cx.listener(|app, _: &DiffMoveWordRight, window, cx| {
            app.diff_motion(
                false,
                window,
                cx,
                crate::app::diff_selection::move_word_right,
            );
        }))
        .on_action(cx.listener(|app, _: &DiffMoveLineStart, window, cx| {
            app.diff_motion(false, window, cx, |_, point| {
                crate::app::diff_selection::line_start(point)
            });
        }))
        .on_action(cx.listener(|app, _: &DiffMoveLineEnd, window, cx| {
            app.diff_motion(false, window, cx, crate::app::diff_selection::line_end);
        }))
        .on_action(cx.listener(|app, _: &DiffMoveDocStart, window, cx| {
            app.diff_motion(false, window, cx, |content, point| {
                crate::app::diff_selection::document_start(content).unwrap_or(point)
            });
        }))
        .on_action(cx.listener(|app, _: &DiffMoveDocEnd, window, cx| {
            app.diff_motion(false, window, cx, |content, point| {
                crate::app::diff_selection::document_end(content).unwrap_or(point)
            });
        }))
        .on_action(cx.listener(|app, _: &DiffSelectLeft, window, cx| {
            app.diff_motion(true, window, cx, crate::app::diff_selection::move_left);
        }))
        .on_action(cx.listener(|app, _: &DiffSelectRight, window, cx| {
            app.diff_motion(true, window, cx, crate::app::diff_selection::move_right);
        }))
        .on_action(cx.listener(|app, _: &DiffSelectUp, window, cx| {
            app.diff_vertical_motion(true, false, window, cx);
        }))
        .on_action(cx.listener(|app, _: &DiffSelectDown, window, cx| {
            app.diff_vertical_motion(true, true, window, cx);
        }))
        .on_action(cx.listener(|app, _: &DiffSelectWordLeft, window, cx| {
            app.diff_motion(true, window, cx, crate::app::diff_selection::move_word_left);
        }))
        .on_action(cx.listener(|app, _: &DiffSelectWordRight, window, cx| {
            app.diff_motion(
                true,
                window,
                cx,
                crate::app::diff_selection::move_word_right,
            );
        }))
        .on_action(cx.listener(|app, _: &DiffSelectLineStart, window, cx| {
            app.diff_motion(true, window, cx, |_, point| {
                crate::app::diff_selection::line_start(point)
            });
        }))
        .on_action(cx.listener(|app, _: &DiffSelectLineEnd, window, cx| {
            app.diff_motion(true, window, cx, crate::app::diff_selection::line_end);
        }))
        .on_action(cx.listener(|app, _: &DiffSelectDocStart, window, cx| {
            app.diff_motion(true, window, cx, |content, point| {
                crate::app::diff_selection::document_start(content).unwrap_or(point)
            });
        }))
        .on_action(cx.listener(|app, _: &DiffSelectDocEnd, window, cx| {
            app.diff_motion(true, window, cx, |content, point| {
                crate::app::diff_selection::document_end(content).unwrap_or(point)
            });
        }))
        .on_action(cx.listener(|app, _: &DiffSelectAll, window, cx| {
            app.select_all_diff(window, cx);
        }))
        .on_action(cx.listener(|app, _: &DiffCopy, _window, cx| {
            app.copy_diff_selection(cx);
        }))
        .on_action(cx.listener(|app, _: &DiffCancelSelection, _window, cx| {
            app.cancel_diff_selection(cx);
        }))
        .on_hover(cx.listener(move |app, hovered: &bool, _window, cx| {
            if *hovered {
                if app.hovered_diff_pane != Some(pane) {
                    app.hovered_diff_pane = Some(pane);
                    cx.notify();
                }
            } else if app.hovered_diff_pane == Some(pane) {
                // Only clear a hover this pane owns; other panes manage theirs.
                app.hovered_diff_pane = None;
                cx.notify();
            }
        }))
        .when(pane_is_empty, |content| {
            // Whole-pane hover feedback for the move-into-empty-pane drop.
            content.drag_over::<DraggedTab>(|style, _drag, _window, _cx| {
                style.bg(palette().drop_target)
            })
        })
        .on_drag_move(
            cx.listener(move |app, event: &DragMoveEvent<DraggedTab>, _window, cx| {
                let bounds = event.bounds;
                let position = event.event.position;
                if !bounds.contains(&position) {
                    // Only clear a zone this pane owns; other panes manage
                    // theirs from their own listeners.
                    if app
                        .tab_drop_zone
                        .is_some_and(|(zone_pane, _)| zone_pane == pane)
                    {
                        app.set_tab_drop_zone(None, cx);
                    }
                    return;
                }
                // An empty pane has no edge zones: it has no tab row to drop
                // onto, so the whole content area accepts the move instead.
                if app.workspace.tabs(pane).is_empty() {
                    app.set_tab_drop_zone(None, cx);
                    return;
                }
                let x = (position.x - bounds.left()) / bounds.size.width;
                let y = (position.y - bounds.top()) / bounds.size.height;
                let zone = if x < EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Left)
                } else if x > 1. - EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Right)
                } else if y < EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Up)
                } else if y > 1. - EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Down)
                } else {
                    None
                };
                app.set_tab_drop_zone(zone.map(|direction| (pane, direction)), cx);
            }),
        )
        .on_drop(cx.listener(move |app, drag: &DraggedTab, _window, cx| {
            let zone = app
                .tab_drop_zone
                .filter(|(zone_pane, _)| *zone_pane == pane);
            if let Some((_, direction)) = zone {
                app.split_workspace_pane_with_tab(pane, direction, drag.pane, drag.index, cx);
            } else if app.workspace.tabs(pane).is_empty() {
                // Empty panes have no tab row; dropping anywhere in the
                // content moves the tab here.
                app.move_workspace_tab(drag.pane, drag.index, pane, 0, cx);
            } else {
                app.set_tab_drop_zone(None, cx);
            }
        }))
        .child(app.render_file_detail(
            repo,
            changeset,
            active_path.as_deref(),
            crate::app::PaneRenderContext {
                pane,
                scroll: &scrolls.diff,
                hovered: app.hovered_diff_pane == Some(pane),
            },
            cx,
        ))
        .when_some(highlight, |content, direction| {
            let selector = format!("workspace-drop-half-{pane}");
            content.child(
                div()
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .bg(palette().accent)
                    .opacity(0.18)
                    .map(|half| match direction {
                        SplitDirection::Left => half.left_0().top_0().bottom_0().w(relative(0.5)),
                        SplitDirection::Right => half.right_0().top_0().bottom_0().w(relative(0.5)),
                        SplitDirection::Up => half.top_0().left_0().right_0().h(relative(0.5)),
                        SplitDirection::Down => half.bottom_0().left_0().right_0().h(relative(0.5)),
                    }),
            )
        })
        .into_any_element()
}

fn render_axis(
    app: &App,
    axis: &AxisNode,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    let axis_id = axis.id;
    let direction = axis.axis;

    let mut container = div()
        .id(SharedString::from(format!("workspace-axis-{axis_id}")))
        .debug_selector(move || format!("workspace-axis-{axis_id}"))
        .flex()
        .size_full()
        .min_w_0()
        .min_h_0()
        .map(|container| match direction {
            SplitAxis::Horizontal => container.flex_row(),
            SplitAxis::Vertical => container.flex_col(),
        })
        .on_drag_move(cx.listener(
            move |app, event: &DragMoveEvent<DraggedDivider>, _window, cx| {
                let (event_axis_id, divider) = {
                    let drag = event.drag(cx);
                    (drag.axis_id, drag.divider)
                };
                if event_axis_id != axis_id {
                    return;
                }
                let bounds = event.bounds;
                let position = event.event.position;
                let fraction = match direction {
                    SplitAxis::Horizontal => (position.x - bounds.left()) / bounds.size.width,
                    SplitAxis::Vertical => (position.y - bounds.top()) / bounds.size.height,
                };
                app.resize_workspace_divider(axis_id, divider, fraction, cx);
            },
        ));

    for (index, child) in axis.children.iter().enumerate() {
        if index > 0 {
            container = container.child(render_divider(axis_id, direction, index - 1));
        }
        container = container.child(
            div()
                .flex()
                .min_w_0()
                .min_h_0()
                .flex_basis(relative(axis.ratios[index]))
                .child(render_pane_group(app, child, repo, changeset, cx)),
        );
    }

    container.into_any_element()
}

fn render_divider(axis_id: usize, direction: SplitAxis, divider: usize) -> AnyElement {
    let selector = format!("workspace-divider-{axis_id}-{divider}");
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .flex_none()
        .map(|handle| match direction {
            SplitAxis::Horizontal => handle.w(px(DIVIDER_THICKNESS)).h_full().cursor_col_resize(),
            SplitAxis::Vertical => handle.h(px(DIVIDER_THICKNESS)).w_full().cursor_row_resize(),
        })
        .bg(palette().border)
        .hover(|handle| handle.bg(palette().accent))
        .on_drag(
            DraggedDivider { axis_id, divider },
            |_drag, _offset, _window, cx| cx.new(|_| EmptyDragPreview),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use crate::workspace::test_util::{
        alpha_row_bounds, click_file_row, open_changeset, simulate_double_click, simulate_drag,
    };
    use crate::workspace::PaneGroup;
    use gpui::{point, px, Modifiers, MouseButton, Point, TestAppContext};

    #[gpui::test]
    async fn dragging_a_tab_within_its_strip_reorders(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // Pin alpha, pin beta: strip order [alpha, beta].
        let row = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row.center());
        let row = visual.debug_bounds("changed-file-row-1").expect("beta row");
        simulate_double_click(&mut visual, row.center());
        cx.run_until_parked();

        let tab0 = visual.debug_bounds("workspace-tab-0-0").expect("tab 0");
        let tab1 = visual.debug_bounds("workspace-tab-0-1").expect("tab 1");
        // Drop beta onto alpha: insert before index 0.
        simulate_drag(&mut visual, tab1.center(), tab0.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let paths: Vec<String> = app
                    .workspace
                    .tabs(0)
                    .iter()
                    .map(|tab| tab.path().to_string())
                    .collect();
                assert_eq!(paths, ["nested/beta.txt", "alpha.txt"]);
            })
            .expect("read reordered tabs");
    }

    #[gpui::test]
    async fn dragging_a_tab_to_another_pane_moves_it_pinned(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // Pin alpha in pane 0 and split: pane 1 opens with a copy of alpha.
        let row = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row.center());
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        // Back in pane 0, open beta as a preview, then drag it onto pane 1's
        // tab: it inserts there, pinned.
        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        visual.simulate_click(pane0.center(), Modifiers::none());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();
        let tab = visual.debug_bounds("workspace-tab-0-1").expect("beta tab");
        let target = visual
            .debug_bounds("workspace-tab-1-0")
            .expect("pane 1's cloned tab");
        simulate_drag(&mut visual, tab.center(), target.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1, "beta left pane 0");
                assert_eq!(app.workspace.tabs(1).len(), 2, "beta joined pane 1");
                assert!(
                    !app.workspace.is_preview(1, 0),
                    "moved preview arrives pinned"
                );
                assert_eq!(app.workspace.active_pane(), 1);
                assert_eq!(
                    app.workspace
                        .active_item(1)
                        .map(|item| item.path().to_string()),
                    Some("nested/beta.txt".to_string()),
                );
            })
            .expect("read moved tab");
    }

    #[gpui::test]
    async fn dropping_into_an_empty_pane_moves_the_tab(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_changeset(cx);
        // Keyboard-split the empty pane 0: both panes are empty, pane 1 is
        // active. Opening beta lands it in pane 1.
        visual.simulate_keystrokes("cmd-k right");
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Drag beta anywhere into empty pane 0: the tab moves there (pinned)
        // and the emptied pane 1 collapses.
        let tab = visual.debug_bounds("workspace-tab-1-0").expect("beta tab");
        let target = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        simulate_drag(&mut visual, tab.center(), target.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [0], "emptied pane collapsed");
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert!(
                    !app.workspace.is_preview(0, 0),
                    "moved preview arrives pinned"
                );
                assert_eq!(app.workspace.active_pane(), 0);
            })
            .expect("read moved tab");
    }

    #[gpui::test]
    async fn dragging_a_tab_to_an_edge_zone_splits_the_pane(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        let row = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Drag beta to the right edge band of pane 0's content.
        let tab = visual.debug_bounds("workspace-tab-0-1").expect("beta tab");
        let pane = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        let target = Point {
            x: pane.right() - px(10.),
            y: pane.center().y,
        };
        simulate_drag(&mut visual, tab.center(), target);
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids().len(), 2, "edge drop split");
                let new_pane = app.workspace.pane_ids()[1];
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert_eq!(app.workspace.tabs(new_pane).len(), 1);
                assert!(!app.workspace.is_preview(new_pane, 0));
                assert_eq!(app.workspace.active_pane(), new_pane);
            })
            .expect("read split-by-drop");
    }

    #[gpui::test]
    async fn dragging_the_last_tab_out_collapses_the_pane(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // One pinned tab in pane 0; the split copies it into pane 1.
        let row = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row.center());
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        // Drag pane 0's only tab onto pane 1, which already holds the same
        // file: the drop merges and the emptied pane 0 collapses.
        let tab = visual.debug_bounds("workspace-tab-0-0").expect("alpha tab");
        let target = visual
            .debug_bounds("workspace-tab-1-0")
            .expect("pane 1's cloned tab");
        simulate_drag(&mut visual, tab.center(), target.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [1], "source pane collapsed");
                assert_eq!(app.workspace.tabs(1).len(), 1, "merge left no duplicate");
            })
            .expect("read collapsed layout");
    }

    #[gpui::test]
    async fn split_right_control_adds_a_pane_to_the_right(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        // The tab row (and its split controls) appears once a tab is open.
        click_file_row(&mut visual, 0);
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split-right control renders");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        let pane1 = visual.debug_bounds("workspace-pane-1").expect("pane 1");
        assert!(
            pane1.left() >= pane0.right(),
            "new pane sits to the right of the source"
        );
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [0, 1]);
                assert_eq!(app.workspace.active_pane(), 1, "new pane is active");
                assert_eq!(
                    app.workspace
                        .active_item(1)
                        .map(|item| item.path().to_string()),
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    "new pane opens with the source pane's file"
                );
                assert!(
                    app.workspace.is_preview(1, 0),
                    "preview source splits as preview"
                );
            })
            .expect("read workspace state");
        visual
            .debug_bounds("workspace-tab-1-0")
            .expect("the copied tab renders in the new pane");
    }

    #[gpui::test]
    async fn split_down_control_adds_a_pane_below(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        click_file_row(&mut visual, 0);
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-down-0")
            .expect("split-down control renders");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        let pane1 = visual.debug_bounds("workspace-pane-1").expect("pane 1");
        assert!(
            pane1.top() >= pane0.bottom(),
            "new pane sits below the source"
        );
        window
            .read_with(cx, |app, _cx| assert_eq!(app.workspace.active_pane(), 1))
            .expect("read active pane");
    }

    #[gpui::test]
    async fn tree_clicks_open_in_the_active_pane_and_clicks_activate_panes(
        cx: &mut TestAppContext,
    ) {
        let (_dir, window, mut visual) = open_changeset(cx);

        // Alpha opens in pane 0; the split makes pane 1 active, so the next
        // tree click lands there.
        click_file_row(&mut visual, 0);
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        visual
            .debug_bounds("workspace-tab-1-0")
            .expect("tab opened in pane 1");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert_eq!(app.workspace.tabs(1).len(), 1);
            })
            .expect("read tab placement");

        // Clicking inside pane 0 activates it; the next open lands there.
        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        visual.simulate_click(pane0.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| assert_eq!(app.workspace.active_pane(), 0))
            .expect("read activation");
        click_file_row(&mut visual, 1);
        cx.run_until_parked();
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1, "preview replaced in place");
                assert_eq!(
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("nested/beta.txt".to_string()),
                    "open routed to pane 0"
                );
            })
            .expect("read routed open");
    }

    #[gpui::test]
    async fn closing_a_panes_last_tab_collapses_the_pane(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // Pin alpha in pane 0 and split right: pane 1 opens with a copy.
        let row = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row.center());
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        // Middle-click pane 1's only tab: the tab closes and so does pane 1.
        let tab = visual
            .debug_bounds("workspace-tab-1-0")
            .expect("pane 1's cloned tab");
        visual.simulate_mouse_down(tab.center(), MouseButton::Middle, Modifiers::none());
        visual.simulate_mouse_up(tab.center(), MouseButton::Middle, Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.workspace.pane_ids(),
                    [0],
                    "closing the last tab closes the pane"
                );
                assert_eq!(app.workspace.active_pane(), 0);
                assert_eq!(app.workspace.tabs(0).len(), 1, "pane 0 is untouched");
            })
            .expect("read collapsed layout");
    }

    #[gpui::test]
    async fn dragging_the_divider_resizes_the_panes(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        click_file_row(&mut visual, 0);
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        let divider = visual
            .debug_bounds("workspace-divider-2-0")
            .expect("divider renders (axis id 2)");
        let start = divider.center();
        let end = start + point(px(80.), px(0.));
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(
            start + point(px(4.), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let PaneGroup::Axis(axis) = app.workspace.layout() else {
                    panic!("expected an axis root");
                };
                assert!(
                    axis.ratios[0] > 0.5,
                    "left pane grew: ratios {:?}",
                    axis.ratios
                );
            })
            .expect("read resized ratios");
    }

    #[gpui::test]
    async fn keyboard_drives_tabs_and_panes(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_changeset(cx);

        // Two tabs in pane 0: pin alpha.txt, preview nested/beta.txt.
        let row = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Ctrl+Tab cycles forward (wraps from index 1 back to 0).
        visual.simulate_keystrokes("ctrl-tab");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_index(0), Some(0));
            })
            .expect("ctrl-tab cycles forward");
        visual.simulate_keystrokes("ctrl-shift-tab");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_index(0), Some(1));
            })
            .expect("ctrl-shift-tab cycles back");

        // Cmd+W closes the active tab.
        visual.simulate_keystrokes("cmd-w");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1);
            })
            .expect("cmd-w closes the active tab");

        // Cmd+K right splits; Cmd+K w closes the new pane again.
        visual.simulate_keystrokes("cmd-k right");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids().len(), 2);
                assert_eq!(app.workspace.active_pane(), 1);
            })
            .expect("cmd-k right splits");
        visual.simulate_keystrokes("cmd-k w");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [0]);
                assert_eq!(app.workspace.active_pane(), 0);
            })
            .expect("cmd-k w closes the pane");
    }

    #[gpui::test]
    async fn cmd_k_down_splits_vertically(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_changeset(cx);
        visual.simulate_keystrokes("cmd-k down");
        window
            .read_with(cx, |app, _cx| {
                use crate::workspace::SplitAxis;
                let PaneGroup::Axis(axis) = app.workspace.layout() else {
                    panic!("expected an axis root");
                };
                assert_eq!(axis.axis, SplitAxis::Vertical);
            })
            .expect("cmd-k down splits vertically");
    }

    #[gpui::test]
    async fn inactive_pane_tab_bar_is_dimmed_but_clickable(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        click_file_row(&mut visual, 0);
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        // Pane 1 is active; pane 0's bar still renders (dimmed via opacity,
        // which bounds cannot observe) and stays clickable: clicking its tab
        // re-activates pane 0.
        let tab = visual
            .debug_bounds("workspace-tab-0-0")
            .expect("pane 0 tab still renders");
        visual.simulate_click(tab.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.workspace.active_pane(),
                    0,
                    "clicking a tab activates its pane"
                );
            })
            .expect("read activation");
    }
}
