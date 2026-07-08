//! File-diff rendering: prepared-diff construction, side-by-side and
//! single-side row building, syntax and word-level highlight attachment, and
//! per-line rendering. Extracted from `app.rs`; see docs/adr/0002-project-layout.md
//! and docs/specs covering the diff surface.

use super::*;

use crate::theme::palette;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Shared per-soft-wrapped-row code-cell bounds, keyed by `(side selector, row
/// index)`. Written by the per-row canvas in `render_wrapped_row` and read back
/// to map a pointer or caret column to a wrapped visual position and to size the
/// wrap width. Held behind `Rc<RefCell<..>>` so the render tree, the mouse
/// listeners, and the pane's scroll state all share one map.
pub(crate) type WrapRowOrigins = Rc<RefCell<HashMap<(&'static str, usize), Bounds<Pixels>>>>;

/// Everything one diff side needs to paint and mutate the selection. Built
/// per render from the caller's pane/key/selection/focus; cheap to clone
/// (an `Entity` handle plus `Rc`/`ScrollHandle` clones only).
///
/// Built once per tab via `App::diff_selection_context` with `side` and
/// `content` left as placeholders, then specialized per side with
/// `clone_for_side` — that also filters `selection` down to the one
/// belonging to that side, since a selection lives on exactly one side.
#[derive(Clone)]
pub(crate) struct DiffSelectionContext {
    pub(crate) pane: crate::workspace::PaneId,
    pub(crate) key: String,
    pub(crate) side: repo::DiffSide,
    pub(crate) content: diff_selection::DiffSideContent,
    pub(crate) selection: Option<diff_selection::DiffSelection>,
    /// Keyboard focus for the pane this diff belongs to; read per row (where
    /// `window` is available, inside the `uniform_list` item builder) to
    /// decide whether the caret paints.
    pub(crate) focus: FocusHandle,
    /// Current blink phase, mirroring `App::caret_blink_visible`: `false`
    /// hides the caret for this render without affecting focus or selection.
    pub(crate) caret_visible: bool,
    /// Handle back to `App`, used by the mouse-down listener. A plain
    /// `on_mouse_down` closure on a nested row div only receives
    /// `&mut Window`/`&mut gpui::App`, not `Context<App>`, so a `cx.listener`
    /// built at this depth cannot be stored in a `Clone` struct — the entity
    /// handle is the standard way to reach back into app state from such a
    /// closure (see `Context::entity`).
    pub(crate) app: Entity<App>,
    pub(crate) origins: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    /// Per soft-wrapped-row code-cell bounds, keyed by `(side selector, row
    /// index)`. Written by the per-row canvas in `render_wrapped_row` and read
    /// back there (and by the drag path) to map a pointer or caret column to a
    /// wrapped visual position and to size the wrap width. Empty in nowrap
    /// mode, which uses `origins` and a fixed row height instead. Owned by
    /// `FileDiffScroll` and cleared on every file switch by its `reset`, so no
    /// entry ever outlives its file.
    pub(crate) wrap_row_origins: WrapRowOrigins,
    pub(crate) hscroll: ScrollHandle,
}

impl DiffSelectionContext {
    /// This context specialized for one side: `content` set to that side's
    /// rows, and `selection` cleared unless it belongs to that side (a
    /// selection lives on exactly one side of one tab at a time).
    fn clone_for_side(
        &self,
        side: repo::DiffSide,
        content: diff_selection::DiffSideContent,
    ) -> Self {
        Self {
            side,
            content,
            selection: self.selection.filter(|selection| selection.side == side),
            ..self.clone()
        }
    }
}

pub(crate) fn render_prepared_file_diff(
    prepared: &Rc<PreparedFileDiff>,
    scroll: &FileDiffScroll,
    hovered: bool,
    soft_wrap: bool,
    selection_ctx: Option<&DiffSelectionContext>,
) -> AnyElement {
    // Consumed by the wrap-mode rendering below; the nowrap path is unchanged.
    if soft_wrap {
        return match prepared.as_ref() {
            PreparedFileDiff::Single { side, rows, .. } => {
                render_wrapped_single(prepared, *side, rows, scroll, selection_ctx)
            }
            PreparedFileDiff::SideBySide { rows, .. } => {
                render_wrapped_side_by_side(prepared, rows, scroll, selection_ctx)
            }
            PreparedFileDiff::Binary => render_binary_diff_placeholder(),
        };
    }
    let p = palette();
    let max_line_chars = prepared.max_line_chars();
    match prepared.as_ref() {
        PreparedFileDiff::Single { side, rows, .. } => {
            let side = *side;
            let selector = match side {
                repo::DiffSide::Old => "file-diff-side-old",
                repo::DiffSide::New => "file-diff-side-new",
            };
            let cells = rows
                .iter()
                .map(|row| match side {
                    repo::DiffSide::Old => row.old.clone(),
                    repo::DiffSide::New => row.new.clone(),
                })
                .collect::<Vec<_>>();

            let ctx = selection_ctx.map(|ctx| {
                ctx.clone_for_side(
                    side,
                    diff_selection::DiffSideContent::Prepared {
                        diff: prepared.clone(),
                        side,
                    },
                )
            });

            render_file_diff_side(
                selector,
                cells,
                scroll.handle_for(side).clone(),
                scroll.hscroll.clone(),
                max_line_chars,
                hovered,
                ctx,
            )
            .into_any_element()
        }
        PreparedFileDiff::SideBySide { rows, .. } => {
            let old_cells = rows.iter().map(|row| row.old.clone()).collect::<Vec<_>>();
            let new_cells = rows.iter().map(|row| row.new.clone()).collect::<Vec<_>>();

            let old_ctx = selection_ctx.map(|ctx| {
                ctx.clone_for_side(
                    repo::DiffSide::Old,
                    diff_selection::DiffSideContent::Prepared {
                        diff: prepared.clone(),
                        side: repo::DiffSide::Old,
                    },
                )
            });
            let new_ctx = selection_ctx.map(|ctx| {
                ctx.clone_for_side(
                    repo::DiffSide::New,
                    diff_selection::DiffSideContent::Prepared {
                        diff: prepared.clone(),
                        side: repo::DiffSide::New,
                    },
                )
            });

            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(render_file_diff_side(
                    "file-diff-side-old",
                    old_cells,
                    scroll.side_by_side.clone(),
                    scroll.hscroll.clone(),
                    max_line_chars,
                    hovered,
                    old_ctx,
                ))
                .child(div().w(px(1.)).flex_none().bg(p.border))
                .child(render_file_diff_side(
                    "file-diff-side-new",
                    new_cells,
                    scroll.side_by_side.clone(),
                    scroll.hscroll.clone(),
                    max_line_chars,
                    hovered,
                    new_ctx,
                ))
                .into_any_element()
        }
        PreparedFileDiff::Binary => render_binary_diff_placeholder(),
    }
}

/// One soft-wrapped diff row for a given side. Reuses the accent bar and gutter
/// from the nowrap `render_file_diff_line`, but the row grows to fit its wrapped
/// text (no fixed height) and the code cell wraps via `whitespace_normal`
/// against its definite flex width rather than panning.
///
/// Selection, caret, and hit-testing are wrap-aware. The code cell records its
/// own painted bounds into `ctx.wrap_row_origins` (a per-row canvas), keyed by
/// `(pane_selector, row_index)`; those bounds give the wrap width used to place
/// the caret at its visual `(x, y)` and to map a click's local point to a byte
/// column across visual rows. The bounds are read from the *previous* frame, so
/// the very first paint of a row (before any bounds exist) falls back to the
/// single-line caret x at the first visual row — a one-frame imprecision that
/// self-corrects once the canvas has run.
fn render_wrapped_row(
    pane_selector: &'static str,
    row_index: usize,
    mut cell: DiffLineCell,
    selection: RowSelectionState,
    window: &mut Window,
) -> impl IntoElement {
    let RowSelectionState {
        ctx: selection_ctx,
        focused,
    } = selection;
    let p = palette();
    let line_number = cell
        .line_number
        .map(|line_number| line_number.to_string())
        .unwrap_or_default();
    let pane_offset = match pane_selector {
        "file-diff-side-old" => 0,
        "file-diff-side-new" => 1,
        _ => 2,
    };
    let id_index = row_index * 3 + pane_offset;
    let row_selector = diff_line_debug_selector(cell.status);
    let accent = diff_line_accent(cell.status);

    let selection = selection_ctx.and_then(|ctx| ctx.selection);
    // Alignment gaps hold no selectable text, matching the nowrap path.
    let selectable = cell.status != DiffLineStatus::Empty;
    let selection_fill = if focused {
        p.diff_selection_bg
    } else {
        p.diff_selection_bg_unfocused
    };
    let selection_span = selection
        .filter(|_| selectable)
        .and_then(|selection| selection_span_for_row(&selection, row_index, cell.text.len()));
    if let Some((span, _is_final_row)) = selection_span {
        // The byte-range overlay follows the wrapped text across visual rows on
        // its own: `StyledText`'s wrapped background paint splits the fill at
        // each wrap boundary. The cross-row "newline stub" the nowrap path
        // draws is a flex sibling of the text, which a wrapping (block) code
        // cell cannot host without forcing an extra visual row; it is omitted
        // in wrap mode, where the fill over the text bytes already reads the
        // selection.
        cell.highlights =
            diff_selection::overlay_background(&cell.highlights, span, selection_fill);
    }
    let has_text = !cell.text.is_empty();

    // The code cell's previous-frame bounds give the wrap width the text
    // painted at; `None` before the first paint of this row.
    let code_bounds = selection_ctx.and_then(|ctx| {
        ctx.wrap_row_origins
            .borrow()
            .get(&(pane_selector, row_index))
            .copied()
    });
    let wrap_width = code_bounds.map(wrap_width_for);

    // Caret: painted at the selection head, gated on focus and the blink
    // phase, at the head's wrapped visual position. The active-line tint is a
    // bare-caret cue only and stays a full-row tint in wrap mode.
    let head_here = selection.filter(|selection| selection.caret().row == row_index);
    let show_active_line = focused && head_here.is_some_and(|selection| selection.is_caret());
    let caret_point = head_here
        .filter(|_| focused && selection_ctx.is_some_and(|ctx| ctx.caret_visible))
        .map(|selection| {
            let column = selection.caret().column;
            match wrap_width {
                Some(width) => wrapped_point_for_column(window, &cell.text, width, column),
                None => gpui::point(x_for_column(window, &cell.text, column), px(0.)),
            }
        });

    let mouse_down_ctx = selection_ctx.cloned();
    let gutter_ctx = selection_ctx.cloned();
    let mouse_down_text = cell.text.clone();
    let has_line_number = cell.line_number.is_some();

    div()
        .relative()
        .flex()
        .w_full()
        .bg(diff_line_fill(cell.status))
        .id(("file-diff-line", id_index))
        .debug_selector(move || row_selector.to_string())
        .when(show_active_line, |row| {
            row.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(p.active_line_bg)
                    .debug_selector(|| "diff-active-line".to_string()),
            )
        })
        .child(div().w(px(DIFF_ACCENT_WIDTH)).flex_none().when_some(
            accent,
            |bar, (color, accent_selector)| {
                bar.bg(color)
                    .debug_selector(move || accent_selector.to_string())
            },
        ))
        .child({
            // Top-aligned so on a multi-visual-row line the number sits on the
            // first visual row rather than centered against the whole block.
            let mut gutter_cell = div()
                .id(("file-diff-line-number", id_index))
                .flex()
                .items_start()
                .justify_end()
                .w(px(DIFF_LINE_NUMBER_WIDTH))
                .pr_2()
                .flex_none()
                .text_color(p.text_muted)
                .text_size(px(DIFF_TEXT_SIZE))
                .line_height(px(DIFF_LINE_HEIGHT))
                .font_family(MONO_FONT_FAMILY)
                .debug_selector(move || diff_line_index_selector(pane_selector, row_index))
                .child(line_number);
            if has_line_number {
                gutter_cell = gutter_cell.on_mouse_down(MouseButton::Left, {
                    let ctx = gutter_ctx.clone();
                    move |_event: &MouseDownEvent, window, cx| {
                        let Some(ctx) = ctx.as_ref() else {
                            return;
                        };
                        let point = diff_selection::DiffPoint {
                            row: row_index,
                            column: 0,
                        };
                        let ctx = ctx.clone();
                        ctx.app.clone().update(cx, |app, cx| {
                            app.select_diff_line(&ctx, point, cx);
                            app.pane_scroll(ctx.pane, cx).focus.focus(window);
                        });
                    }
                });
            }
            gutter_cell
        })
        .child({
            // A block (not a flex container): its definite flex-item width is
            // handed to the `StyledText` child as the wrap width, so a long
            // line breaks across visual rows instead of overflowing. A flex
            // container would instead keep the text at its single-line
            // max-content width. `.relative()` anchors the caret and the
            // bounds-capturing canvas inside it.
            let mut code_cell = div()
                .id(("file-diff-code", id_index))
                .relative()
                .flex_1()
                .min_w_0()
                .px(px(DIFF_CODE_CELL_PADDING))
                .text_size(px(DIFF_TEXT_SIZE))
                .line_height(px(DIFF_LINE_HEIGHT))
                .whitespace_normal()
                .debug_selector(move || diff_code_content_selector(pane_selector, row_index))
                .when(has_text, |content| {
                    content.child(
                        StyledText::new(cell.text.clone())
                            .with_default_highlights(&diff_text_style(), cell.highlights.clone()),
                    )
                });
            if let Some(ctx) = selection_ctx {
                // Record this row's code-cell bounds for the next frame's
                // caret placement and this frame's hit-testing. The capture
                // lives in the canvas *paint* callback, not prepaint: a
                // variable-height `list` prepaints its items several times per
                // frame (measurement plus rolled-back autoscroll retries) at
                // scratch origins, but paints each visible item exactly once at
                // its final position — so paint is the authoritative bounds.
                let origins = ctx.wrap_row_origins.clone();
                let key = (pane_selector, row_index);
                code_cell = code_cell.child(
                    canvas(
                        |_bounds, _window, _cx| {},
                        move |bounds, _prepaint, _window, _cx| {
                            origins.borrow_mut().insert(key, bounds);
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                );
            }
            if let Some(caret_point) = caret_point {
                code_cell = code_cell.child(
                    div()
                        .absolute()
                        .top(caret_point.y + px(2.))
                        .h(px(DIFF_LINE_HEIGHT - 4.))
                        .left(caret_point.x + px(DIFF_CODE_CELL_PADDING) - px(1.))
                        .w(px(2.))
                        .bg(p.caret)
                        .debug_selector(|| "diff-caret".to_string()),
                );
            }
            if selectable {
                code_cell = code_cell.on_mouse_down(MouseButton::Left, {
                    let ctx = mouse_down_ctx.clone();
                    let text = mouse_down_text.clone();
                    move |event: &MouseDownEvent, window, cx| {
                        let Some(ctx) = ctx.as_ref() else {
                            return;
                        };
                        let bounds = ctx
                            .wrap_row_origins
                            .borrow()
                            .get(&(pane_selector, row_index))
                            .copied();
                        let Some(bounds) = bounds else {
                            return;
                        };
                        let local = local_point_in_cell(bounds, event.position);
                        let width = wrap_width_for(bounds);
                        let column = wrapped_column_for_point(window, &text, width, local);
                        let point = ctx.content.clamp(diff_selection::DiffPoint {
                            row: row_index,
                            column,
                        });
                        let ctx = ctx.clone();
                        ctx.app.clone().update(cx, |app, cx| {
                            app.begin_diff_mouse_selection(&ctx, point, event, window, cx);
                        });
                    }
                });
            }
            code_cell
        })
}

/// Soft-wrap rendering for a single-side diff: one variable-height `list` whose
/// items are wrapped rows. The list state persists on the pane so measured
/// heights survive across renders; its count is synced to the row count here.
fn render_wrapped_single(
    prepared: &Rc<PreparedFileDiff>,
    side: repo::DiffSide,
    rows: &[DiffRow],
    scroll: &FileDiffScroll,
    selection_ctx: Option<&DiffSelectionContext>,
) -> AnyElement {
    let p = palette();
    let selector: &'static str = match side {
        repo::DiffSide::Old => "file-diff-side-old",
        repo::DiffSide::New => "file-diff-side-new",
    };
    let cells: Vec<DiffLineCell> = rows
        .iter()
        .map(|row| match side {
            repo::DiffSide::Old => row.old.clone(),
            repo::DiffSide::New => row.new.clone(),
        })
        .collect();

    // Specialize the context to this side, exactly like the nowrap Single arm:
    // `content` becomes this side's prepared rows, and `selection` is filtered
    // down to the one belonging to this side.
    let ctx = selection_ctx.map(|ctx| {
        ctx.clone_for_side(
            side,
            diff_selection::DiffSideContent::Prepared {
                diff: prepared.clone(),
                side,
            },
        )
    });

    let state = scroll.wrap_list().clone();
    if state.item_count() != cells.len() {
        state.reset(cells.len());
    }
    let cells = Rc::new(cells);

    div()
        .flex()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .bg(p.background)
        .debug_selector(move || selector.to_string())
        .when_some(ctx.clone(), |side, ctx| {
            // A click anywhere on this side lands focus on the pane's handle,
            // so the caret (gated on pane focus) can paint; the drag handlers
            // extend an in-flight selection and clear it on mouse-up.
            attach_wrap_drag_handlers(side, &ctx, selector)
        })
        .child(
            list(state, move |index, window, _cx| {
                let focused = ctx.as_ref().is_some_and(|ctx| ctx.focus.is_focused(window));
                render_wrapped_row(
                    selector,
                    index,
                    cells[index].clone(),
                    RowSelectionState {
                        ctx: ctx.as_ref(),
                        focused,
                    },
                    window,
                )
                .into_any_element()
            })
            .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
            .flex_1(),
        )
        .into_any_element()
}

/// Soft-wrap rendering for a side-by-side diff: one variable-height `list`
/// whose items are full-width row pairs (old column, 1px divider, new column).
/// Both columns live in the same item, so flex's default `align-items: stretch`
/// makes the two columns share the pair's measured height even when one side
/// wraps taller than the other.
fn render_wrapped_side_by_side(
    prepared: &Rc<PreparedFileDiff>,
    rows: &[DiffRow],
    scroll: &FileDiffScroll,
    selection_ctx: Option<&DiffSelectionContext>,
) -> AnyElement {
    let p = palette();
    let old_cells: Vec<DiffLineCell> = rows.iter().map(|r| r.old.clone()).collect();
    let new_cells: Vec<DiffLineCell> = rows.iter().map(|r| r.new.clone()).collect();

    // Per-side contexts, exactly like the nowrap SideBySide arm: each side's
    // `content` is its own prepared rows, and `selection` survives only on the
    // side that owns it.
    let old_ctx = selection_ctx.map(|ctx| {
        ctx.clone_for_side(
            repo::DiffSide::Old,
            diff_selection::DiffSideContent::Prepared {
                diff: prepared.clone(),
                side: repo::DiffSide::Old,
            },
        )
    });
    let new_ctx = selection_ctx.map(|ctx| {
        ctx.clone_for_side(
            repo::DiffSide::New,
            diff_selection::DiffSideContent::Prepared {
                diff: prepared.clone(),
                side: repo::DiffSide::New,
            },
        )
    });

    let state = scroll.wrap_list().clone();
    if state.item_count() != rows.len() {
        state.reset(rows.len());
    }
    let old_cells = Rc::new(old_cells);
    let new_cells = Rc::new(new_cells);

    div()
        .flex()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(
            list(state, move |index, window, _cx| {
                let old_focused = old_ctx
                    .as_ref()
                    .is_some_and(|ctx| ctx.focus.is_focused(window));
                let new_focused = new_ctx
                    .as_ref()
                    .is_some_and(|ctx| ctx.focus.is_focused(window));
                div()
                    .flex()
                    .w_full()
                    .child(wrapped_side_column(
                        "file-diff-side-old",
                        index,
                        old_cells[index].clone(),
                        old_ctx.as_ref(),
                        old_focused,
                        window,
                    ))
                    .child(div().w(px(1.)).flex_none().bg(p.border))
                    .child(wrapped_side_column(
                        "file-diff-side-new",
                        index,
                        new_cells[index].clone(),
                        new_ctx.as_ref(),
                        new_focused,
                        window,
                    ))
                    .into_any_element()
            })
            .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
            .flex_1(),
        )
        .into_any_element()
}

/// One side's column within a side-by-side wrapped row pair: the background
/// wrapper plus the wrapped row. Focus tracking and the drag/mouse-up handlers
/// live on this per-column wrapper (rather than a whole-side container) because
/// side-by-side rows interleave both columns inside a single list item, so
/// there is no single ancestor spanning just one side's rows.
fn wrapped_side_column(
    selector: &'static str,
    row_index: usize,
    cell: DiffLineCell,
    ctx: Option<&DiffSelectionContext>,
    focused: bool,
    window: &mut Window,
) -> AnyElement {
    let p = palette();
    let column = div()
        .flex()
        .flex_1()
        .min_w_0()
        .bg(p.background)
        .debug_selector(move || selector.to_string())
        .child(
            render_wrapped_row(
                selector,
                row_index,
                cell,
                RowSelectionState { ctx, focused },
                window,
            )
            .into_any_element(),
        );
    match ctx {
        Some(ctx) => attach_wrap_drag_handlers(column, ctx, selector).into_any_element(),
        None => column.into_any_element(),
    }
}

/// Attach the soft-wrap side's focus and drag wiring to `element`: `track_focus`
/// so a click lands pane focus (the caret is gated on it), an `on_mouse_move`
/// that extends an in-flight selection via `App::drag_diff_wrapped_move`, and
/// the `on_mouse_up`/`on_mouse_up_out` pair that clears the drag. Shared by the
/// single-side container and each side-by-side column, which both need the
/// identical five handlers.
fn attach_wrap_drag_handlers(
    element: gpui::Div,
    ctx: &DiffSelectionContext,
    selector: &'static str,
) -> gpui::Div {
    let element = element.track_focus(&ctx.focus);
    let element = element.on_mouse_move({
        let ctx = ctx.clone();
        move |event: &MouseMoveEvent, window, cx| {
            let ctx = ctx.clone();
            ctx.app.clone().update(cx, move |app, cx| {
                app.drag_diff_wrapped_move(&ctx, selector, event, window, cx);
            });
        }
    });
    let clear_drag = {
        let ctx = ctx.clone();
        move |_event: &MouseUpEvent, _window: &mut Window, cx: &mut gpui::App| {
            let ctx = ctx.clone();
            ctx.app.clone().update(cx, move |app, _cx| {
                app.end_diff_mouse_drag(&ctx);
            });
        }
    };
    let element = element.on_mouse_up(MouseButton::Left, clear_drag.clone());
    element.on_mouse_up_out(MouseButton::Left, clear_drag)
}

/// the viewport (`end_row >= topmost_row`). This reads correctly even when the
/// diff is scrolled to the bottom and a late block cannot reach the very top —
/// the block is still visible, so it is still reported. Falls back to the last
/// block when everything is scrolled above the top, and returns 0 for an empty
/// slice (callers only invoke this with at least one block).
pub(crate) fn current_block_index(blocks: &[ChangeBlock], topmost_row: usize) -> usize {
    blocks
        .iter()
        .position(|block| block.end_row >= topmost_row)
        .unwrap_or_else(|| blocks.len().saturating_sub(1))
}

/// The block to step forward to: the first block that begins strictly below the
/// anchor row. This is visibility-aware — a block below the current top is the
/// next stop even when the counter already reports it, so a step never skips an
/// unseen change. Wraps to the first block when no block begins below the
/// anchor. (The caller handles the bottom-clamp wrap, where a late block sits
/// below the anchor only because the viewport cannot scroll far enough.)
pub(crate) fn next_block_index(blocks: &[ChangeBlock], anchor_row: usize) -> usize {
    blocks
        .iter()
        .position(|block| block.start_row > anchor_row)
        .unwrap_or(0)
}

/// The block to step backward to: the last block that begins strictly above the
/// anchor row. Wraps to the last block when no block begins above the anchor
/// (e.g. stepping back from the first block or the top of the file). Returns 0
/// for an empty slice.
pub(crate) fn previous_block_index(blocks: &[ChangeBlock], anchor_row: usize) -> usize {
    blocks
        .iter()
        .rposition(|block| block.start_row < anchor_row)
        .unwrap_or_else(|| blocks.len().saturating_sub(1))
}

/// The index of the topmost row visible for a given vertical scroll offset. The
/// offset is zero at the top and grows negative as the content scrolls up, so
/// the row count above the fold is `-offset / row_height`, floored. Clamped to
/// the last row.
pub(crate) fn topmost_row_for_offset(offset_y: Pixels, row_count: usize) -> usize {
    if row_count == 0 {
        return 0;
    }
    let rows_above = (-offset_y / px(DIFF_LINE_HEIGHT)).floor().max(0.) as usize;
    rows_above.min(row_count - 1)
}

/// The topmost visible row of the soft-wrap list. Wrap rows are
/// variable-height, so `ListState` reports the topmost item index directly
/// (`logical_scroll_top().item_ix`) — no pixel-over-row-height math. Clamped to
/// the last row, and zero for an empty list. This is the wrap-mode counterpart
/// of `topmost_row_for_offset`; both feed the shared block-index math.
pub(crate) fn wrapped_topmost_row(scroll: &FileDiffScroll, row_count: usize) -> usize {
    if row_count == 0 {
        return 0;
    }
    scroll
        .wrap_list()
        .logical_scroll_top()
        .item_ix
        .min(row_count - 1)
}

/// Scroll the soft-wrap list so a change block's `start_row` rests at the top of
/// the viewport with up to `CHANGE_BLOCK_CONTEXT_ROWS` of context above it. The
/// wrap-mode counterpart of `set_diff_scroll_top` +
/// `scroll_offset_for_block_top`: it targets the item index directly rather than
/// a pixel offset. `ListState::scroll_to` with `offset_in_item: px(0.)` lands
/// the item flush at the viewport top and updates `logical_scroll_top`
/// synchronously, so the footer counter reads the new position in the same
/// frame (no one-action lag).
pub(crate) fn scroll_wrapped_to_block_top(scroll: &FileDiffScroll, start_row: usize) {
    let item_ix = start_row.saturating_sub(CHANGE_BLOCK_CONTEXT_ROWS);
    scroll.wrap_list().scroll_to(gpui::ListOffset {
        item_ix,
        offset_in_item: px(0.),
    });
}

/// Land wrap row `row` flush at the top of the viewport, no context margin.
/// Unlike `scroll_wrapped_to_block_top` this targets the row directly, so it is
/// the wrap-mode seed used when translating a preserved scroll position across
/// a soft-wrap toggle (the topmost row is already the exact row to restore).
///
/// The caller MUST have synced the wrap list's item count to the diff's row
/// count *before* invoking this. The render's count-sync guard
/// (`render_wrapped_single` / `render_wrapped_side_by_side`) calls
/// `ListState::reset(N)` whenever the count differs, and `reset` nulls
/// `logical_scroll_top` (see gpui `elements/list.rs`), which would silently
/// discard this seed on the first wrap render after the toggle. Syncing the
/// count in the toggle path first makes that guard a no-op, so this seed
/// survives to paint. See `App::set_diff_soft_wrap`.
pub(crate) fn scroll_wrapped_to_row(scroll: &FileDiffScroll, row: usize) {
    scroll.wrap_list().scroll_to(gpui::ListOffset {
        item_ix: row,
        offset_in_item: px(0.),
    });
}

/// The scroll handle and row count that change-block navigation drives for a
/// prepared diff: the shared handle for a side-by-side diff, or the visible
/// side's handle for a single-side one. `None` for a binary diff, which has no
/// navigable rows.
pub(crate) fn change_block_scroll_target(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> Option<(UniformListScrollHandle, usize)> {
    match prepared {
        PreparedFileDiff::Single { side, rows, .. } => {
            Some((scroll.handle_for(*side).clone(), rows.len()))
        }
        PreparedFileDiff::SideBySide { rows, .. } => {
            Some((scroll.side_by_side.clone(), rows.len()))
        }
        PreparedFileDiff::Binary => None,
    }
}

/// The topmost visible row for a prepared diff at its current scroll position.
/// In soft-wrap mode the variable-height `ListState` reports the topmost item
/// directly (`wrapped_topmost_row`); in nowrap mode the fixed-height pixel math
/// (`topmost_row_for_offset`) applies. `None` for a binary diff.
pub(crate) fn change_block_topmost_row(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
    soft_wrap: bool,
) -> Option<usize> {
    let (handle, row_count) = change_block_scroll_target(prepared, scroll)?;
    if soft_wrap {
        return Some(wrapped_topmost_row(scroll, row_count));
    }
    let offset_y = handle.0.borrow().base_handle.offset().y;
    Some(topmost_row_for_offset(offset_y, row_count))
}

/// The anchor row that next/previous navigation compares block starts against:
/// the topmost visible row plus the context margin, so a block resting at its
/// scrolled position (context rows above it) is treated as the current block
/// and a step moves off it. `None` for a binary diff.
pub(crate) fn change_block_anchor_row(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
    soft_wrap: bool,
) -> Option<usize> {
    Some(change_block_topmost_row(prepared, scroll, soft_wrap)? + CHANGE_BLOCK_CONTEXT_ROWS)
}

/// Whether the diff is scrolled to the bottom, where a late change block cannot
/// be brought fully to the top. Forward navigation treats this as "past the
/// last block" so the next step wraps to the first. Requires a real scroll
/// range, so a short diff that entirely fits is never considered at-bottom.
///
/// Nowrap reads the fixed-height pixel offset against its max. Wrap mode has no
/// fixed row height, so it uses the `ListState`'s measured content height:
/// at-bottom is when the topmost item's pixel offset is within a row of the
/// maximum scrollable offset (content height minus viewport height). Both paths
/// return `false` for a diff that fits without scrolling. A full row of
/// tolerance (vs. nowrap's 1px) is required because wrap offsets are not
/// row-quantized: the final block landed flush at the top can leave the content
/// just short of the absolute maximum, so a 1px threshold would never trip.
pub(crate) fn change_block_scrolled_to_bottom(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
    soft_wrap: bool,
) -> bool {
    let Some((handle, _row_count)) = change_block_scroll_target(prepared, scroll) else {
        return false;
    };
    if soft_wrap {
        let list = scroll.wrap_list();
        let max_offset = list.max_offset_for_scrollbar().height;
        let current = -list.scroll_px_offset_for_scrollbar().y;
        return max_offset > px(0.) && current >= max_offset - px(DIFF_LINE_HEIGHT);
    }
    let state = handle.0.borrow();
    let max_height = state.base_handle.max_offset().height;
    let offset_y = state.base_handle.offset().y;
    max_height > px(0.) && offset_y <= -max_height + px(1.)
}

/// The block the reviewer is currently looking at, for a prepared diff at its
/// current scroll position. `None` when the diff has no change blocks.
pub(crate) fn current_change_block(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
    soft_wrap: bool,
) -> Option<usize> {
    let blocks = prepared.blocks();
    if blocks.is_empty() {
        return None;
    }
    let topmost = change_block_topmost_row(prepared, scroll, soft_wrap)?;
    Some(current_block_index(blocks, topmost))
}

/// The vertical scroll offset that places a block's `start_row` at the top of
/// the viewport, leaving up to `context_rows` of context above it. The offset
/// is negative because the content scrolls up. Clamped at the top of the file
/// via `saturating_sub`, so early blocks simply sit at the top.
pub(crate) fn scroll_offset_for_block_top(start_row: usize, context_rows: usize) -> Pixels {
    let top_row = start_row.saturating_sub(context_rows);
    -px(top_row as f32 * DIFF_LINE_HEIGHT)
}

/// Set a diff side's vertical scroll offset directly, preserving the horizontal
/// offset. Used to jump to a change block synchronously so the footer counter
/// reflects the new position in the same frame.
pub(crate) fn set_diff_scroll_top(handle: &UniformListScrollHandle, offset_y: Pixels) {
    let state = handle.0.borrow();
    let x = state.base_handle.offset().x;
    state.base_handle.set_offset(point(x, offset_y));
}

/// A diff side's current vertical scroll offset (negative once scrolled
/// down). Mirrors `set_diff_scroll_top`; used by drag/autoscroll row math to
/// translate a pointer y position into a row index.
pub(crate) fn diff_scroll_top(handle: &UniformListScrollHandle) -> Pixels {
    handle.0.borrow().base_handle.offset().y
}

/// When a diff is first shown, scroll it to its first change block so the
/// reviewer lands on the change instead of the file's top. Consumes the
/// scroll's pending-focus flag (set on open), so it fires once per open and
/// leaves later manual scrolling untouched. A no-op for a diff with no blocks.
///
/// In soft-wrap mode the jump targets the wrap list's item index
/// (`scroll_wrapped_to_block_top`); in nowrap mode it sets the fixed-height
/// pixel offset. The wrap path requires the list's item count to be populated,
/// so the caller must build the diff content (which resets the list to the row
/// count) before invoking this.
pub(crate) fn focus_first_change_block(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
    soft_wrap: bool,
) {
    if !scroll.take_pending_focus() {
        return;
    }
    let Some(first) = prepared.blocks().first() else {
        return;
    };
    if soft_wrap {
        scroll_wrapped_to_block_top(scroll, first.start_row);
        return;
    }
    let Some((handle, _row_count)) = change_block_scroll_target(prepared, scroll) else {
        return;
    };
    set_diff_scroll_top(
        &handle,
        scroll_offset_for_block_top(first.start_row, CHANGE_BLOCK_CONTEXT_ROWS),
    );
}

/// Height of the change-block navigation footer.
const CHANGE_BLOCK_FOOTER_HEIGHT: f32 = 28.;
const CHANGE_BLOCK_BUTTON_SIZE: f32 = 20.;
const CHANGE_BLOCK_ICON_SIZE: f32 = 14.;

/// The right-aligned footer bar for change-block navigation: a `Change N of M`
/// counter and up/down chevrons that jump to the previous/next change block.
/// `None` when the diff has no change blocks, so the caller renders nothing.
pub(crate) fn render_change_block_footer(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
    soft_wrap: bool,
) -> Option<AnyElement> {
    let total = prepared.blocks().len();
    let current = current_change_block(prepared, scroll, soft_wrap)?;
    let p = palette();

    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap_2()
            .h(px(CHANGE_BLOCK_FOOTER_HEIGHT))
            .px_2()
            .border_t_1()
            .border_color(p.border)
            .bg(p.surface)
            .id("change-block-footer")
            .debug_selector(|| "change-block-footer".to_string())
            .child(
                div()
                    .text_color(p.text_muted)
                    .text_size(px(12.))
                    .font_family(MONO_FONT_FAMILY)
                    .debug_selector(|| "change-block-label".to_string())
                    .child(format!("Change {} of {}", current + 1, total)),
            )
            .child(change_block_button(
                "change-block-prev",
                LucideIcon::ChevronUp,
                |window, cx| window.dispatch_action(Box::new(PreviousChangeBlock), cx),
            ))
            .child(change_block_button(
                "change-block-next",
                LucideIcon::ChevronDown,
                |window, cx| window.dispatch_action(Box::new(NextChangeBlock), cx),
            ))
            .into_any_element(),
    )
}

/// One chevron button in the change-block footer. Clicking it dispatches the
/// navigation action `dispatch` runs, so the button and keybinding share one
/// path.
fn change_block_button(
    selector: &'static str,
    icon: LucideIcon,
    dispatch: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let p = palette();
    div()
        .id(selector)
        .debug_selector(move || selector.to_string())
        .flex()
        .items_center()
        .justify_center()
        .w(px(CHANGE_BLOCK_BUTTON_SIZE))
        .h(px(CHANGE_BLOCK_BUTTON_SIZE))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(|button| button.bg(p.element_hover))
        .on_click(move |_event: &gpui::ClickEvent, window, cx| dispatch(window, cx))
        .child(
            Icon::new(icon)
                .size(px(CHANGE_BLOCK_ICON_SIZE))
                .text_color(p.text_muted),
        )
}

pub(crate) fn render_binary_diff_placeholder() -> AnyElement {
    let p = palette();
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(p.background)
        .id("file-diff-binary")
        .debug_selector(|| "file-diff-binary".to_string())
        .text_color(p.text_muted)
        .text_size(px(14.))
        .child("No textual diff is available for this file.")
        .into_any_element()
}

pub(crate) fn render_file_content(
    content: repo::FileContentBody,
    scroll: &FileDiffScroll,
    language: &str,
    hovered: bool,
    selection_ctx: Option<&DiffSelectionContext>,
) -> AnyElement {
    match content {
        repo::FileContentBody::Text(text) => {
            let runs = diff_highlight::line_highlight_runs(&text, language);
            let cells = read_only_file_cells(&text)
                .into_iter()
                .map(|mut cell| {
                    attach_line_runs(&mut cell, &runs);
                    cell
                })
                .collect::<Vec<_>>();

            let max_line_chars = cells
                .iter()
                .map(|cell| cell.text.chars().count())
                .max()
                .unwrap_or(0);

            let ctx = selection_ctx.map(|ctx| {
                ctx.clone_for_side(
                    repo::DiffSide::New,
                    diff_selection::DiffSideContent::ReadOnly {
                        cells: Rc::new(cells.clone()),
                    },
                )
            });

            render_file_diff_side(
                "file-read-only-content",
                cells,
                scroll.handle_for(repo::DiffSide::New).clone(),
                scroll.hscroll.clone(),
                max_line_chars,
                hovered,
                ctx,
            )
            .into_any_element()
        }
        repo::FileContentBody::Binary => render_binary_diff_placeholder(),
    }
}

pub(crate) fn render_file_diff_error(message: String) -> AnyElement {
    let p = palette();
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(p.background)
        .id("file-diff-error")
        .debug_selector(|| "file-diff-error".to_string())
        .text_color(p.danger_fg)
        .text_size(px(14.))
        .child(message)
        .into_any_element()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLineStatus {
    Unchanged,
    Added,
    Removed,
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffLineCell {
    pub(crate) line_number: Option<usize>,
    pub(crate) text: String,
    pub(crate) status: DiffLineStatus,
    pub(crate) highlights: Vec<(Range<usize>, HighlightStyle)>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffRow {
    pub(crate) old: DiffLineCell,
    pub(crate) new: DiffLineCell,
}

impl DiffRow {
    /// A row is "changed" when either side departs from `Unchanged`. `Empty`
    /// is the alignment counterpart of a change on the other side, so it
    /// counts as changed too.
    fn is_changed(&self) -> bool {
        self.old.status != DiffLineStatus::Unchanged || self.new.status != DiffLineStatus::Unchanged
    }
}

/// A navigable region of change: an inclusive range of row indices into the
/// flat rows vec. Endpoints are always changed rows; runs of changed rows
/// separated by no more than `CHANGE_BLOCK_MAX_GAP` unchanged rows merge into
/// a single block, so the merged-in context rows sit inside the span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChangeBlock {
    pub(crate) start_row: usize,
    pub(crate) end_row: usize,
}

/// Two runs of changed rows separated by this many unchanged "context" rows or
/// fewer merge into a single change block, mirroring git's default 3-line
/// context.
pub(crate) const CHANGE_BLOCK_MAX_GAP: usize = 3;

/// When navigating to a change block, leave up to this many context rows above
/// it so the reviewer sees a little of what precedes the change. Clamped
/// automatically at the top of the file.
pub(crate) const CHANGE_BLOCK_CONTEXT_ROWS: usize = 3;

/// Group the flat diff rows into navigable change blocks. Contiguous runs of
/// changed rows separated by `max_gap` or fewer unchanged rows merge into one
/// block; larger gaps split them. Returns blocks in row order.
pub(crate) fn change_blocks(rows: &[DiffRow], max_gap: usize) -> Vec<ChangeBlock> {
    let mut blocks: Vec<ChangeBlock> = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        if !row.is_changed() {
            continue;
        }

        match blocks.last_mut() {
            // Extend the open block when this change is within the gap of the
            // previous change (the unchanged rows between them number
            // `index - last.end_row - 1`).
            Some(last) if index - last.end_row - 1 <= max_gap => {
                last.end_row = index;
            }
            _ => blocks.push(ChangeBlock {
                start_row: index,
                end_row: index,
            }),
        }
    }

    blocks
}

/// The widest line in the diff, in characters, across both sides. Drives the
/// shared horizontal scroll range: every code cell advertises this width so
/// all cells clamp the shared horizontal offset identically (a gpui
/// scrollable clamps a shared handle to its own content each frame). Tabs
/// count as one character — an accepted imprecision for a monospace layout.
pub(crate) fn widest_line_chars(rows: &[DiffRow]) -> usize {
    rows.iter()
        .map(|row| {
            row.old
                .text
                .chars()
                .count()
                .max(row.new.text.chars().count())
        })
        .max()
        .unwrap_or(0)
}

/// Identifies a cached diff: the changed file's path plus the commit and base
/// shas it was diffed against. For committed changesets, the key's shas make
/// entries immutable — different changesets produce different keys. For the
/// pending sentinel sha, the worktree can change under the key; an open pending
/// changeset deliberately caches the first-viewed state as a snapshot, and
/// clearing happens on changeset open/close or repository close.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DiffCacheKey {
    pub(crate) path: String,
    pub(crate) commit_sha: String,
    pub(crate) base_sha: Option<String>,
}

/// A changed file's diff content with the expensive work already done: the line
/// diff computed, the per-side rows aligned, and syntax/word-level highlights
/// attached. This is what the diff cache holds so `render_changed_file_detail`
/// can rebuild its elements cheaply. Not `Eq`: the attached `HighlightStyle`
/// runs carry colors that only implement `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreparedFileDiff {
    Single {
        side: repo::DiffSide,
        rows: Vec<DiffRow>,
        blocks: Vec<ChangeBlock>,
        max_line_chars: usize,
    },
    SideBySide {
        rows: Vec<DiffRow>,
        blocks: Vec<ChangeBlock>,
        max_line_chars: usize,
    },
    Binary,
}

impl PreparedFileDiff {
    pub(crate) fn from_content(content: repo::FileDiffContent, language: &str) -> Self {
        match content {
            repo::FileDiffContent::Single { side, text } => {
                let runs = diff_highlight::line_highlight_runs(&text, language);
                let mut rows = single_side_diff_rows(side, &text);
                for row in &mut rows {
                    match side {
                        repo::DiffSide::Old => attach_line_runs(&mut row.old, &runs),
                        repo::DiffSide::New => attach_line_runs(&mut row.new, &runs),
                    }
                }
                let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);
                let max_line_chars = widest_line_chars(&rows);
                PreparedFileDiff::Single {
                    side,
                    rows,
                    blocks,
                    max_line_chars,
                }
            }
            repo::FileDiffContent::SideBySide { old_text, new_text } => {
                let mut rows = side_by_side_diff_rows(&old_text, &new_text);
                attach_diff_highlights(&mut rows, &old_text, &new_text, language);
                let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);
                let max_line_chars = widest_line_chars(&rows);
                PreparedFileDiff::SideBySide {
                    rows,
                    blocks,
                    max_line_chars,
                }
            }
            repo::FileDiffContent::Binary => PreparedFileDiff::Binary,
        }
    }

    /// The navigable change blocks for this diff, in row order. Empty for a
    /// binary diff or a diff with no changes.
    pub(crate) fn blocks(&self) -> &[ChangeBlock] {
        match self {
            PreparedFileDiff::Single { blocks, .. }
            | PreparedFileDiff::SideBySide { blocks, .. } => blocks,
            PreparedFileDiff::Binary => &[],
        }
    }

    /// The widest line of this diff in characters, across both sides. Zero
    /// for a binary diff, which has no lines to pan.
    pub(crate) fn max_line_chars(&self) -> usize {
        match self {
            PreparedFileDiff::Single { max_line_chars, .. }
            | PreparedFileDiff::SideBySide { max_line_chars, .. } => *max_line_chars,
            PreparedFileDiff::Binary => 0,
        }
    }
}

pub(crate) fn single_side_diff_rows(side: repo::DiffSide, text: &str) -> Vec<DiffRow> {
    let status = match side {
        repo::DiffSide::Old => DiffLineStatus::Removed,
        repo::DiffSide::New => DiffLineStatus::Added,
    };

    content_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let visible = DiffLineCell {
                line_number: Some(index + 1),
                text: line,
                status,
                highlights: Vec::new(),
            };

            match side {
                repo::DiffSide::Old => DiffRow {
                    old: visible,
                    new: empty_diff_cell(),
                },
                repo::DiffSide::New => DiffRow {
                    old: empty_diff_cell(),
                    new: visible,
                },
            }
        })
        .collect()
}

pub(crate) fn side_by_side_diff_rows(old_text: &str, new_text: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(old_text, new_text);
    let old_lines = diff.old_slices();
    let new_lines = diff.new_slices();
    let mut rows = Vec::new();

    for op in diff.ops() {
        match op.tag() {
            DiffTag::Equal => {
                for (old_index, new_index) in op.old_range().zip(op.new_range()) {
                    rows.push(DiffRow {
                        old: diff_cell(old_index, old_lines[old_index], DiffLineStatus::Unchanged),
                        new: diff_cell(new_index, new_lines[new_index], DiffLineStatus::Unchanged),
                    });
                }
            }
            DiffTag::Delete => {
                for old_index in op.old_range() {
                    rows.push(DiffRow {
                        old: diff_cell(old_index, old_lines[old_index], DiffLineStatus::Removed),
                        new: empty_diff_cell(),
                    });
                }
            }
            DiffTag::Insert => {
                for new_index in op.new_range() {
                    rows.push(DiffRow {
                        old: empty_diff_cell(),
                        new: diff_cell(new_index, new_lines[new_index], DiffLineStatus::Added),
                    });
                }
            }
            DiffTag::Replace => {
                let old_indices = op.old_range().collect::<Vec<_>>();
                let new_indices = op.new_range().collect::<Vec<_>>();
                let len = old_indices.len().max(new_indices.len());

                for index in 0..len {
                    let old = old_indices
                        .get(index)
                        .map(|old_index| {
                            diff_cell(*old_index, old_lines[*old_index], DiffLineStatus::Removed)
                        })
                        .unwrap_or_else(empty_diff_cell);
                    let new = new_indices
                        .get(index)
                        .map(|new_index| {
                            diff_cell(*new_index, new_lines[*new_index], DiffLineStatus::Added)
                        })
                        .unwrap_or_else(empty_diff_cell);

                    rows.push(DiffRow { old, new });
                }
            }
        }
    }

    rows
}

pub(crate) fn attach_line_runs(
    cell: &mut DiffLineCell,
    runs: &[Vec<(Range<usize>, HighlightStyle)>],
) {
    if let Some(line) = cell.line_number {
        cell.highlights = runs.get(line - 1).cloned().unwrap_or_default();
    }
}

pub(crate) fn attach_diff_highlights(
    rows: &mut [DiffRow],
    old_text: &str,
    new_text: &str,
    language: &str,
) {
    let old_runs = diff_highlight::line_highlight_runs(old_text, language);
    let new_runs = diff_highlight::line_highlight_runs(new_text, language);
    let p = palette();

    for row in rows.iter_mut() {
        attach_line_runs(&mut row.old, &old_runs);
        attach_line_runs(&mut row.new, &new_runs);

        if row.old.status == DiffLineStatus::Removed && row.new.status == DiffLineStatus::Added {
            let (old_emphasis, new_emphasis) =
                diff_highlight::inline_diff_ranges(&row.old.text, &row.new.text);
            let any_changes = !old_emphasis.is_empty() || !new_emphasis.is_empty();
            if any_changes
                && emphasis_is_subtle(&old_emphasis, row.old.text.len())
                && emphasis_is_subtle(&new_emphasis, row.new.text.len())
            {
                row.old.highlights = diff_highlight::merge_emphasis(
                    &row.old.highlights,
                    &old_emphasis,
                    p.diff_removed_emphasis,
                );
                row.new.highlights = diff_highlight::merge_emphasis(
                    &row.new.highlights,
                    &new_emphasis,
                    p.diff_added_emphasis,
                );
            }
        }
    }
}

/// Word-level emphasis only helps when it stays a small slice of the line;
/// a pair whose differences cover most of the line reads better as a
/// whole-line change. A side with no changed ranges passes trivially —
/// pure insertions/deletions still emphasize the other side.
pub(crate) fn emphasis_is_subtle(ranges: &[Range<usize>], line_len: usize) -> bool {
    if ranges.is_empty() {
        return true;
    }
    if line_len == 0 {
        return false;
    }
    let covered: usize = ranges.iter().map(|range| range.len()).sum();
    covered * 100 <= line_len * EMPHASIS_MAX_COVERAGE_PERCENT
}

/// Above this share of the line, word-level emphasis is noise.
pub(crate) const EMPHASIS_MAX_COVERAGE_PERCENT: usize = 60;

pub(crate) fn diff_cell(line_index: usize, line: &str, status: DiffLineStatus) -> DiffLineCell {
    DiffLineCell {
        line_number: Some(line_index + 1),
        text: trim_line_ending(line),
        status,
        highlights: Vec::new(),
    }
}

pub(crate) fn empty_diff_cell() -> DiffLineCell {
    DiffLineCell {
        line_number: None,
        text: String::new(),
        status: DiffLineStatus::Empty,
        highlights: Vec::new(),
    }
}

pub(crate) fn read_only_file_cells(text: &str) -> Vec<DiffLineCell> {
    content_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| DiffLineCell {
            line_number: Some(index + 1),
            text: line,
            status: DiffLineStatus::Unchanged,
            highlights: Vec::new(),
        })
        .collect()
}

/// Minimum width of every code cell's inner content: the diff's widest line
/// at the monospace advance width, plus the cell padding. Every cell
/// advertising the same content width keeps the shared pan's clamp identical
/// across rows and panes — a gpui scrollable clamps a shared handle to its
/// own content each frame, so a narrower cell would zero the pan.
fn code_cell_min_width(max_line_chars: usize, window: &gpui::Window) -> Pixels {
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&gpui::font(MONO_FONT_FAMILY));
    let advance = text_system
        .advance(font_id, px(DIFF_TEXT_SIZE), '0')
        .map(|size| size.width)
        // A missing glyph metric only mis-sizes the scroll range; 0.6em is a
        // reasonable monospace advance.
        .unwrap_or(px(DIFF_TEXT_SIZE * 0.6));
    advance * max_line_chars as f32 + px(2. * DIFF_CODE_CELL_PADDING)
}

pub(crate) fn render_file_diff_side(
    selector: &'static str,
    cells: Vec<DiffLineCell>,
    scroll_handle: UniformListScrollHandle,
    hscroll: ScrollHandle,
    max_line_chars: usize,
    hovered: bool,
    selection_ctx: Option<DiffSelectionContext>,
) -> impl IntoElement {
    let p = palette();
    let list_hscroll = hscroll.clone();
    let list_ctx = selection_ctx.clone();
    // Cloned before `.track_scroll` consumes `scroll_handle`; the drag/
    // autoscroll listeners below need their own handle to read and mutate
    // this side's vertical offset.
    let drag_scroll_handle = scroll_handle.clone();
    let mut list = uniform_list(selector, cells.len(), move |range, window, _cx| {
        let code_min_width = code_cell_min_width(max_line_chars, window);
        // Read focus here, where `window` is available (a plain
        // `on_mouse_down` closure deep in the row tree only gets `&mut
        // Window`/`&mut gpui::App`, not this list-builder's `window`).
        let focused = list_ctx
            .as_ref()
            .is_some_and(|ctx| ctx.focus.is_focused(window));
        range
            .map(|index| {
                render_file_diff_line(
                    selector,
                    index,
                    cells[index].clone(),
                    &list_hscroll,
                    code_min_width,
                    RowSelectionState {
                        ctx: list_ctx.as_ref(),
                        focused,
                    },
                    window,
                )
            })
            .collect::<Vec<_>>()
    })
    .flex_1()
    .h_full()
    .min_h_0()
    .min_w_0()
    .bg(p.background)
    // Reserve the same 12px right gutter the pre-virtualization scroll area
    // held via `scrollbar_width`. `UniformList` does not implement
    // `StatefulInteractiveElement`, so that modifier is unavailable; right
    // padding insets the rows identically and keeps the diff's appearance.
    .pr(px(12.))
    .track_scroll(scroll_handle)
    .debug_selector(move || selector.to_string());
    // Without this, gpui redirects a *horizontal* wheel gesture onto this
    // vertical-only list (an unused axis delta falls through to the scrollable
    // axis), fighting the code cells' pan.
    list.interactivity().base_style.restrict_scroll_to_axis = Some(true);

    // `max_offset` reads the clamp the code cells wrote on the last paint, so
    // the bar appears only when the content actually overflows.
    let show_scrollbar = hovered && hscroll.max_offset().width > px(0.);

    div()
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .child(list)
        .when_some(selection_ctx, |side, ctx| {
            // Captures this side's painted bounds for pixel->column mapping
            // in the mouse-down listener below.
            let origins = ctx.origins.clone();
            // `track_focus` makes gpui's own mouse-down auto-focus land on
            // the pane's focus handle for any click on this side. Without
            // it, the app root's `track_focus` (the only other focusable)
            // reclaims focus in the same bubble pass — its handler runs
            // after the row listeners — and the caret, gated on pane focus,
            // would never paint. The innermost tracked element wins because
            // gpui prevents parents from transferring focus after it does.
            let side = side.track_focus(&ctx.focus).child(
                canvas(
                    move |bounds, _window, _cx| {
                        origins.borrow_mut().insert(selector, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );

            let side = side.on_mouse_move({
                let ctx = ctx.clone();
                let side_scroll = DiffSideScroll {
                    selector,
                    vertical: drag_scroll_handle.clone(),
                    hscroll: hscroll.clone(),
                };
                move |event: &MouseMoveEvent, window, cx| {
                    let ctx = ctx.clone();
                    let side_scroll = side_scroll.clone();
                    ctx.app.clone().update(cx, move |app, cx| {
                        app.drag_diff_mouse_move(&ctx, &side_scroll, event, window, cx);
                    });
                }
            });

            let clear_drag = {
                let ctx = ctx.clone();
                move |_event: &MouseUpEvent, _window: &mut Window, cx: &mut gpui::App| {
                    let ctx = ctx.clone();
                    ctx.app.clone().update(cx, move |app, _cx| {
                        app.end_diff_mouse_drag(&ctx);
                    });
                }
            };
            let side = side.on_mouse_up(MouseButton::Left, clear_drag.clone());
            side.on_mouse_up_out(MouseButton::Left, clear_drag)
        })
        .when(show_scrollbar, |side| {
            side.child(
                // The overlay spans only the panning code region: inset past
                // the frozen gutter on the left and the reserved 12px row
                // gutter on the right, mirroring the file tree's arrangement.
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(DIFF_GUTTER_WIDTH))
                    .right(px(12.))
                    .debug_selector(move || format!("{selector}-hscrollbar"))
                    .child(Scrollbar::horizontal(&hscroll).scrollbar_show(ScrollbarShow::Always)),
            )
        })
}

/// The byte span within `row_index`'s line that a selection covers, and
/// whether that row is the selection's final row (so no trailing-newline
/// stub is drawn). `None` when the row is not part of a selected range at
/// all — including a bare caret, which highlights nothing.
fn selection_span_for_row(
    selection: &diff_selection::DiffSelection,
    row_index: usize,
    line_len: usize,
) -> Option<(Range<usize>, bool)> {
    if selection.is_caret() {
        return None;
    }
    if !selection.line_range().contains(&row_index) {
        return None;
    }
    let (start, end) = selection.range();
    let span_start = if row_index == start.row {
        start.column
    } else {
        0
    };
    let span_end = if row_index == end.row {
        end.column
    } else {
        line_len
    };
    Some((span_start..span_end, row_index == end.row))
}

/// A diff side's selection context plus this render's focus snapshot, bundled
/// so `render_file_diff_line` stays under clippy's argument-count limit.
/// `focused` is read once per side (in the `uniform_list` item builder, where
/// `window` is available) rather than per row.
pub(crate) struct RowSelectionState<'a> {
    pub(crate) ctx: Option<&'a DiffSelectionContext>,
    pub(crate) focused: bool,
}

pub(crate) fn render_file_diff_line(
    pane_selector: &'static str,
    row_index: usize,
    mut cell: DiffLineCell,
    hscroll: &ScrollHandle,
    code_min_width: Pixels,
    selection: RowSelectionState,
    window: &mut Window,
) -> impl IntoElement {
    let RowSelectionState {
        ctx: selection_ctx,
        focused,
    } = selection;
    let p = palette();
    let line_number = cell
        .line_number
        .map(|line_number| line_number.to_string())
        .unwrap_or_default();
    let pane_offset = match pane_selector {
        "file-diff-side-old" => 0,
        "file-diff-side-new" => 1,
        _ => 2,
    };
    let id_index = row_index * 3 + pane_offset;
    let row_selector = diff_line_debug_selector(cell.status);
    let accent = diff_line_accent(cell.status);

    let selection = selection_ctx.and_then(|ctx| ctx.selection);
    // Alignment gaps are not selectable: they hold no text and
    // `selection_text` skips them, so they take no fill and no newline stub
    // even when a range crosses them.
    let selectable = cell.status != DiffLineStatus::Empty;
    // Unfocused panes still show a selection (so it isn't lost when the user
    // clicks another pane), but dimmed, and with no caret or active-line
    // cue — those read as "you are here," which isn't true for a pane that
    // doesn't own focus.
    let selection_fill = if focused {
        p.diff_selection_bg
    } else {
        p.diff_selection_bg_unfocused
    };
    let selection_span = selection
        .filter(|_| selectable)
        .and_then(|selection| selection_span_for_row(&selection, row_index, cell.text.len()));
    let show_newline_stub = match selection_span {
        Some((span, is_final_row)) => {
            cell.highlights =
                diff_selection::overlay_background(&cell.highlights, span, selection_fill);
            !is_final_row
        }
        None => false,
    };
    let has_text = !cell.text.is_empty();

    // The caret always paints at the selection's head — bare caret or the
    // moving end of a range — but only while the pane owns focus and the
    // blink phase shows it. The active-line tint is a bare-caret cue only: a
    // range selection reads by its fill. Both are skipped entirely when the
    // pane is unfocused.
    let head_here = selection.filter(|selection| selection.caret().row == row_index);
    let show_active_line = focused && head_here.is_some_and(|selection| selection.is_caret());
    let caret_x = head_here
        .filter(|_| focused && selection_ctx.is_some_and(|ctx| ctx.caret_visible))
        .map(|selection| x_for_column(window, &cell.text, selection.caret().column));

    let mouse_down_ctx = selection_ctx.cloned();
    let mouse_down_text = cell.text.clone();

    div()
        .relative()
        .flex()
        .h(px(DIFF_LINE_HEIGHT))
        // `uniform_list` lays each row out with the pane's width as the
        // available space, but a bare flex row sizes to its content.
        // `w_full` pins the row to that width so the status fill and the
        // empty-gap hatch span it; a `min_w_full` (a lower bound, not a cap)
        // let the code cell's huge min-content width balloon the row itself,
        // stretching the scrollable code cell to its content and zeroing the
        // pan's clamp. Long code no longer widens the row — it pans inside
        // the code cell instead.
        .w_full()
        .bg(diff_line_fill(cell.status))
        .id(("file-diff-line", id_index))
        .debug_selector(move || row_selector.to_string())
        .when(show_active_line, |row| {
            row.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(p.active_line_bg)
                    .debug_selector(|| "diff-active-line".to_string()),
            )
        })
        .child(div().w(px(DIFF_ACCENT_WIDTH)).flex_none().when_some(
            accent,
            |bar, (color, accent_selector)| {
                bar.bg(color)
                    .debug_selector(move || accent_selector.to_string())
            },
        ))
        .child({
            let mut gutter_cell = div()
                .id(("file-diff-line-number", id_index))
                .flex()
                .items_center()
                .justify_end()
                .w(px(DIFF_LINE_NUMBER_WIDTH))
                .pr_2()
                .flex_none()
                .text_color(p.text_muted)
                .text_size(px(DIFF_TEXT_SIZE))
                .line_height(px(DIFF_LINE_HEIGHT))
                .font_family(MONO_FONT_FAMILY)
                .debug_selector(move || diff_line_index_selector(pane_selector, row_index))
                .child(line_number);
            if cell.line_number.is_some() {
                let ctx = mouse_down_ctx.clone();
                gutter_cell = gutter_cell.on_mouse_down(MouseButton::Left, {
                    move |_event: &MouseDownEvent, window, cx| {
                        let Some(ctx) = ctx.as_ref() else {
                            return;
                        };
                        let point = diff_selection::DiffPoint {
                            row: row_index,
                            column: 0,
                        };
                        let ctx = ctx.clone();
                        ctx.app.clone().update(cx, |app, cx| {
                            app.select_diff_line(&ctx, point, cx);
                            app.pane_scroll(ctx.pane, cx).focus.focus(window);
                        });
                    }
                });
            }
            gutter_cell
        })
        .child({
            let mut code_cell = div()
                .id(("file-diff-code", id_index))
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(hscroll)
                // gpui's `StyledText` takes its font size and line height from
                // the ambient `window.text_style()` (the cascade of `.text_size`
                // / `.line_height` from ancestor divs), NOT from the `TextStyle`
                // passed to `with_default_highlights` — that style only supplies
                // per-run font family, weight, and color. So the authoritative
                // sizing for the code text lives here, not in `diff_text_style()`.
                .text_size(px(DIFF_TEXT_SIZE))
                .line_height(px(DIFF_LINE_HEIGHT))
                .child({
                    // Every cell's content advertises the same minimum width
                    // (see `code_cell_min_width`), keeping the shared pan's
                    // clamp identical across rows and panes.
                    let mut content = div()
                        .relative()
                        .flex()
                        .items_center()
                        .h_full()
                        .px(px(DIFF_CODE_CELL_PADDING))
                        .min_w(code_min_width)
                        .whitespace_nowrap()
                        .debug_selector(move || {
                            diff_code_content_selector(pane_selector, row_index)
                        })
                        .when(has_text, |content| {
                            content.child(
                                StyledText::new(cell.text.clone())
                                    .with_default_highlights(&diff_text_style(), cell.highlights),
                            )
                        })
                        .when(show_newline_stub, |content| {
                            let mut stub = div()
                                .w(px(DIFF_TEXT_SIZE * 0.6))
                                .h_full()
                                .bg(selection_fill);
                            if !focused {
                                stub = stub.debug_selector(|| "diff-selection-dim".to_string());
                            }
                            content.child(stub)
                        });
                    if let Some(caret_x) = caret_x {
                        content = content.child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .bottom(px(2.))
                                .left(caret_x + px(DIFF_CODE_CELL_PADDING) - px(1.))
                                .w(px(2.))
                                .bg(p.caret)
                                .debug_selector(|| "diff-caret".to_string()),
                        );
                    }
                    content
                });
            // Without this, gpui redirects a *vertical* wheel gesture onto
            // this horizontal-only cell, panning when the user meant to
            // scroll the rows.
            code_cell.interactivity().base_style.restrict_scroll_to_axis = Some(true);
            code_cell.on_mouse_down(MouseButton::Left, {
                let ctx = mouse_down_ctx.clone();
                let text = mouse_down_text.clone();
                move |event: &MouseDownEvent, window, cx| {
                    let Some(ctx) = ctx.as_ref() else {
                        return;
                    };
                    let origin = ctx
                        .origins
                        .borrow()
                        .get(pane_selector)
                        .map(|bounds| bounds.origin);
                    let Some(origin) = origin else {
                        return;
                    };
                    // x within the code content: window x minus the side
                    // origin, the frozen gutter, and the cell padding, plus
                    // the shared pan (negative when panned right, so adding
                    // it shifts the hit-test to match the panned text).
                    let pan = ctx.hscroll.offset().x;
                    let x = event.position.x
                        - origin.x
                        - px(DIFF_GUTTER_WIDTH)
                        - px(DIFF_CODE_CELL_PADDING)
                        - pan;
                    let column = column_for_x(window, &text, x);
                    let point = ctx.content.clamp(diff_selection::DiffPoint {
                        row: row_index,
                        column,
                    });
                    let ctx = ctx.clone();
                    ctx.app.clone().update(cx, |app, cx| {
                        app.begin_diff_mouse_selection(&ctx, point, event, window, cx);
                    });
                }
            })
        })
}

/// Per-row debug selector for the panning code content, e.g.
/// `file-diff-code-new-12`. Its painted origin shifts with the shared pan,
/// which is what lets a test prove the code moved while the gutter did not.
pub(crate) fn diff_code_content_selector(pane_selector: &str, row_index: usize) -> String {
    let side = match pane_selector {
        "file-diff-side-old" => "old",
        "file-diff-side-new" => "new",
        _ => "single",
    };
    format!("file-diff-code-{side}-{row_index}")
}

/// Per-row debug selector encoding the column side and row index, e.g.
/// `file-diff-line-new-12`. Called from a `debug_selector` closure, so it runs
/// only in test/test-support builds and only for rows the virtualized list
/// actually renders — which is what lets a test prove off-screen rows are absent.
pub(crate) fn diff_line_index_selector(pane_selector: &str, row_index: usize) -> String {
    let side = match pane_selector {
        "file-diff-side-old" => "old",
        "file-diff-side-new" => "new",
        _ => "single",
    };
    format!("file-diff-line-{side}-{row_index}")
}

/// Row height for one diff line: tall enough to contain the 14px monospace
/// glyphs without clipping, with extra vertical padding around them, and the
/// shared box that vertically centers the gutter number against its code line.
pub(crate) const DIFF_LINE_HEIGHT: f32 = 22.;

/// Extra measured area above and below the viewport for the soft-wrap list,
/// ~9 rows, to avoid pop-in during fast scrolls.
pub(crate) const DIFF_WRAP_OVERDRAW: f32 = 200.;

/// Width of the status accent bar at a row's left edge.
pub(crate) const DIFF_ACCENT_WIDTH: f32 = 3.;

/// Width of the line-number cell.
pub(crate) const DIFF_LINE_NUMBER_WIDTH: f32 = 48.;

/// The frozen gutter: accent bar plus line numbers. Code cells pan to its
/// right; the horizontal scrollbar overlay is inset by this much so it spans
/// only the panning region.
pub(crate) const DIFF_GUTTER_WIDTH: f32 = DIFF_ACCENT_WIDTH + DIFF_LINE_NUMBER_WIDTH;

/// Horizontal padding inside a code cell, each side.
pub(crate) const DIFF_CODE_CELL_PADDING: f32 = 8.;

/// Horizontal margin kept between the caret and the pane edge when keyboard
/// motion scrolls the caret into view — the horizontal counterpart of the
/// one-line vertical margin, so the caret never sits flush against the edge
/// with the adjacent text hidden.
pub(crate) const DIFF_CARET_H_MARGIN: f32 = 24.;

/// Font size for diff gutter numbers and code lines. Kept in one place so the
/// gutter and the code text always render at the same scale.
pub(crate) const DIFF_TEXT_SIZE: f32 = 14.;

/// Base text style for diff code lines; syntax runs override color per token.
///
/// Note: gpui's `StyledText` reads `font_size` and `line_height` from the
/// ambient `window.text_style()`, not from this struct — only the per-run
/// font family, weight, and color carry through `with_default_highlights`.
/// The `font_size`/`line_height` here are kept in sync with `DIFF_TEXT_SIZE`
/// and `DIFF_LINE_HEIGHT` for documentation, but the values that actually
/// drive layout are set on the code-cell div in `render_file_diff_line`.
pub(crate) fn diff_text_style() -> TextStyle {
    let p = palette();
    TextStyle {
        color: p.code_text,
        font_family: MONO_FONT_FAMILY.into(),
        font_size: px(DIFF_TEXT_SIZE).into(),
        line_height: px(DIFF_LINE_HEIGHT).into(),
        ..TextStyle::default()
    }
}

/// Map a pixel offset within a line's code content to the nearest column
/// (UTF-8 byte offset on a char boundary), by shaping the line once. A
/// single default run is enough: every token uses the same mono font/size,
/// so color-only syntax runs don't change metrics.
pub(crate) fn column_for_x(window: &mut Window, text: &str, x: Pixels) -> usize {
    let style = diff_text_style();
    let run = style.to_run(text.len());
    let shaped =
        window
            .text_system()
            .shape_line(text.to_string().into(), px(DIFF_TEXT_SIZE), &[run], None);
    shaped.closest_index_for_x(x)
}

/// The x offset (within the shaped line, no cell padding or pan) of `column`.
pub(crate) fn x_for_column(window: &mut Window, text: &str, column: usize) -> Pixels {
    let style = diff_text_style();
    let run = style.to_run(text.len());
    let shaped =
        window
            .text_system()
            .shape_line(text.to_string().into(), px(DIFF_TEXT_SIZE), &[run], None);
    shaped.x_for_index(column)
}

/// The wrap width a code cell's text laid out at: the captured cell width minus
/// the padding on both sides, floored at 1px so shaping never gets a
/// non-positive width. Shared by the caret placement, the click hit-test, and
/// the drag path so they all size the wrap identically.
fn wrap_width_for(bounds: Bounds<Pixels>) -> Pixels {
    (bounds.size.width - px(2. * DIFF_CODE_CELL_PADDING)).max(px(1.))
}

/// A window position translated into a wrapped code cell's local content frame:
/// origin at the text's top-left (one cell-padding right of the cell's origin;
/// wrap mode never pans, so no horizontal pan offset applies). `x` locates the
/// column within a visual row, `y` the visual row.
fn local_point_in_cell(bounds: Bounds<Pixels>, position: Point<Pixels>) -> Point<Pixels> {
    gpui::point(
        position.x - bounds.origin.x - px(DIFF_CODE_CELL_PADDING),
        position.y - bounds.origin.y,
    )
}

/// Shape `text` wrapped to `wrap_width`, mirroring how `StyledText` lays out a
/// wrapped diff code cell: one default run (every diff token uses the same mono
/// font/size, so color-only syntax runs don't change metrics), the same font
/// size, and no line clamp. Returns the first (and only, since diff cell text
/// holds no newlines) wrapped line, or `None` if shaping fails.
fn shape_wrapped(window: &mut Window, text: &str, wrap_width: Pixels) -> Option<gpui::WrappedLine> {
    let style = diff_text_style();
    let run = style.to_run(text.len());
    window
        .text_system()
        .shape_text(
            text.to_string().into(),
            px(DIFF_TEXT_SIZE),
            &[run],
            Some(wrap_width),
            None,
        )
        .ok()
        .and_then(|lines| lines.into_iter().next())
}

/// Map a local point within a wrapped code cell's content (origin at the text's
/// top-left, cell padding and pan already removed) to the nearest column, using
/// the same wrap width the cell painted at. `WrappedLine::closest_index_for_position`
/// picks the visual row from `point.y / line_height`, then the byte index from
/// `point.x` within that row — so a click on the second visual row maps past the
/// first row's final column. Falls back to the single-line mapping when shaping
/// fails.
pub(crate) fn wrapped_column_for_point(
    window: &mut Window,
    text: &str,
    wrap_width: Pixels,
    point: Point<Pixels>,
) -> usize {
    match shape_wrapped(window, text, wrap_width) {
        Some(line) => match line.closest_index_for_position(point, px(DIFF_LINE_HEIGHT)) {
            Ok(index) => index,
            Err(index) => index,
        },
        None => column_for_x(window, text, point.x),
    }
}

/// The visual (x, y) position of `column` within a wrapped code cell's content
/// (origin at the text's top-left, no cell padding or pan), using the same wrap
/// width the cell painted at. The y locates the caret's visual row; the x its
/// offset within that row. Falls back to `(x_for_column, 0)` — the first visual
/// row — when shaping fails or the index is out of range, which is acceptable
/// for a one-frame imprecision that self-corrects once bounds are captured.
pub(crate) fn wrapped_point_for_column(
    window: &mut Window,
    text: &str,
    wrap_width: Pixels,
    column: usize,
) -> Point<Pixels> {
    match shape_wrapped(window, text, wrap_width)
        .and_then(|line| line.position_for_index(column, px(DIFF_LINE_HEIGHT)))
    {
        Some(position) => position,
        None => gpui::point(x_for_column(window, text, column), px(0.)),
    }
}

/// Map a pointer position during a wrap-mode drag to a `DiffPoint` on
/// `ctx.content`. Wrap rows are variable-height, so the row cannot be derived
/// from a fixed row height; instead each row's painted code-cell bounds (stored
/// in `ctx.wrap_row_origins`) are tested against the pointer's y. The pointer is
/// clamped to the nearest captured row when it sits above the first or below the
/// last — so a drag that runs off either end still extends toward that end. The
/// column then comes from the wrap-aware point mapping within the chosen row's
/// cell. Returns `None` when no row bounds have been captured yet (nothing to
/// hit-test against).
pub(crate) fn wrapped_drag_point(
    window: &mut Window,
    ctx: &DiffSelectionContext,
    selector: &'static str,
    position: Point<Pixels>,
) -> Option<diff_selection::DiffPoint> {
    // Snapshot this side's captured rows, sorted by row index, so the search
    // and the above/below clamp read in document order. The map is cleared on
    // every file switch (`FileDiffScroll::reset`), so every entry belongs to
    // the current file.
    let rows: Vec<(usize, Bounds<Pixels>)> = {
        let origins = ctx.wrap_row_origins.borrow();
        let mut rows: Vec<(usize, Bounds<Pixels>)> = origins
            .iter()
            .filter(|((sel, _), _)| *sel == selector)
            .map(|((_, row), bounds)| (*row, *bounds))
            .collect();
        rows.sort_by_key(|(row, _)| *row);
        rows
    };
    let (&(first_row, first_bounds), &(last_row, last_bounds)) = (rows.first()?, rows.last()?);

    // The row whose vertical band contains the pointer, else the clamp ends.
    let (row, bounds) = if position.y < first_bounds.origin.y {
        (first_row, first_bounds)
    } else if position.y >= last_bounds.bottom_left().y {
        (last_row, last_bounds)
    } else {
        rows.iter()
            .copied()
            .find(|(_, bounds)| {
                position.y >= bounds.origin.y && position.y < bounds.bottom_left().y
            })
            // Between captured rows (a gap the search missed): fall back to the
            // last row at or above the pointer.
            .or_else(|| {
                rows.iter()
                    .copied()
                    .rev()
                    .find(|(_, bounds)| position.y >= bounds.origin.y)
            })
            .unwrap_or((last_row, last_bounds))
    };

    let text = ctx.content.cell(row).text.clone();
    let width = wrap_width_for(bounds);
    let local = local_point_in_cell(bounds, position);
    let column = wrapped_column_for_point(window, &text, width, local);
    Some(ctx.content.clamp(diff_selection::DiffPoint { row, column }))
}

/// A diff side's scroll handles plus its debug selector, bundled for the
/// drag/autoscroll listeners so they stay under clippy's argument-count
/// limit. `selector` is this side's `content_origins` key, used to look up
/// its painted bounds.
#[derive(Clone)]
pub(crate) struct DiffSideScroll {
    pub(crate) selector: &'static str,
    pub(crate) vertical: UniformListScrollHandle,
    pub(crate) hscroll: ScrollHandle,
}

/// Map a pointer position during a drag or autoscroll tick to a `DiffPoint`
/// on `ctx.content`: the row from the pointer's y within `side_bounds` minus
/// the side's vertical scroll offset, divided by the row height and clamped
/// to the content's row range, then the column from `column_for_x` on that
/// row's text using the same x math as the mouse-down listener. `content.clamp`
/// snaps a gap row to a neighboring selectable row and the column to a char
/// boundary within the line.
pub(crate) fn drag_point(
    window: &mut Window,
    ctx: &DiffSelectionContext,
    side_scroll: &DiffSideScroll,
    side_bounds: Bounds<Pixels>,
    position: Point<Pixels>,
) -> diff_selection::DiffPoint {
    let vertical_offset = diff_scroll_top(&side_scroll.vertical);
    let relative_y = (position.y - side_bounds.origin.y) - vertical_offset;
    let row_count = ctx.content.len();
    let row = if row_count == 0 {
        0
    } else {
        let candidate = (relative_y / px(DIFF_LINE_HEIGHT)).floor();
        (candidate.max(0.) as usize).min(row_count - 1)
    };

    let pan = ctx.hscroll.offset().x;
    let x = position.x
        - side_bounds.origin.x
        - px(DIFF_GUTTER_WIDTH)
        - px(DIFF_CODE_CELL_PADDING)
        - pan;
    let text = if row_count == 0 {
        String::new()
    } else {
        ctx.content.cell(row).text.clone()
    };
    let column = column_for_x(window, &text, x);
    ctx.content.clamp(diff_selection::DiffPoint { row, column })
}

/// Event-driven edge autoscroll for a drag: when the pointer sits within
/// `DIFF_LINE_HEIGHT` (capped at a third of the side's height) of the top or
/// bottom edge, nudges the side's vertical offset by up to one line height;
/// same shape horizontally against the code region's left/right edges,
/// applied to the shared pan. gpui's own layout clamps both offsets to their
/// content's range on the next paint, so no manual clamping is needed here.
/// Returns whether either axis moved, so the caller knows to repaint even
/// when the drag point itself did not change.
pub(crate) fn autoscroll_diff_side(
    side_scroll: &DiffSideScroll,
    side_bounds: Bounds<Pixels>,
    position: Point<Pixels>,
) -> bool {
    let margin = px(DIFF_LINE_HEIGHT).min(side_bounds.size.height / 3.);
    let mut delta = point(px(0.), px(0.));
    if position.y < side_bounds.origin.y + margin {
        delta.y = (side_bounds.origin.y + margin - position.y).min(px(DIFF_LINE_HEIGHT));
    } else if position.y > side_bounds.bottom_left().y - margin {
        delta.y = -(position.y - (side_bounds.bottom_left().y - margin)).min(px(DIFF_LINE_HEIGHT));
    }

    let code_left = side_bounds.origin.x + px(DIFF_GUTTER_WIDTH);
    let code_right = side_bounds.bottom_right().x;
    let h_margin = px(DIFF_LINE_HEIGHT).min((code_right - code_left) / 3.);
    if position.x < code_left + h_margin {
        delta.x = (code_left + h_margin - position.x).min(px(DIFF_LINE_HEIGHT));
    } else if position.x > code_right - h_margin {
        delta.x = -(position.x - (code_right - h_margin)).min(px(DIFF_LINE_HEIGHT));
    }

    if delta.y != px(0.) {
        let state = side_scroll.vertical.0.borrow();
        let offset = state.base_handle.offset();
        state
            .base_handle
            .set_offset(point(offset.x, offset.y + delta.y));
    }
    if delta.x != px(0.) {
        let offset = side_scroll.hscroll.offset();
        side_scroll
            .hscroll
            .set_offset(point(offset.x + delta.x, offset.y));
    }

    delta.y != px(0.) || delta.x != px(0.)
}

/// Stroke width and spacing, in device pixels, of the alignment-gap hatch.
///
/// gpui's slash shader phases the stripes from each quad's own origin, and
/// every diff row is its own quad — so a multi-line gap, drawn as a stack of
/// `Empty` rows, only reads as one continuous hatched block when the pattern
/// period (stroke + interval) divides the row height in device pixels.
/// Otherwise the diagonals shear at every row seam. 2 + 9 = 11 divides the
/// 22px row at 1x, 1.5x, and 2x backing scales; a test guards the invariant.
pub(crate) const DIFF_HATCH_STROKE: f32 = 2.;
pub(crate) const DIFF_HATCH_INTERVAL: f32 = 9.;

/// Material red/green at ~15% alpha over the editor background; alignment gaps
/// hatch with a diagonal pattern like Zed's, in a light theme grey so the
/// slashes read clearly against the dark background.
pub(crate) fn diff_line_fill(status: DiffLineStatus) -> Background {
    match status {
        DiffLineStatus::Unchanged => palette().background.into(),
        DiffLineStatus::Added => palette().diff_added_bg.into(),
        DiffLineStatus::Removed => palette().diff_removed_bg.into(),
        DiffLineStatus::Empty => pattern_slash(
            palette().diff_empty_hatch,
            DIFF_HATCH_STROKE,
            DIFF_HATCH_INTERVAL,
        ),
    }
}

pub(crate) fn diff_line_accent(status: DiffLineStatus) -> Option<(Hsla, &'static str)> {
    match status {
        DiffLineStatus::Added => Some((palette().diff_added_fg, "file-diff-accent-added")),
        DiffLineStatus::Removed => Some((palette().diff_removed_fg, "file-diff-accent-removed")),
        DiffLineStatus::Unchanged | DiffLineStatus::Empty => None,
    }
}

pub(crate) fn diff_line_debug_selector(status: DiffLineStatus) -> &'static str {
    match status {
        DiffLineStatus::Unchanged => "file-diff-row-unchanged",
        DiffLineStatus::Added => "file-diff-row-added",
        DiffLineStatus::Removed => "file-diff-row-removed",
        DiffLineStatus::Empty => "file-diff-row-empty",
    }
}

pub(crate) fn content_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.lines().map(str::to_string).collect()
}

pub(crate) fn trim_line_ending(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::repo::{ChangeKind, DiffSide};
    use gpui::{px, TestAppContext, VisualTestContext, WindowHandle};

    #[test]
    fn change_blocks_group_one_contiguous_run() {
        let rows = side_by_side_diff_rows("a_old\nb_old\n", "a_new\nb_new\n");
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 1);
    }

    #[test]
    fn change_blocks_split_runs_separated_by_a_large_gap() {
        // Four unchanged context rows separate the two changes: gap > max, so
        // the runs stay distinct blocks.
        let old_text = "a_old\nctx1\nctx2\nctx3\nctx4\nb_old\n";
        let new_text = "a_new\nctx1\nctx2\nctx3\nctx4\nb_new\n";
        let rows = side_by_side_diff_rows(old_text, new_text);
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 0);
        assert_eq!(blocks[1].start_row, 5);
        assert_eq!(blocks[1].end_row, 5);
    }

    #[test]
    fn change_blocks_merge_runs_within_the_gap() {
        // Exactly three unchanged context rows separate the changes: gap == max,
        // so the runs merge into one block whose endpoints are changed rows.
        let old_text = "a_old\nctx1\nctx2\nctx3\nb_old\n";
        let new_text = "a_new\nctx1\nctx2\nctx3\nb_new\n";
        let rows = side_by_side_diff_rows(old_text, new_text);
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 4);
    }

    #[test]
    fn change_blocks_single_side_is_one_block() {
        let rows = single_side_diff_rows(DiffSide::New, "first\nsecond\nthird\n");
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 2);
    }

    #[test]
    fn change_blocks_are_empty_when_nothing_changed() {
        let rows = side_by_side_diff_rows("same\nlines\n", "same\nlines\n");
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert!(blocks.is_empty());
    }

    #[test]
    fn prepared_side_by_side_diff_exposes_change_blocks() {
        let content = repo::FileDiffContent::SideBySide {
            old_text: "a_old\nctx\nb_old\n".to_string(),
            new_text: "a_new\nctx\nb_new\n".to_string(),
        };
        let prepared = PreparedFileDiff::from_content(content, "");

        // One unchanged context row separates the changes (gap 1 <= max), so
        // they merge into a single block.
        assert_eq!(prepared.blocks().len(), 1);
        assert_eq!(prepared.blocks()[0].start_row, 0);
        assert_eq!(prepared.blocks()[0].end_row, 2);
    }

    #[test]
    fn prepared_binary_diff_has_no_change_blocks() {
        let prepared = PreparedFileDiff::from_content(repo::FileDiffContent::Binary, "");
        assert!(prepared.blocks().is_empty());
    }

    #[test]
    fn current_block_index_reports_the_first_block_at_or_below_the_top() {
        let blocks = [
            ChangeBlock {
                start_row: 4,
                end_row: 6,
            },
            ChangeBlock {
                start_row: 29,
                end_row: 31,
            },
        ];

        // Above the first block: it is the first one still in view.
        assert_eq!(current_block_index(&blocks, 0), 0);
        // Within the first block.
        assert_eq!(current_block_index(&blocks, 5), 0);
        // Below the first block but above the second: the second is next in view.
        assert_eq!(current_block_index(&blocks, 20), 1);
        // Within the second block — reported even if it cannot reach the top.
        assert_eq!(current_block_index(&blocks, 30), 1);
        // Scrolled past every block: clamp to the last.
        assert_eq!(current_block_index(&blocks, 50), 1);
    }

    #[test]
    fn next_block_index_steps_to_the_first_block_below_the_anchor() {
        let blocks = [
            ChangeBlock {
                start_row: 4,
                end_row: 4,
            },
            ChangeBlock {
                start_row: 29,
                end_row: 29,
            },
        ];

        // Anchor above the first block: the first block is the next stop, not
        // skipped (the visibility-aware fix).
        assert_eq!(next_block_index(&blocks, 0), 0);
        // Resting on the first block: advance to the second.
        assert_eq!(next_block_index(&blocks, 4), 1);
        // In the gap: the second block is the next stop.
        assert_eq!(next_block_index(&blocks, 10), 1);
        // Nothing begins below the anchor: wrap to the first block.
        assert_eq!(next_block_index(&blocks, 29), 0);
    }

    #[test]
    fn previous_block_index_steps_to_the_last_block_above_the_anchor() {
        let blocks = [
            ChangeBlock {
                start_row: 4,
                end_row: 4,
            },
            ChangeBlock {
                start_row: 29,
                end_row: 29,
            },
        ];

        // At or before the first block: wrap to the last.
        assert_eq!(previous_block_index(&blocks, 4), 1);
        assert_eq!(previous_block_index(&blocks, 0), 1);
        // In the gap: step back to the first block.
        assert_eq!(previous_block_index(&blocks, 10), 0);
        // At the last block: step back to the first.
        assert_eq!(previous_block_index(&blocks, 29), 0);
    }

    #[test]
    fn topmost_row_for_offset_floors_by_row_height() {
        // No scroll: the first row is at the top.
        assert_eq!(topmost_row_for_offset(px(0.), 10), 0);
        // Offset is negative as the content scrolls up; 2.5 rows down floors to 2.
        assert_eq!(topmost_row_for_offset(px(-DIFF_LINE_HEIGHT * 2.5), 10), 2);
        // Beyond the last row clamps to it.
        assert_eq!(topmost_row_for_offset(px(-DIFF_LINE_HEIGHT * 100.), 10), 9);
        // An empty diff has no rows to land on.
        assert_eq!(topmost_row_for_offset(px(-50.), 0), 0);
    }

    #[test]
    fn scroll_offset_for_block_top_leaves_context_above_the_block() {
        // A block starting at row 10 lands with three context rows above it.
        assert_eq!(
            scroll_offset_for_block_top(10, CHANGE_BLOCK_CONTEXT_ROWS),
            -px(7. * DIFF_LINE_HEIGHT)
        );
        // A block within the context margin of the top clamps to the top.
        assert_eq!(
            scroll_offset_for_block_top(2, CHANGE_BLOCK_CONTEXT_ROWS),
            px(0.)
        );
    }

    #[test]
    fn line_diff_single_side_added_marks_lines_as_added() {
        let rows = single_side_diff_rows(DiffSide::New, "first\nsecond\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].new.status, DiffLineStatus::Added);
        assert_eq!(rows[0].new.line_number, Some(1));
        assert_eq!(rows[0].new.text, "first");
        assert_eq!(rows[0].old.status, DiffLineStatus::Empty);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
    }

    #[test]
    fn line_diff_single_side_deleted_marks_lines_as_removed() {
        let rows = single_side_diff_rows(DiffSide::Old, "first\nsecond\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[0].old.line_number, Some(1));
        assert_eq!(rows[0].old.text, "first");
        assert_eq!(rows[0].new.status, DiffLineStatus::Empty);
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
    }

    #[test]
    fn line_diff_side_by_side_aligns_equal_removed_and_added_rows() {
        let rows = side_by_side_diff_rows("same\nold\n", "same\nnew\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].old.status, DiffLineStatus::Unchanged);
        assert_eq!(rows[0].new.status, DiffLineStatus::Unchanged);
        assert_eq!(rows[0].old.text, "same");
        assert_eq!(rows[0].new.text, "same");
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
        assert_eq!(rows[1].old.text, "old");
        assert_eq!(rows[1].new.text, "new");
    }

    #[test]
    fn line_diff_side_by_side_pads_uneven_replacements() {
        let rows = side_by_side_diff_rows("one\ntwo\n", "one\nalpha\nbeta\n");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
        assert_eq!(rows[2].old.status, DiffLineStatus::Empty);
        assert_eq!(rows[2].new.status, DiffLineStatus::Added);
        assert_eq!(rows[2].new.text, "beta");
    }

    #[test]
    fn python_replace_rows_carry_syntax_and_word_level_highlights() {
        let old_text = "threshold = 10\n";
        let new_text = "threshold = None\n";
        let mut rows = side_by_side_diff_rows(old_text, new_text);
        attach_diff_highlights(&mut rows, old_text, new_text, "py");

        assert!(
            !rows[0].old.highlights.is_empty(),
            "old side should carry syntax runs"
        );
        assert!(
            rows[0]
                .new
                .highlights
                .iter()
                .any(|(_, style)| style.background_color.is_some()),
            "changed token on the new side should carry an emphasis background"
        );
    }

    #[test]
    fn unrelated_replace_rows_skip_word_level_emphasis() {
        let old_text = "alpha beta gamma\n";
        let new_text = "completely different text\n";
        let mut rows = side_by_side_diff_rows(old_text, new_text);
        attach_diff_highlights(&mut rows, old_text, new_text, "");

        assert!(
            rows[0]
                .new
                .highlights
                .iter()
                .all(|(_, style)| style.background_color.is_none()),
            "near-total rewrites should read as whole-line changes"
        );
    }

    #[test]
    fn pure_insertion_replace_rows_keep_word_level_emphasis() {
        let old_text = "return x\n";
        let new_text = "return x + 1\n";
        let mut rows = side_by_side_diff_rows(old_text, new_text);
        attach_diff_highlights(&mut rows, old_text, new_text, "");

        assert!(
            rows[0]
                .new
                .highlights
                .iter()
                .any(|(_, style)| style.background_color.is_some()),
            "an appended token should still be emphasized on the new side"
        );
        assert!(
            rows[0]
                .old
                .highlights
                .iter()
                .all(|(_, style)| style.background_color.is_none()),
            "the unchanged old side has nothing to emphasize"
        );
    }

    #[test]
    fn emphasis_subtlety_gate_boundary() {
        // Exactly 60% coverage passes; just over fails; empty side passes.
        // Use struct-literal form to avoid the single_range_in_vec_init Clippy lint.
        let r = |start, end| std::ops::Range::<usize> { start, end };
        assert!(emphasis_is_subtle(&[r(0, 6)], 10));
        assert!(!emphasis_is_subtle(&[r(0, 7)], 10));
        assert!(emphasis_is_subtle(&[], 0));
        assert!(!emphasis_is_subtle(&[r(0, 1)], 0));
    }

    #[test]
    fn diff_line_fill_uses_palette_diff_backgrounds() {
        use crate::theme::palette;
        let p = palette();
        assert_eq!(
            diff_line_fill(DiffLineStatus::Added),
            p.diff_added_bg.into()
        );
        assert_eq!(
            diff_line_fill(DiffLineStatus::Removed),
            p.diff_removed_bg.into()
        );
        assert_eq!(
            diff_line_fill(DiffLineStatus::Unchanged),
            p.background.into()
        );
    }

    #[test]
    fn empty_hatch_tiles_seamlessly_across_stacked_gap_rows() {
        use crate::theme::palette;

        assert_eq!(
            diff_line_fill(DiffLineStatus::Empty),
            pattern_slash(
                palette().diff_empty_hatch,
                DIFF_HATCH_STROKE,
                DIFF_HATCH_INTERVAL
            )
        );

        // gpui's slash pattern restarts at each row quad's own origin, so a
        // multi-row alignment gap shears at every row seam unless the pattern
        // period divides the row height in device pixels at every backing
        // scale the app renders at.
        let period = DIFF_HATCH_STROKE + DIFF_HATCH_INTERVAL;
        for scale in [1.0_f32, 1.5, 2.0] {
            assert_eq!(
                (DIFF_LINE_HEIGHT * scale) % period,
                0.0,
                "hatch period {period} must divide the device-pixel row height \
                 at {scale}x so stacked gap rows tile without a seam"
            );
        }
    }

    #[test]
    fn widest_line_chars_spans_both_sides() {
        let rows = side_by_side_diff_rows("short\n", "a much longer replacement line\n");
        assert_eq!(
            widest_line_chars(&rows),
            "a much longer replacement line".chars().count()
        );
    }

    #[test]
    fn widest_line_chars_of_no_rows_is_zero() {
        assert_eq!(widest_line_chars(&[]), 0);
    }

    #[test]
    fn prepared_diff_exposes_its_widest_line() {
        let content = repo::FileDiffContent::SideBySide {
            old_text: "tiny\n".to_string(),
            new_text: "a considerably wider line\n".to_string(),
        };
        let prepared = PreparedFileDiff::from_content(content, "");
        assert_eq!(
            prepared.max_line_chars(),
            "a considerably wider line".chars().count()
        );
    }

    #[test]
    fn single_side_prepared_diff_exposes_its_widest_line() {
        let content = repo::FileDiffContent::Single {
            side: repo::DiffSide::New,
            text: "short\na much longer added line\n".to_string(),
        };
        let prepared = PreparedFileDiff::from_content(content, "");
        assert_eq!(
            prepared.max_line_chars(),
            "a much longer added line".chars().count()
        );
    }

    #[test]
    fn widest_line_chars_breaks_ties_between_equal_sides() {
        let rows = side_by_side_diff_rows("same width A\n", "same width B\n");
        assert_eq!(widest_line_chars(&rows), "same width A".chars().count());
    }

    #[test]
    fn binary_prepared_diff_has_no_line_width() {
        let prepared = PreparedFileDiff::from_content(repo::FileDiffContent::Binary, "");
        assert_eq!(prepared.max_line_chars(), 0);
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_text_diff_content(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
    }

    #[gpui::test]
    async fn added_file_diff_renders_only_the_new_side(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open added file changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "hello.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Added);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read added file changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("added line row debug bounds");
        assert!(
            visual.debug_bounds("file-diff-side-old").is_none(),
            "added file diff should not render an empty old-side pane"
        );
    }

    #[gpui::test]
    async fn deleted_file_diff_renders_only_the_old_side(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_deleted_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open deleted file changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "obsolete.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Deleted);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read deleted file changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed line row debug bounds");
        assert!(
            visual.debug_bounds("file-diff-side-new").is_none(),
            "deleted file diff should not render an empty new-side pane"
        );
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_line_highlights(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed line row debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("added line row debug bounds");
    }

    #[gpui::test]
    async fn modified_file_diff_renders_status_accent_bars(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_python_change();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open python changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-accent-removed")
            .expect("removed accent bar debug bounds");
        visual
            .debug_bounds("file-diff-accent-added")
            .expect("added accent bar debug bounds");
    }

    #[gpui::test]
    async fn status_row_background_fills_the_full_side_width(cx: &mut TestAppContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_python_change();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open python changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        // A wide window makes each diff side far wider than its short code
        // lines, so a row background that only spans its text leaves a gap.
        visual.simulate_resize(size(px(1600.), px(600.)));

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        let old_side = visual
            .debug_bounds("file-diff-side-old")
            .expect("old diff side debug bounds");
        let new_side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new diff side debug bounds");
        let removed = visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed row debug bounds");
        let added = visual
            .debug_bounds("file-diff-row-added")
            .expect("added row debug bounds");

        // The removed row lives on the old side, the added row on the new side;
        // each background must reach its side's full width (less the reserved
        // scrollbar gutter), not stop at the code text.
        assert!(
            removed.size.width >= old_side.size.width - px(16.),
            "removed row background must fill the old side (row {:?} vs side {:?})",
            removed.size.width,
            old_side.size.width
        );
        assert!(
            added.size.width >= new_side.size.width - px(16.),
            "added row background must fill the new side (row {:?} vs side {:?})",
            added.size.width,
            new_side.size.width
        );
    }

    #[gpui::test]
    async fn empty_alignment_row_background_fills_the_full_side_width(cx: &mut TestAppContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_inserted_lines();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open inserted-lines changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(1600.), px(600.)));

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        let old_side = visual
            .debug_bounds("file-diff-side-old")
            .expect("old diff side debug bounds");
        let empty = visual
            .debug_bounds("file-diff-row-empty")
            .expect("empty alignment row debug bounds");

        // The inserted lines put hatched alignment gaps on the old side; the
        // hatch must span the side's full width so the gap reads as a filled
        // slashed block rather than a thin strip beside the gutter.
        assert!(
            empty.size.width >= old_side.size.width - px(16.),
            "empty alignment row must fill the old side (row {:?} vs side {:?})",
            empty.size.width,
            old_side.size.width
        );
    }

    #[gpui::test]
    async fn scrolling_long_file_diff_moves_the_diff_scroll_area(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let max_offset = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_max_offset(cx))
            .expect("read new diff scroll max offset");
        assert!(
            max_offset.height > px(0.),
            "long diff should exceed the visible diff scroll area"
        );

        let before = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset before wheel");
        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });
        let after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset after wheel");

        assert!(
            after.y < before.y,
            "wheel scroll should move the diff content upward"
        );
    }

    #[gpui::test]
    async fn scrolling_new_side_of_side_by_side_diff_scrolls_old_side(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let old_before = window
            .read_with(cx, |app, cx| app.file_diff_old_scroll_offset(cx))
            .expect("read old diff scroll offset before wheel");
        let new_before = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });

        let old_after = window
            .read_with(cx, |app, cx| app.file_diff_old_scroll_offset(cx))
            .expect("read old diff scroll offset after wheel");
        let new_after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset after wheel");

        assert!(
            new_after.y < new_before.y,
            "wheel scroll should move the new side upward"
        );
        assert_ne!(
            old_after.y, old_before.y,
            "old side should move when the new side scrolls"
        );
        assert_eq!(
            old_after.y, new_after.y,
            "old side should stay aligned with new side"
        );
    }

    #[gpui::test]
    async fn scrolling_old_side_of_side_by_side_diff_scrolls_new_side(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        let old_before = window
            .read_with(cx, |app, cx| app.file_diff_old_scroll_offset(cx))
            .expect("read old diff scroll offset before wheel");
        let new_before = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });

        let old_after = window
            .read_with(cx, |app, cx| app.file_diff_old_scroll_offset(cx))
            .expect("read old diff scroll offset after wheel");
        let new_after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset after wheel");

        assert!(
            old_after.y < old_before.y,
            "wheel scroll should move the old side upward"
        );
        assert_ne!(
            new_after.y, new_before.y,
            "new side should move when the old side scrolls"
        );
        assert_eq!(
            old_after.y, new_after.y,
            "new side should stay aligned with old side"
        );
    }

    #[gpui::test]
    async fn diff_view_virtualizes_offscreen_rows(cx: &mut TestAppContext) {
        use gpui::size;

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        // The diff rewrites 160 lines, so the content (>=160 rows x 20px) far
        // exceeds this 320px-tall viewport (~16 rows visible).
        visual.simulate_resize(size(px(700.), px(320.)));

        // The first row is on screen and must be materialized.
        visual
            .debug_bounds("file-diff-line-new-0")
            .expect("first diff row should be materialized");

        // Row 150 sits ~3000px down, far past the viewport, and must NOT be
        // built while off screen. That asymmetry is the proof of virtualization.
        assert!(
            visual.debug_bounds("file-diff-line-new-150").is_none(),
            "row 150 is far below the viewport and must not be materialized"
        );
    }

    /// The active tab's diff selection, if any — reads through the active
    /// pane and item key so tests don't repeat that lookup per assertion.
    fn read_active_selection(
        window: &WindowHandle<App>,
        cx: &TestAppContext,
    ) -> Option<diff_selection::DiffSelection> {
        window
            .read_with(cx, |app, _| {
                let pane = app.workspace.active_pane();
                let key = app.workspace.active_item(pane)?.key().to_string();
                app.diff_selection(pane, &key)
            })
            .unwrap()
    }

    /// The active tab's new-side cell text at `row`, for asserting copy
    /// output against the diff's own content instead of a hardcoded literal.
    fn read_active_row_text(window: &WindowHandle<App>, cx: &TestAppContext, row: usize) -> String {
        window
            .read_with(cx, |app, cx| {
                let (prepared, _scroll) = app.active_pane_prepared_diff(cx)?;
                let content = diff_selection::DiffSideContent::Prepared {
                    diff: prepared,
                    side: repo::DiffSide::New,
                };
                Some(content.cell(row).text.clone())
            })
            .unwrap()
            .expect("active pane has a prepared diff")
    }

    /// Open the three-block modified file in a single pane at a size that keeps
    /// the diff scrollable, returning the temp repo (kept alive by the caller),
    /// the window, and its visual context.
    fn open_multi_block_diff(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_multiple_change_blocks();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("blocks.txt".to_string(), cx);
            })
            .expect("open multi-block diff");

        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(360.)));
        cx.run_until_parked();

        (dir, window, visual)
    }

    // `blocks.txt`'s three changed lines are 5, 30, and 55 (1-indexed; see
    // `init_repo_with_multiple_change_blocks`). All lines above line 5 are
    // unchanged, so the first change block's replace of line 5 lands on flat
    // diff row index 4 (0-indexed) on both sides.
    #[gpui::test]
    async fn clicking_a_diff_line_places_the_caret(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let bounds = visual
            .debug_bounds("file-diff-code-new-4")
            .expect("row 4 code content");
        visual.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("click created a selection");
        assert!(selection.is_caret());
        assert_eq!(selection.head.row, 4);
        visual.debug_bounds("diff-caret").expect("caret painted");
        visual
            .debug_bounds("diff-active-line")
            .expect("active line tinted");
    }

    #[gpui::test]
    async fn caret_blinks_off_and_back(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let target = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row")
            .center();
        visual.simulate_click(target, Modifiers::default());
        cx.run_until_parked();
        visual
            .debug_bounds("diff-caret")
            .expect("caret visible after click");
        assert!(
            window
                .read_with(cx, |app, _| app.caret_blink_visible)
                .unwrap(),
            "caret starts solid immediately after placement"
        );

        // gpui's test harness only records that a `debug_selector` *has*
        // painted at some point in this window's lifetime — a selector that
        // stops painting keeps its last-recorded bounds rather than
        // clearing, so an "off" phase can't be proven by `debug_bounds`
        // absence. Assert against `caret_blink_visible` instead: it is the
        // exact value `render_file_diff_line`'s `if let Some(caret_x)` gate
        // reads (via `DiffSelectionContext::caret_visible`) to decide
        // whether to paint the caret at all.
        cx.executor()
            .advance_clock(CARET_BLINK_INTERVAL + std::time::Duration::from_millis(50));
        cx.run_until_parked();
        assert!(
            !window
                .read_with(cx, |app, _| app.caret_blink_visible)
                .unwrap(),
            "caret blinked off"
        );

        cx.executor().advance_clock(CARET_BLINK_INTERVAL);
        cx.run_until_parked();
        assert!(
            window
                .read_with(cx, |app, _| app.caret_blink_visible)
                .unwrap(),
            "caret blinked back on"
        );
        visual
            .debug_bounds("diff-caret")
            .expect("caret painted again once the blink phase returns to solid");
    }

    #[gpui::test]
    async fn unfocused_pane_dims_its_selection_and_hides_the_caret(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let pane_a = window
            .read_with(cx, |app, _| app.workspace.active_pane())
            .unwrap();

        // A multi-row range selection (rather than a bare caret) so the
        // newline stub — and therefore `diff-selection-dim` — actually
        // paints: a caret-only selection has no span to fill or dim.
        let start = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row 1")
            .center();
        let end = visual
            .debug_bounds("file-diff-code-new-3")
            .expect("row 3")
            .center();
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        visual
            .debug_bounds("diff-caret")
            .expect("caret visible while pane A is focused");
        assert!(
            visual.debug_bounds("diff-selection-dim").is_none(),
            "a focused pane's selection is not dimmed"
        );

        // Split so a second pane exists, then explicitly move keyboard focus
        // to it — `split_workspace_pane` makes the new pane active, which is
        // what actually moves gpui focus away from pane A.
        window
            .update(cx, |app, window, cx| {
                app.split_workspace_pane(pane_a, crate::workspace::SplitDirection::Right, cx);
                let pane_b = app.workspace.active_pane();
                assert_ne!(pane_b, pane_a, "split creates a distinct second pane");
                app.pane_scroll(pane_b, cx).focus.focus(window);
            })
            .unwrap();
        cx.run_until_parked();

        // `debug_bounds` only ever records that a selector *has* painted, so
        // the caret's disappearance is asserted against the same `focused`
        // read `render_file_diff_line` gates on, not against bounds absence
        // (see `caret_blinks_off_and_back` for the same caveat).
        let pane_a_focused = window
            .update(cx, |app, window, cx| {
                app.pane_scroll(pane_a, cx).focus.is_focused(window)
            })
            .unwrap();
        assert!(!pane_a_focused, "focus moved to pane B");
        visual
            .debug_bounds("diff-selection-dim")
            .expect("unfocused pane A dims its selection fill");
    }

    #[gpui::test]
    async fn clicking_the_other_side_moves_the_selection(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let new_side = visual
            .debug_bounds("file-diff-code-new-4")
            .expect("new side");
        visual.simulate_click(new_side.center(), Modifiers::default());
        cx.run_until_parked();
        let old_side = visual
            .debug_bounds("file-diff-code-old-4")
            .expect("old side");
        visual.simulate_click(old_side.center(), Modifiers::default());
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("selection exists");
        assert_eq!(selection.side, repo::DiffSide::Old);
    }

    #[gpui::test]
    async fn dragging_selects_text_across_rows(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let start = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row 1")
            .center();
        let end = visual
            .debug_bounds("file-diff-code-new-3")
            .expect("row 3")
            .center();
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("drag created a selection");
        let drag_cleared = window
            .read_with(cx, |app, _| app.diff_drag.is_none())
            .unwrap();
        assert!(!selection.is_caret());
        assert_eq!(selection.line_range(), 1..=3);
        assert!(drag_cleared, "mouse-up ended the drag");
    }

    #[gpui::test]
    async fn shift_click_extends_from_the_anchor(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let start = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row 1")
            .center();
        visual.simulate_click(start, Modifiers::default());
        cx.run_until_parked();
        let end = visual
            .debug_bounds("file-diff-code-new-3")
            .expect("row 3")
            .center();
        visual.simulate_click(
            end,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("selection exists");
        assert_eq!(selection.anchor.row, 1);
        assert_eq!(selection.head.row, 3);
    }

    #[gpui::test]
    async fn double_click_selects_the_word(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let target = visual
            .debug_bounds("file-diff-code-new-4")
            .expect("row")
            .center();
        visual.simulate_mouse_down(target, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        visual.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: target,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("selection");
        assert!(!selection.is_caret(), "double-click selected a word");
        assert_eq!(selection.head.row, 4);
        assert_eq!(selection.anchor.row, 4);
    }

    #[gpui::test]
    async fn dragging_a_word_selection_backward_keeps_the_head_on_the_pointer(
        cx: &mut TestAppContext,
    ) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let origin = visual
            .debug_bounds("file-diff-code-new-4")
            .expect("row 4")
            .center();
        visual.simulate_mouse_down(origin, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(origin, MouseButton::Left, Modifiers::default());
        visual.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: origin,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.run_until_parked();
        let earlier = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row 1")
            .center();
        visual.simulate_mouse_move(earlier, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("drag created a selection");
        assert_eq!(
            selection.head.row, 1,
            "head should track the pointer on a backward drag"
        );
        assert_eq!(
            selection.anchor.row, 4,
            "anchor should stay at the drag origin"
        );
    }

    #[gpui::test]
    async fn gutter_click_selects_the_whole_line(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let gutter = visual
            .debug_bounds("file-diff-line-new-4")
            .expect("gutter cell")
            .center();
        visual.simulate_click(gutter, Modifiers::default());
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("selection");
        assert_eq!(
            selection.anchor,
            crate::app::diff_selection::DiffPoint { row: 4, column: 0 }
        );
        assert_eq!(selection.head.row, 4);
        assert!(selection.head.column > 0, "head at line end");
    }

    /// Open the wide-lined modified file in a single pane, returning the temp
    /// repo (kept alive by the caller), the window, and its visual context.
    fn open_wide_diff(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_long_lines();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("wide.txt".to_string(), cx);
            })
            .expect("open wide diff");

        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        cx.run_until_parked();

        (dir, window, visual)
    }

    #[gpui::test]
    async fn horizontal_wheel_pans_the_diff_through_the_shared_offset(cx: &mut TestAppContext) {
        use gpui::{point, px, ScrollDelta, ScrollWheelEvent};

        let (_dir, window, mut visual) = open_wide_diff(cx);

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let before = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset before wheel");
        assert_eq!(before.x, px(0.), "a fresh diff starts unpanned");

        visual.simulate_event(ScrollWheelEvent {
            position: side.origin + point(px(100.), px(30.)),
            delta: ScrollDelta::Pixels(point(px(-240.), px(0.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let after = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset after wheel");
        assert!(after.x < before.x, "horizontal wheel should pan the diff");
    }

    #[gpui::test]
    async fn dragging_against_the_right_edge_autoscrolls_the_pan(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (_dir, window, mut visual) = open_wide_diff(cx);

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        // Press near the code cell's left edge: a wide cell's painted bounds
        // extend past the viewport, so its center may fall outside the side.
        let row = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row 1 code content");
        let start = point(row.left() + px(30.), row.center().y);
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        let before = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset before drag");
        assert_eq!(before.x, px(0.), "a fresh diff starts unpanned");

        // Pointer pressed against the side's right edge, vertically centered
        // so only the horizontal axis sits in its autoscroll margin.
        let edge = point(side.bottom_right().x - px(2.), side.center().y);
        visual.simulate_mouse_move(edge, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();

        let after = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset after drag");
        assert!(
            after.x < before.x,
            "dragging against the right edge should pan the diff toward the overflow"
        );
    }

    #[gpui::test]
    async fn panning_keeps_the_gutter_frozen_while_code_shifts(cx: &mut TestAppContext) {
        use gpui::{point, px, ScrollDelta, ScrollWheelEvent};

        let (_dir, _window, mut visual) = open_wide_diff(cx);

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let number_before = visual
            .debug_bounds("file-diff-line-new-1")
            .expect("number cell before pan");
        let code_before = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("code content before pan");
        let old_code_before = visual
            .debug_bounds("file-diff-code-old-1")
            .expect("old-side code content before pan");

        visual.simulate_event(ScrollWheelEvent {
            position: side.origin + point(px(100.), px(30.)),
            delta: ScrollDelta::Pixels(point(px(-240.), px(0.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let number_after = visual
            .debug_bounds("file-diff-line-new-1")
            .expect("number cell after pan");
        let code_after = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("code content after pan");

        assert_eq!(
            number_after.origin.x, number_before.origin.x,
            "line numbers stay frozen at the left edge"
        );
        assert!(
            code_after.origin.x < code_before.origin.x,
            "code content pans under the gutter"
        );

        let old_code_after = visual
            .debug_bounds("file-diff-code-old-1")
            .expect("old-side code content after pan");
        assert_eq!(
            old_code_after.origin.x - old_code_before.origin.x,
            code_after.origin.x - code_before.origin.x,
            "the old side pans in lockstep with the new side"
        );
    }

    #[gpui::test]
    async fn short_lines_do_not_pan(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_lines();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("narrow.txt".to_string(), cx);
            })
            .expect("open narrow diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        cx.run_until_parked();

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual.simulate_event(ScrollWheelEvent {
            position: side.origin + point(px(100.), px(10.)),
            delta: ScrollDelta::Pixels(point(px(-240.), px(0.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let offset = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset");
        assert_eq!(
            offset.x,
            px(0.),
            "a diff whose lines fit clamps the pan back to zero"
        );
    }

    #[gpui::test]
    async fn vertical_wheel_scrolls_rows_without_panning(cx: &mut TestAppContext) {
        use gpui::{point, px, ScrollDelta, ScrollWheelEvent};

        let (_dir, window, mut visual) = open_wide_diff(cx);

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let vertical_before = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read vertical offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: side.origin + point(px(100.), px(30.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-120.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let vertical_after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read vertical offset after wheel");
        let pan = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset");

        assert!(
            vertical_after.y < vertical_before.y,
            "vertical wheel over a code cell still scrolls the rows"
        );
        assert_eq!(
            pan.x,
            px(0.),
            "a purely vertical wheel must not leak into a horizontal pan"
        );
    }

    #[gpui::test]
    async fn opening_another_file_resets_the_pan(cx: &mut TestAppContext) {
        use gpui::{point, px, ScrollDelta, ScrollWheelEvent};

        let (_dir, window, mut visual) = open_wide_diff(cx);

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual.simulate_event(ScrollWheelEvent {
            position: side.origin + point(px(100.), px(30.)),
            delta: ScrollDelta::Pixels(point(px(-240.), px(0.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let panned = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset while panned");
        assert!(panned.x < px(0.), "the wide diff panned");

        window
            .update(cx, |app, _window, cx| {
                app.open_file_preview("narrow.txt".to_string(), cx);
            })
            .expect("open narrow file");
        cx.run_until_parked();

        let reset = window
            .read_with(cx, |app, cx| app.file_diff_hscroll_offset(cx))
            .expect("read pan offset after switching files");
        assert_eq!(reset.x, px(0.), "opening a file starts it unpanned");
    }

    #[gpui::test]
    async fn change_block_footer_reports_the_current_position(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        visual
            .debug_bounds("change-block-footer")
            .expect("change-block footer");
        visual
            .debug_bounds("change-block-label")
            .expect("change-block label");

        let position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position");
        assert_eq!(
            position,
            Some((0, 3)),
            "a fresh diff sits on the first of three blocks"
        );
    }

    #[gpui::test]
    async fn clicking_next_change_block_advances_and_scrolls(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        let before = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read offset before");

        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position");
        assert_eq!(position, Some((1, 3)), "next advances to the second block");

        let after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read offset after");
        assert!(
            after.y < before.y,
            "advancing to the next block scrolls the diff down"
        );
    }

    #[gpui::test]
    async fn clicking_previous_change_block_wraps_to_the_last(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        let prev = visual
            .debug_bounds("change-block-prev")
            .expect("previous button");
        visual.simulate_click(prev.center(), Modifiers::none());
        cx.run_until_parked();

        let position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position");
        assert_eq!(
            position,
            Some((2, 3)),
            "previous from the first block wraps to the last"
        );
    }

    #[gpui::test]
    async fn change_block_keybindings_navigate_the_active_diff(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        visual.simulate_keystrokes("alt-cmd-down");
        cx.run_until_parked();
        let after_next = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position after alt-cmd-down");
        assert_eq!(after_next, Some((1, 3)), "alt-cmd-down advances a block");

        visual.simulate_keystrokes("alt-cmd-up");
        cx.run_until_parked();
        let after_prev = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position after alt-cmd-up");
        assert_eq!(after_prev, Some((0, 3)), "alt-cmd-up steps back a block");
    }

    #[gpui::test]
    async fn opening_a_diff_scrolls_to_its_first_change_block(cx: &mut TestAppContext) {
        use gpui::px;

        let (_dir, window, _visual) = open_multi_block_diff(cx);

        let offset = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read offset");
        assert!(
            offset.y < px(0.),
            "a freshly opened diff auto-scrolls down to its first change block"
        );
    }

    #[gpui::test]
    async fn stepping_next_from_the_top_lands_on_the_first_block_without_skipping(
        cx: &mut TestAppContext,
    ) {
        use gpui::{point, px, ScrollDelta, ScrollWheelEvent};

        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        // Scroll back above the first change, to the very top of the file.
        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side");
        visual.simulate_event(ScrollWheelEvent {
            position: side.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(480.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let at_top = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read offset at top");
        assert_eq!(at_top.y, px(0.), "scrolled to the very top of the file");
        let at_top_position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position at top");
        assert_eq!(
            at_top_position,
            Some((0, 3)),
            "the counter still points at the first block while above it"
        );

        // Next must bring the first block into view, not skip to the second.
        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read offset after next");
        assert!(
            after.y < px(0.),
            "next from the top scrolls down into the first block"
        );
        let after_position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position after next");
        assert_eq!(
            after_position,
            Some((0, 3)),
            "next stepped onto the first block rather than skipping it"
        );
    }

    #[gpui::test]
    async fn binary_diff_hides_the_change_block_footer(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_binary_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open binary changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        visual
            .debug_bounds("file-diff-binary")
            .expect("binary diff placeholder");
        assert!(
            visual.debug_bounds("change-block-footer").is_none(),
            "a binary diff has no change blocks and shows no footer"
        );
    }

    #[gpui::test]
    async fn hovering_a_wide_diff_reveals_the_horizontal_scrollbar(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (_dir, _window, mut visual) = open_wide_diff(cx);

        assert!(
            visual
                .debug_bounds("file-diff-side-new-hscrollbar")
                .is_none(),
            "no scrollbar before the pointer enters the pane"
        );

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual.simulate_mouse_move(
            side.origin + point(px(100.), px(30.)),
            None,
            Modifiers::default(),
        );
        cx.run_until_parked();

        assert!(
            visual
                .debug_bounds("file-diff-side-new-hscrollbar")
                .is_some(),
            "hovering the pane reveals the new side's scrollbar"
        );
        assert!(
            visual
                .debug_bounds("file-diff-side-old-hscrollbar")
                .is_some(),
            "both sides reveal their bars together"
        );

        visual.simulate_mouse_move(point(px(-10.), px(-10.)), None, Modifiers::default());
        cx.run_until_parked();

        assert!(
            visual
                .debug_bounds("file-diff-side-new-hscrollbar")
                .is_none(),
            "leaving the pane hides the scrollbar"
        );
    }

    #[gpui::test]
    async fn short_diff_shows_no_horizontal_scrollbar_on_hover(cx: &mut TestAppContext) {
        use gpui::{point, px, size};

        let (dir, oid_hex) = init_repo_with_long_lines();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("narrow.txt".to_string(), cx);
            })
            .expect("open narrow diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        cx.run_until_parked();

        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual.simulate_mouse_move(
            side.origin + point(px(100.), px(10.)),
            None,
            Modifiers::default(),
        );
        cx.run_until_parked();

        assert!(
            visual
                .debug_bounds("file-diff-side-new-hscrollbar")
                .is_none(),
            "a diff with no horizontal overflow never shows the bar"
        );
    }

    #[gpui::test]
    async fn arrow_keys_move_the_caret_and_shift_extends(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let target = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("row")
            .center();
        visual.simulate_click(target, Modifiers::default());
        cx.run_until_parked();
        let before = read_active_selection(&window, cx).expect("caret");
        visual.simulate_keystrokes("down");
        cx.run_until_parked();
        let after = read_active_selection(&window, cx).expect("caret moved");
        assert!(after.head.row > before.head.row);
        assert!(after.is_caret());
        visual.simulate_keystrokes("shift-down shift-right");
        cx.run_until_parked();
        let extended = read_active_selection(&window, cx).expect("selection");
        assert!(!extended.is_caret());
        assert_eq!(
            extended.anchor, after.head,
            "anchor stayed put while shift extended"
        );
    }

    #[gpui::test]
    async fn cmd_c_copies_the_selected_text(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        let gutter = visual
            .debug_bounds("file-diff-line-new-2")
            .expect("gutter")
            .center();
        visual.simulate_click(gutter, Modifiers::default()); // whole line 2
        cx.run_until_parked();
        visual.simulate_keystrokes("cmd-c");
        cx.run_until_parked();
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("clipboard text");
        let expected = read_active_row_text(&window, cx, 2);
        assert_eq!(copied, expected);
    }

    #[gpui::test]
    async fn selection_keys_do_nothing_before_the_first_click(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_keystrokes("cmd-c");
        cx.run_until_parked();
        assert!(
            read_active_selection(&window, cx).is_none(),
            "no caret, no selection"
        );
    }

    #[gpui::test]
    async fn block_navigation_moved_to_alt_cmd(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_multi_block_diff(cx);
        visual.simulate_keystrokes("alt-cmd-down");
        cx.run_until_parked();
        let position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .unwrap();
        assert_eq!(
            position,
            Some((1, 3)),
            "alt-cmd-down stepped to the second block"
        );
    }

    /// Open `wide.txt` (a short line 0 and a ~400-char line 1) in a single pane
    /// with soft wrap ON, returning the temp repo (kept alive by the caller),
    /// the window, and its visual context.
    fn open_wrapped_wide_diff(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_long_lines();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.set_diff_soft_wrap(true, cx);
                app.open_file_preview("wide.txt".to_string(), cx);
            })
            .expect("open wide diff with wrap on");

        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        cx.run_until_parked();

        (dir, window, visual)
    }

    #[gpui::test]
    async fn soft_wrap_hides_horizontal_scrollbar(cx: &mut TestAppContext) {
        let (_dir, _window, mut visual) = open_wrapped_wide_diff(cx);

        // Wrapped code content renders on the new side...
        visual
            .debug_bounds("file-diff-code-new-1")
            .expect("wrapped code content present");
        // ...and hovering the pane must not reveal a horizontal scrollbar,
        // because wrapped rows never overflow horizontally.
        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual.simulate_mouse_move(
            side.origin + gpui::point(px(100.), px(30.)),
            None,
            Modifiers::default(),
        );
        cx.run_until_parked();
        assert!(
            visual
                .debug_bounds("file-diff-side-new-hscrollbar")
                .is_none(),
            "wrap mode shows no horizontal scrollbar"
        );
    }

    #[gpui::test]
    async fn soft_wrap_grows_a_long_lines_row(cx: &mut TestAppContext) {
        let (_dir, _window, mut visual) = open_wrapped_wide_diff(cx);

        // `wide.txt` line 1 (row index 1) is ~400 chars and wraps to several
        // visual rows; line 0 stays short and occupies a single row. The
        // wrapped long row must therefore be taller than the short one.
        let short = visual
            .debug_bounds("file-diff-code-new-0")
            .expect("short row code content");
        let long = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("long row code content");
        assert!(
            long.size.height > short.size.height,
            "a wrapped long line must be taller than a short line \
             (long {:?} vs short {:?})",
            long.size.height,
            short.size.height
        );
    }

    // A click on the SECOND visual row of the wrapped long line must map to a
    // byte column past the first visual row's end — proving the click's y (not
    // just its x) drives the wrap-aware mapping. `wide.txt` row 1 is ~400 chars
    // and wraps to several visual rows in the 700px pane.
    #[gpui::test]
    async fn soft_wrap_click_maps_to_second_visual_row(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (_dir, window, mut visual) = open_wrapped_wide_diff(cx);
        let long = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("long row code content");

        // A click near the left edge of the first visual row lands at (or near)
        // column 0; the same x on the second visual row must land later.
        let first_row = point(long.origin.x + px(12.), long.origin.y + px(3.));
        visual.simulate_click(first_row, Modifiers::default());
        cx.run_until_parked();
        let first = read_active_selection(&window, cx).expect("first-row click selects");
        assert_eq!(first.head.row, 1, "click stayed on the wrapped line's row");

        let second_row = point(
            long.origin.x + px(12.),
            long.origin.y + px(DIFF_LINE_HEIGHT + 3.),
        );
        visual.simulate_click(second_row, Modifiers::default());
        cx.run_until_parked();
        let second = read_active_selection(&window, cx).expect("second-row click selects");
        assert_eq!(second.head.row, 1, "still the same wrapped line");
        assert!(
            second.head.column > first.head.column,
            "a click on the second visual row maps past the first row's column \
             (second {} vs first {})",
            second.head.column,
            first.head.column
        );
        assert!(
            second.head.column > 0,
            "the wrapped continuation is not column 0"
        );
    }

    // Selecting the whole wrapped long line via its gutter number fills the
    // wrapped content: the selection spans row 1 from column 0 to line end, and
    // the row still renders its (now background-overlaid) code content.
    #[gpui::test]
    async fn soft_wrap_selection_covers_wrapped_line(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_wrapped_wide_diff(cx);
        let gutter = visual
            .debug_bounds("file-diff-line-new-1")
            .expect("gutter cell for the long line")
            .center();
        visual.simulate_click(gutter, Modifiers::default());
        cx.run_until_parked();

        let selection = read_active_selection(&window, cx).expect("gutter click selects the line");
        assert!(!selection.is_caret(), "a whole-line select is a range");
        assert_eq!(
            selection.anchor,
            crate::app::diff_selection::DiffPoint { row: 1, column: 0 }
        );
        assert_eq!(selection.head.row, 1);
        assert!(
            selection.head.column > 0,
            "the head sits at the wrapped line's end"
        );
        // The wrapped row still renders its code content with the selection fill
        // overlaid on the text bytes.
        visual
            .debug_bounds("file-diff-code-new-1")
            .expect("wrapped code content still present under the fill");
    }

    // A caret placed mid-line on the wrapped long line paints below the first
    // visual row — proving the caret's y comes from the wrap-aware mapping.
    #[gpui::test]
    async fn soft_wrap_caret_paints_on_wrapped_row(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (_dir, _window, mut visual) = open_wrapped_wide_diff(cx);
        let long = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("long row code content");

        // Click on the second visual row, then let a frame paint so the caret
        // reads the captured bounds and lands at the head's wrapped position.
        let second_row = point(
            long.origin.x + px(40.),
            long.origin.y + px(DIFF_LINE_HEIGHT + 3.),
        );
        visual.simulate_click(second_row, Modifiers::default());
        cx.run_until_parked();

        let caret = visual
            .debug_bounds("diff-caret")
            .expect("caret paints on the wrapped row");
        assert!(
            caret.origin.y >= long.origin.y + px(DIFF_LINE_HEIGHT - 2.),
            "the caret sits on the second visual row or lower \
             (caret y {:?} vs first-row top {:?})",
            caret.origin.y,
            long.origin.y
        );
    }

    // A drag from the first visual row to the second visual row of the wrapped
    // long line extends a character-mode selection whose head lands past the
    // anchor — the wrap-aware drag path resolves the moving pointer to a
    // row+column via the captured per-row bounds.
    #[gpui::test]
    async fn soft_wrap_drag_extends_within_wrapped_line(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (_dir, window, mut visual) = open_wrapped_wide_diff(cx);
        let long = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("long row code content");

        let start = point(long.origin.x + px(12.), long.origin.y + px(3.));
        let end = point(
            long.origin.x + px(80.),
            long.origin.y + px(DIFF_LINE_HEIGHT + 3.),
        );
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();

        let selection = read_active_selection(&window, cx).expect("drag created a selection");
        assert!(!selection.is_caret(), "the drag produced a range");
        assert_eq!(selection.anchor.row, 1, "anchor stayed on the wrapped line");
        assert_eq!(selection.head.row, 1, "head stayed on the wrapped line");
        assert!(
            selection.head.column > selection.anchor.column,
            "the drag extended past the anchor onto the second visual row \
             (head {} vs anchor {})",
            selection.head.column,
            selection.anchor.column
        );
        let drag_cleared = window
            .read_with(cx, |app, _| app.diff_drag.is_none())
            .unwrap();
        assert!(drag_cleared, "mouse-up ended the drag");
    }

    fn open_wrapped_multi_block_diff(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_wrapped_change_blocks();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.set_diff_soft_wrap(true, cx);
                app.open_file_preview("wrapped_blocks.txt".to_string(), cx);
            })
            .expect("open wrapped multi-block diff");

        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(360.)));
        cx.run_until_parked();

        (dir, window, visual)
    }

    #[gpui::test]
    async fn soft_wrap_focus_first_change_block_on_open(cx: &mut TestAppContext) {
        let (_dir, window, _visual) = open_wrapped_multi_block_diff(cx);

        // `wrapped_blocks.txt`'s first change is at line 5 (row index 4), far
        // enough below the top that landing on it must scroll the wrap list off
        // row 0. The counter reports the first block.
        let position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position on open");
        assert_eq!(
            position,
            Some((0, 2)),
            "a freshly opened wrapped diff sits on the first of two blocks"
        );

        let top = window
            .read_with(cx, |app, cx| app.file_diff_wrap_topmost_row(cx))
            .expect("read wrap topmost row")
            .expect("wrap topmost row present");
        assert!(
            top > 0,
            "opening a wrapped diff scrolls past the top to reveal the first \
             change block (topmost row {top})"
        );
    }

    #[gpui::test]
    async fn soft_wrap_next_change_block_advances(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_wrapped_multi_block_diff(cx);

        let before = window
            .read_with(cx, |app, cx| app.file_diff_wrap_topmost_row(cx))
            .expect("read wrap topmost row before")
            .expect("wrap topmost row before present");

        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let position = window
            .read_with(cx, |app, cx| app.active_diff_block_position(cx))
            .expect("read block position");
        assert_eq!(
            position,
            Some((1, 2)),
            "next advances to the second block in wrap mode"
        );

        let after = window
            .read_with(cx, |app, cx| app.file_diff_wrap_topmost_row(cx))
            .expect("read wrap topmost row after")
            .expect("wrap topmost row after present");
        assert!(
            after > before,
            "advancing to the next block scrolls the wrapped diff down \
             (topmost row {before} -> {after})"
        );
    }

    #[gpui::test]
    async fn soft_wrap_change_block_counter_reports_position(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_wrapped_multi_block_diff(cx);

        // Advance, then step back, asserting the counter tracks the current
        // block in the same frame each time (no lag from a deferred scroll).
        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            window
                .read_with(cx, |app, cx| app.active_diff_block_position(cx))
                .expect("read block position after next"),
            Some((1, 2)),
            "the counter reports the second block after advancing"
        );

        let prev = visual
            .debug_bounds("change-block-prev")
            .expect("previous button");
        visual.simulate_click(prev.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            window
                .read_with(cx, |app, cx| app.active_diff_block_position(cx))
                .expect("read block position after previous"),
            Some((0, 2)),
            "the counter reports the first block after stepping back"
        );
    }

    // An added file renders through the single-side wrap path
    // (`render_wrapped_single`), which no modified-file fixture exercises —
    // every modified diff renders side-by-side. Opening `hello.txt` (added)
    // with wrap ON must render the new-side code cell and, crucially, remain
    // interactive: a click on that cell must place a caret, proving the
    // single-side path's per-row mouse-down wiring runs.
    #[gpui::test]
    async fn soft_wrap_single_side_added_file_renders_and_selects(cx: &mut TestAppContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.set_diff_soft_wrap(true, cx);
                app.open_file_preview("hello.txt".to_string(), cx);
            })
            .expect("open added file with wrap on");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        cx.run_until_parked();

        // The single-side wrap path renders only the new side.
        let cell = visual
            .debug_bounds("file-diff-code-new-0")
            .expect("single-side wrap renders the new-side code cell");
        assert!(
            visual.debug_bounds("file-diff-side-old").is_none(),
            "an added file's single-side wrap renders no old-side pane"
        );

        // Clicking the cell must place a caret — the single-side path's
        // mouse-down wiring is live, not just its rendering.
        visual.simulate_click(cell.center(), Modifiers::default());
        cx.run_until_parked();
        let selection = read_active_selection(&window, cx).expect("click placed a selection");
        assert_eq!(selection.side, repo::DiffSide::New);
        assert_eq!(selection.head.row, 0);
        visual
            .debug_bounds("diff-caret")
            .expect("caret paints on the single-side wrapped row");
    }

    // Toggling wrap ON while a file is already open must rewrap in place: the
    // wrap `ListState` count resyncs from 0 (never populated in nowrap mode) to
    // the row count on the mode flip. Every other wrap test sets wrap before
    // opening, so this is the only coverage of the live toggle.
    #[gpui::test]
    async fn toggling_soft_wrap_on_an_open_file_rewraps(cx: &mut TestAppContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_long_lines();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        // Open in the DEFAULT (nowrap) mode — no `set_diff_soft_wrap`.
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("wide.txt".to_string(), cx);
            })
            .expect("open wide diff in nowrap mode");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        cx.run_until_parked();

        // In nowrap mode the long row is a single fixed-height line and the
        // pane can overflow horizontally.
        let nowrap_long = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("nowrap renders the long row's code cell");
        assert!(
            (nowrap_long.size.height - px(DIFF_LINE_HEIGHT)).abs() < px(1.),
            "a nowrap row is one line tall (got {:?})",
            nowrap_long.size.height
        );

        // Flip to wrap mode on the already-open file.
        window
            .update(cx, |app, _window, cx| {
                app.set_diff_soft_wrap(true, cx);
            })
            .expect("toggle soft wrap on");
        cx.run_until_parked();

        // The same long row now wraps to several visual rows, so it is taller
        // than one line — proving the wrap list resynced its count and remeasured.
        let wrapped_long = visual
            .debug_bounds("file-diff-code-new-1")
            .expect("wrap renders the long row's code cell after the toggle");
        assert!(
            wrapped_long.size.height > px(DIFF_LINE_HEIGHT),
            "the long row rewrapped taller than one line after toggling wrap on \
             (got {:?})",
            wrapped_long.size.height
        );
    }

    // The first toggle from nowrap to wrap must land the same row at the top,
    // not jump to the file's top. This is the reported bug: nowrap stores scroll
    // as a pixel offset on a uniform-list handle, wrap as an item index on a
    // separate `ListState`, and the first wrap render's count-sync guard used to
    // `reset` (and null) the wrap scroll — so the position was lost. The toggle
    // path now syncs the count and seeds the row before that render runs.
    #[gpui::test]
    async fn toggling_soft_wrap_preserves_scroll_position_nowrap_to_wrap(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        // Scroll well down the file by advancing to the last change block; the
        // topmost row is then unambiguously below the file's top.
        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let before = window
            .read_with(cx, |app, cx| app.file_diff_nowrap_topmost_row(cx))
            .expect("read nowrap topmost row before toggle")
            .expect("nowrap topmost row present");
        assert!(
            before > 0,
            "the test must scroll below the top before toggling (row {before})"
        );

        // First toggle into wrap mode — the buggy path reset this to 0.
        window
            .update(cx, |app, _window, cx| app.set_diff_soft_wrap(true, cx))
            .expect("toggle soft wrap on");
        cx.run_until_parked();

        let after = window
            .read_with(cx, |app, cx| app.file_diff_wrap_topmost_row(cx))
            .expect("read wrap topmost row after toggle")
            .expect("wrap topmost row present");
        assert!(
            after.abs_diff(before) <= 1,
            "toggling wrap on preserves the topmost row (nowrap {before} -> wrap {after})"
        );
    }

    // The symmetric direction: starting in wrap mode scrolled below the top,
    // toggling wrap OFF must keep the same row at the top of the nowrap view.
    // Opens the three-block file directly in wrap mode and navigates to the
    // MIDDLE block, so the target row sits inside both modes' scroll ranges —
    // near the file bottom the shorter nowrap content clamps earlier than
    // wrap's rows, which is expected and would mask the preservation tested
    // here.
    #[gpui::test]
    async fn toggling_soft_wrap_preserves_scroll_position_wrap_to_nowrap(cx: &mut TestAppContext) {
        use gpui::{px, size};

        let (_dir, oid_hex) = init_repo_with_multiple_change_blocks();
        let path = _dir.path().to_path_buf();
        let window = add_app_window(cx);

        // Open the file with wrap ON from the start, so the wrap scroll is
        // established by wrap's own first-change focus — not by the nowrap ->
        // wrap translation this test must stay independent of.
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.set_diff_soft_wrap(true, cx);
                app.open_file_preview("blocks.txt".to_string(), cx);
            })
            .expect("open three-block diff in wrap mode");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(360.)));
        cx.run_until_parked();

        // Advance to the middle of three blocks: well below row 0 but far from
        // the file bottom, so the row is representable in nowrap too.
        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let before = window
            .read_with(cx, |app, cx| app.file_diff_wrap_topmost_row(cx))
            .expect("read wrap topmost row before toggle")
            .expect("wrap topmost row present");
        assert!(
            before > 1,
            "the test must scroll well below the top before toggling (row {before})"
        );

        // Toggle wrap OFF; the nowrap view must show the same topmost row.
        window
            .update(cx, |app, _window, cx| app.set_diff_soft_wrap(false, cx))
            .expect("toggle soft wrap off");
        cx.run_until_parked();

        let after = window
            .read_with(cx, |app, cx| app.file_diff_nowrap_topmost_row(cx))
            .expect("read nowrap topmost row after toggle")
            .expect("nowrap topmost row present");
        assert!(
            after.abs_diff(before) <= 1,
            "toggling wrap off preserves the topmost row (wrap {before} -> nowrap {after})"
        );
    }

    // A binary file has no rows to wrap; wrap mode must fall through to the same
    // placeholder the nowrap path shows, not attempt to build a wrap list.
    #[gpui::test]
    async fn soft_wrap_binary_file_shows_placeholder(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_binary_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.set_diff_soft_wrap(true, cx);
            })
            .expect("open binary changeset with wrap on");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        visual
            .debug_bounds("file-diff-binary")
            .expect("binary diff shows its placeholder in wrap mode");
    }
}
