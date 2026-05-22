# Zed Editor Crate Architecture & Feasibility Study for Greviewer

## TL;DR

**File Viewer (Read-Only, Syntax-Highlighted):** Recommend **Option A: Build from scratch on gpui**. The editor crate's value lies entirely in LSP integration, real-time collaboration, and complex interaction state (completions, code actions, multi-cursor editing). For a read-only display, you need ≈5% of the editor and would inherit substantial complexity. A minimal renderer using gpui's `ShapedLine` + `DisplayMap`-like wrapping logic (≈2–4 weeks).

**Side-by-Side Diff Viewer:** Recommend **Option B: Selective vendor of DisplayMap + BufferDiff**. Zed's split editor shows two synchronized `Editor` instances; for diffs, vendoring the `display_map` subsystem (layout + wrapping, ≈800 lines clean) + `buffer_diff` crate (diff hunks, ≈4k lines) gives you structured diffs without full editor complexity. Estimate ≈3–6 weeks integration + testing.

---

## 1. Crate Size & Shape

### Editor Crate

- **Total lines:** 148,637 across 52+ files
- **Top modules:**
  - `editor.rs` (1,100+ lines): Main `Editor` struct, vast state machine for editing, selections, completion, hover
  - `element.rs` (14,199 lines): Rendering pipeline; layout, text prep, gutter, diff hunks, paint
  - `display_map.rs` (14,428 lines across 9 files): Coordinate transformations, wrapping, folding, inlay hints
  - `scroll.rs`, `selection.rs`, `selections_collection.rs`: Scroll/selection management
  - `git.rs` (dir): Diff rendering, blame, hunk tracking
  - `inlays.rs`, `hover_popover.rs`, `code_actions.rs`, `completions.rs`: LSP-driven features

### Supporting Crates

| Crate | Lines | Purpose |
|-------|-------|---------|
| `language` | 19,846 | Syntax highlighting via Tree-sitter, language registry, LSP bindings |
| `multi_buffer` | 16,289 | Multi-file buffer abstraction, anchors, excerpts |
| `buffer_diff` | 4,036 | Diff hunks, line/word diff, secondary diffs (staged/unstaged) |
| `gpui-component` (vendor) | 58,022 | UI component library (Apache-2.0); includes text view, highlighter |

**Scale message:** Building from Zed's editor means inheriting ≈190k lines of production code across 5 crates. You'll drag in collaboration, LSP, completions, hover, code actions, settings, diagnostics, and editing state machines. None of those are needed for read-only views.

---

## 2. Architectural Seams

### Major Subsystems in the Editor Crate

| Subsystem | Files | 1-Line Purpose |
|-----------|-------|-----------------|
| **Core Editor** | `editor.rs` | 1,100-line struct holding buffer, display_map, selections, all editing/LSP state |
| **Display Map** | `display_map/*.rs` (9 files, 14.4k lines) | Layered coordinate transforms: Inlay → Fold → Tab → Wrap → Block → Highlights |
| **Rendering** | `element.rs` (14.2k lines) | Prepaint & paint: text layout, syntax highlight application, gutter, diff hunks, selection |
| **Scroll/Selection** | `scroll.rs`, `selection.rs`, `selections_collection.rs` | Scroll anchors, cursor tracking, multi-selection state |
| **Buffer/Multi-Buffer** | (in `language`/`multi_buffer` crates) | Rope-based text storage, multi-file excerpts, undo/redo |
| **Syntax Highlighting** | (in `language` crate) | Tree-sitter integration, highlight map, semantic tokens |
| **Diff Rendering** | `git.rs` (dir with blame.rs, etc.) | Diff hunks, blame, inline annotations |
| **Inlays** | `inlays.rs`, `inlay_map` (in display_map) | Inlay hints (LSP), rendering, placement |
| **Completions** | `completions.rs`, `code_context_menus.rs` | Completion menu, filtering, insertion |
| **Code Actions** | `code_actions.rs`, `code_context_menus.rs` | LSP code action menu, codebase-aware fixes |
| **Hover** | `hover_popover.rs`, `hover_links.rs` | LSP hover, markdown rendering, link detection |
| **Diagnostics** | `diagnostics.rs` | Inline error/warning display, gutter indicators |
| **Bookmarks/Breakpoints** | `bookmarks.rs` (ref to stores) | Gutter marker state |
| **Editing Machinery** | `input.rs`, `edit_prediction.rs`, `movement.rs` | Text insertion, deletion, autoclose, edit prediction |

### Display Map Layer Hierarchy

The display map is a **stack of coordinate-space transforms** (each layer builds on the one below):

```
Layer 0: BufferSnapshot (raw text)
  ↓
InlayMap (inject hint text)
  ↓
FoldMap (hide folded ranges)
  ↓
TabMap (expand tabs to spaces for display)
  ↓
WrapMap (soft-wrap long lines)
  ↓
BlockMap (inject custom blocks: diagnostics, sticky headers)
  ↓
DisplayMap (apply background highlights, coordinate the whole stack)
```

Each layer has a `Snapshot` type capturing state, `Transform` sum types describing mappings, and a `TransformSummary` (`input: TextSummary, output: TextSummary`). To convert a buffer point to a display point, you traverse down: `BufferPoint → InlayPoint → FoldPoint → TabPoint → WrapPoint → BlockPoint → DisplayPoint`.

---

## 3. Read-Only Display Path: From Text String to Screen

### Zed's Current Path (Complex, Full-Featured)

```
Editor (state machine for everything)
  ↓
MultiBufferSnapshot (excerpts, multi-file)
  ↓
DisplaySnapshot (display_map.snapshot())
  ↓
EditorElement::request_layout()
  ↓
EditorElement::prepaint() → line layout
  ↓
prepaint_lines() → ShapedLine per line
  ↓
EditorElement::paint() → fills hitbox with text + selections + gutter
```

### Minimum for Read-Only Syntax-Highlighted View

1. **Text storage:** Simple `Arc<str>` or `ropey::Rope` (if multi-MB files + incremental rendering needed)
2. **Syntax highlighting:** Tree-sitter parse → query ranges → map to highlight theme → `HighlightStyle` tuples
3. **Layout:** Render each line:
   - Compute visual width (tabs → spaces, soft-wrap if enabled)
   - Build `TextRun` array with highlight styles
   - Call `gpui::ShapedLine::new(text_runs)` to shape + layout
4. **Scroll & viewport:** Track `(row_offset, column_offset)` as user scrolls; only render visible lines
5. **Selection & copy:** Store `(start_offset, end_offset)` range; apply selection highlight on render
6. **Gutter (line numbers):** Optional, per-line counter

### Key Types Needed

- `ShapedLine` (from `gpui::`): Shaped text with metrics, ready to paint
- `TextRun { text, style }` (from `gpui::`)
- `HighlightStyle` (from `gpui::`): Color, font weight, italic, underline
- Custom: `LineLayout { shaped_line: ShapedLine, row: u32, width: f32 }`

### What You Can Skip

- `Editor` struct (editing state machine)
- `MultiBuffer` (unless you want multi-file view)
- `selections_collection`, `selection.rs` (you just need one selection range)
- `input.rs`, `edit_prediction.rs` (you're read-only)
- `completions.rs`, `hover_popover.rs`, `code_actions.rs`
- All LSP integration

---

## 4. Syntax Highlighting

### Where It Lives

**File:** `/Users/jellison/code/zed/crates/language/src/language.rs` (19.8k lines)

The `language` crate provides:
1. **`Language` struct:** Tree-sitter grammar, queries (highlights, locals, injections)
2. **`LanguageRegistry`:** Global registry mapping file ext → `Language`
3. **`SyntaxMap`:** Per-buffer parse tree cache + incremental updates
4. **`HighlightMap`:** Mapping highlight query captures → theme colors

### Tree-Sitter Integration

```rust
// Pseudo-pseudocode
let parser = tree_sitter::Parser::new();
let tree = parser.parse(source_code, None)?;

let query = Query::new(language.highlights_query)?;
let mut cursor = QueryCursor::new();

for match in cursor.captures(&query, tree.root_node(), source_bytes) {
    for (node, cap_idx) in match.captures {
        let highlight_name = query.capture_names()[cap_idx];
        let offset = node.start_byte()..node.end_byte();
        highlights.push((offset, highlight_map[highlight_name]));
    }
}
```

### Highlighting in the Editor Crate

**File:** `/Users/jellison/code/zed/crates/editor/src/element.rs`, lines ~8300–8350

The editor calls `LanguageAwareStyling::highlight_text()` to get `HighlightedText` chunks, then constructs `TextRun` arrays:

```rust
pub struct HighlightedText {
    pub text: Arc<str>,
    pub highlight: Option<HighlightId>,
}

// Later in element.rs:
for segment in &segments {
    runs.push(TextRun {
        text: segment.text.clone(),
        style: apply_highlight_style(segment.highlight, theme),
    });
}
```

### What You Need to Lift Out

1. **Option 1 (Simplest):** Vendor just the `language` crate's Tree-sitter interface. It's relatively standalone and doesn't depend on the editor.
2. **Option 2 (Lighter):** Use `tree-sitter` crate directly + hand-rolled queries for common languages (Rust, Python, JS). Tree-sitter languages are separate crates (`tree-sitter-rust`, etc.).

### Zed's Highlight Theme

Located in `/Users/jellison/code/zed/crates/syntax_theme/` (separate crate, ~1.5k lines). Maps highlight names to RGBA colors. You'll need this or a simpler color mapping.

---

## 5. Diff Display

### Zed's Diff Approach

1. **`buffer_diff` crate** (`/Users/jellison/code/zed/crates/buffer_diff/src/buffer_diff.rs`, 4.036k lines):
   - Compares a buffer's current state against a "base" (git HEAD, or staged version).
   - Returns `Vec<DiffHunk>` with `kind: Modified | Added | Deleted` and `secondary_status` (staged/unstaged).
   - Uses git2-rs for initial diff; supports incremental updates.

2. **Split Editor** (`split.rs`, 1.5k lines):
   - Wraps two `Editor` entities (left = base, right = modified).
   - Each editor shows a multi-buffer excerpt.
   - Synchronized scroll via `SharedScrollAnchor`.

3. **Diff Rendering** (`git.rs` dir, ~2k lines):
   - Paints gutter diff markers (colored bars for Added/Modified/Deleted).
   - Renders hunk headers + controls.
   - Optional inline diff annotations (character-level changes within lines).

4. **display_map's `CompanionExcerptPatch`** (display_map/block_map.rs):
   - Maps ranges in the left buffer to corresponding ranges in the right buffer.
   - Handles insertions/deletions that shift line numbers.

### Simple Side-by-Side Diff Without Full Editor

```
┌─────────────────────────────────────────────┐
│  Left Editor (base)  │  Right Editor (modified) │
│  (read-only)        │  (read-only)             │
│  Row 1: void foo()  │  Row 1: void foo(int x)  │
│  Row 2: {           │  Row 2: {                │
│  Row 3: }           │  Row 3:   bar(x);        │
│         ↕ scroll synchronized              │  Row 4: }                │
└─────────────────────────────────────────────┘
```

You need:
1. Two viewport + text rendering stacks (or reuse your read-only viewer twice).
2. A diff algorithm to compute hunks. You can use:
   - `buffer_diff::BufferDiff` (vendor the crate, ≈4k lines).
   - Or a lightweight diff library like `similar` (Rust crate, ≈2k lines).
3. Synchronized scroll: Share a scroll anchor; one editor moves, both update.
4. Gutter coloring for hunks: Paint a colored bar in each gutter (green/red/yellow).

### Does Zed Render Inline Diffs?

Yes. In `element.rs`, around line 6292 (`paint_gutter_diff_hunks`), it can optionally paint character-level diffs within lines (e.g., highlighting the exact characters that changed). This requires a second pass with `word_diff_ranges()` from the `language` crate.

### What's Viable

- **Two-pane split:** Straightforward; each pane is a viewport + renderer.
- **Hunk tracking:** Vendor `buffer_diff` crate or call `git2` directly.
- **Synchronized scroll:** Store scroll offset once; both panes read it.
- **Gutter diff markers:** Paint colored rectangles; simple.
- **Character-level inline diffs:** Doable but adds complexity; start without it.

---

## 6. Concrete Recommendation

### Option A: Build from Scratch on GPUI

**Estimated effort:** 2–4 weeks (one senior engineer)

**Pros:**
- Minimal dependencies; you own all code.
- Apache-2.0 license (GPUI) stays clean.
- Fast iteration; no GPL entanglement.
- Exact feature set you need, no cruft.

**Cons:**
- Must implement: line wrapping, soft-wrap, tab handling, viewport culling.
- Tree-sitter integration requires some care (incremental parsing, caching).
- No multi-file excerpts (add later if needed).

**Rough implementation:**

```rust
pub struct ReadOnlyViewer {
    text: Arc<str>,
    language: Option<Language>,
    syntax_tree: Option<tree_sitter::Tree>,
    highlights: Vec<(Range<usize>, HighlightId)>,
    scroll_offset: (f32, f32), // (pixels_y, pixels_x)
    selected_range: Option<Range<usize>>,
    line_cache: Vec<LineLayout>,
}

impl ReadOnlyViewer {
    fn render_lines(&mut self, viewport: Rect) -> Vec<AnyElement> {
        let start_row = (self.scroll_offset.0 / line_height) as usize;
        let end_row = start_row + viewport.height as usize / line_height as usize + 1;
        
        let mut elements = Vec::new();
        for (row, line_text) in self.text.lines().enumerate().skip(start_row).take(end_row - start_row) {
            let highlight_runs = self.highlights_for_line(row);
            let text_runs = self.build_text_runs(line_text, highlight_runs);
            let shaped_line = ShapedLine::new(text_runs, None);
            // layout + append to elements
        }
        elements
    }
}
```

### Option B: Selective Vendor of Zed Editor Pieces

**Estimated effort:** 3–6 weeks

**What to vendor:**

1. **`display_map/` subsystem** (≈800 clean lines after removing inlay/code-lens/hover logic):
   - `fold_map.rs` (folding)
   - `wrap_map.rs` (soft-wrapping)
   - `tab_map.rs` (tab expansion)
   - Core coordinate transforms

2. **`buffer_diff` crate** (4k lines):
   - Drop-in; handles line/word diff, hunks, staging status.

3. **Minimal `Editor` read-only wrapper** (≈500 lines):
   - No editing, no completions, no hover; just display state.
   - Delegates to display_map for layout.
   - Calls language crate for syntax highlights.

**Pros:**
- Tested code; battle-hardened in production.
- Advanced features free (soft-wrap, fold, tab handling).
- Clear module boundaries; easier to isolate what you use.
- Split editor + diff sync already in `split_editor_view.rs`.

**Cons:**
- GPL-3.0 license; flips Greviewer to GPL.
- Inherits Zed's heavy dependencies (even with pruning): `gpui`, `language`, `multi_buffer`, `git`, `lsp`.
- Must continuously sync upstream if Zed updates display_map (unlikely for core, but possible).
- Integration complexity; you're embedded in Zed's architecture, not independent.

**Manifest entry (simplified example):**

```toml
[dependencies]
zed_display_map = { path = "vendor/zed-crates/display_map" }
zed_buffer_diff = { path = "vendor/zed-crates/buffer_diff" }
zed_language = { path = "vendor/zed-crates/language" }
zed_multi_buffer = { path = "vendor/zed-crates/multi_buffer" }
gpui = "0.x" # Zed already uses this
```

### Option C: Vendor Entire Editor Crate & Trim

**Estimated effort:** 6–12 weeks

**Approach:** Copy `/Users/jellison/code/zed/crates/editor/` into your `vendor/` dir. Delete files:
- `completions.rs`, `code_actions.rs`, `code_context_menus.rs`
- `hover_popover.rs`, `hover_links.rs`
- `diagnostics.rs` (optional; you might want gutter markers)
- `inlays.rs` (LSP inlay hints)
- `edit_prediction.rs`, `edit_prediction_tests.rs`
- `semantic_tokens.rs`, `runnables.rs`, `bookmarks.rs`
- `input.rs` (editing)
- All tests

Then stub or remove calls to deleted modules from `editor.rs`, `element.rs`.

**Pros:**
- Everything works out of the box.
- Advanced rendering: minimap, breadcrumbs, blame, hunk comments, etc.

**Cons:**
- Still GPL-3.0.
- ~40–50% of remaining code is still dead weight (LSP bindings, collaboration, persistence, vim mode).
- Maintenance nightmare: Zed evolves; you maintain a fork.
- Total binary size larger.
- You're compiling a full editor's infrastructure for a viewer.

**Honest assessment:** Not recommended. You'll spend 6–12 weeks learning which features are load-bearing, only to end up with ≈60% of the editor crate anyway. Better to start lean (Option A) or be surgical (Option B).

---

## 7. What NOT to Take from the Editor

| System | Why Skip |
|--------|----------|
| **Completions** (`completions.rs`, `code_context_menus.rs`) | Requires LSP, snippet state, user input. Read-only means no completion. |
| **Code Actions** (`code_actions.rs`) | LSP-driven; requires `Project`, language server. |
| **Inlay Hints** (`inlays.rs`, `inlay_map`) | LSP-driven; pure decoration for editing workflows. |
| **Hover Popover** (`hover_popover.rs`) | LSP hover results; nice-to-have, not essential. |
| **Diagnostics** (`diagnostics.rs`) | Gutter markers are simple; the full system is overkill. |
| **Edit Prediction** (`edit_prediction.rs`) | Speculative rendering during typing. Not applicable. |
| **Editing Machinery** (`input.rs`, `movement.rs`, `clipboard.rs`) | You're read-only; selection is enough. |
| **Collaboration** (`client`, `rpc` deps) | Multi-user presence, remote ID, etc. |
| **Bookmarks/Breakpoints** (store entities) | Debugger integration. |
| **Vim Mode** | Keybinding modal. Just use standard nav. |
| **Semantic Tokens** (`semantic_tokens.rs`) | LSP semantic highlighting (on top of syntax). |
| **Runnables** (`runnables.rs`) | Execute code in editor. |
| **Code Lens** (`code_lens.rs`) | LSP-driven inline metrics. |

---

## 8. GPUI-Component Code Editor Evaluation

**Location:** `/Users/jellison/code/glinqpad/vendor/gpui-component-0.5.1/src/`

### What's There

- **`text/text_view.rs`:** A read-only text display with optional syntax highlighting.
- **`highlighter/highlighter.rs`:** Tree-sitter-based `SyntaxHighlighter` struct.
- **`highlighter/languages.rs`:** Language registry for Tree-sitter grammars.
- **`input/`:** Multi-component input fields (text, number, OTP, etc.), **not** a code editor.

### Capabilities of `TextView`

```rust
pub fn new() -> Self { /* ... */ }
pub fn code(self, language: &str, code: &str) -> Self
pub fn line_numbers(self, show: bool) -> Self
pub fn theme(self, theme: HighlightTheme) -> Self
pub fn scrollable(self) -> Self
```

**Pros:**
- Apache-2.0 licensed (clean for Greviewer).
- Includes `SyntaxHighlighter` with Tree-sitter queries.
- Simple, read-only focus.
- Line numbers, scrolling, themes baked in.

**Cons:**
- Designed for **static code blocks** in UI (like code examples in docs), not an interactive editor.
- No selection/copy interaction (mention in code review shows: supports `Copy` action, but minimal UX).
- Limited customization (you'd fork to add diff markers, etc.).
- Minimal documentation in the library.
- Community library; not battle-tested like Zed.

### Can You Use It?

**For a single read-only file viewer:** Possibly. It covers ≈60% of your needs (syntax highlighting, line numbers, scrolling). You'd need to add:
- Selection highlighting + copy interaction.
- Gutter extensions (line numbers → diff markers).
- Scrollbar styling.
- Viewport optimization (large files).

**For a diff viewer:** No. It doesn't support side-by-side layout or hunk tracking. You'd have to fork it significantly.

### Verdict

`gpui-component`'s `TextView` is a **good starting point** for Option A (build from scratch). You can reference its `SyntaxHighlighter` and theme system. But for production, you'd want to build a minimal, custom viewer that handles large files and diff scenarios gracefully. Using `TextView` directly is **not recommended** for Greviewer's requirements (interactive selection, diff support, performance).

---

## 9. Dependency Map: What Option B Requires

If you go **Option B (selective vendor)**, you'll import:

**From Zed:**
- `display_map/*.rs` (includes `fold_map`, `wrap_map`, `tab_map`, `block_map`, coordinate transforms)
- `buffer_diff/src/buffer_diff.rs` (diff hunks, line/word diff)
- `language/` (syntax highlighting, Tree-sitter)
- `multi_buffer/` (buffer storage, anchors, excerpts)

**External crates** (in Cargo.toml):
- `gpui` (UI framework; Zed's base)
- `tree-sitter` (parsing)
- `tree-sitter-*` languages (grammar binaries)
- `rope` / `ropey` (text storage)
- `git2` (diff against git)
- Standard: `serde`, `regex`, `parking_lot`, `futures`, etc.

**Binary size impact:** ≈40–50 MB (unstripped). With vendor approach, ≈30–40 MB. With Option A (scratch), ≈15–25 MB.

---

## 10. Summary: Which Option for Each Surface

### File Viewer (Read-Only, Syntax-Highlighted)

| Approach | Feasibility | Timeline | License | Maintenance |
|----------|-------------|----------|---------|-------------|
| **Option A** (from scratch) | ✅ High | 2–4 wks | Apache-2.0 (GPUI only) | Low; you own it |
| **Option B** (vendor display_map) | ✅ High | 2–3 wks | GPL-3.0 (flips project) | Medium; sync with Zed |
| **Option C** (full editor, trim) | ✅ Feasible | 4–6 wks | GPL-3.0 | High; large fork |
| **gpui-component `TextView`** | ⚠️ Partial | 1 wk | Apache-2.0 | Low (fork + customize) |

**Recommendation:** **Option A.** Clean, independent, and you learn the rendering pipeline. Option B is viable if you're comfortable with GPL; consider it only if you need advanced features (soft-wrap, folds, blame) immediately.

### Side-by-Side Diff Viewer

| Approach | Feasibility | Timeline | Notes |
|----------|-------------|----------|-------|
| **Build custom split** | ✅ High | 2–3 wks | Two read-only viewers + scroll sync + hunk gutter |
| **Vendor `split_editor_view.rs`** | ✅ High | 1–2 wks | Zed's split is battle-tested; ~900 lines |
| **Vendor `buffer_diff` + custom render** | ✅ High | 2 wks | Hunk logic is solid; rendering is yours |

**Recommendation:** **Build custom split + vendor `buffer_diff` crate only.** You get tested hunk logic (git2 integration, word diff) without inheriting the full editor. Total: ≈3–5 weeks.

---

## 11. Implementation Sketch (Option A Recommended)

### Minimal File Viewer

```
greviewer-ui/
  src/
    lib.rs
    viewer/
      mod.rs          # ReadOnlyViewer struct
      renderer.rs     # Line layout, text shaping
      syntax.rs       # Tree-sitter integration (or delegate to language crate)
      scroll.rs       # ScrollOffset, viewport culling
      input.rs        # Selection, copy handling
      gutter.rs       # Line numbers + optional markers
```

**Rough module sizes:**
- `viewer.rs`: 200–300 lines (state, scroll, selection)
- `renderer.rs`: 400–600 lines (ShapedLine, TextRun building, paint)
- `syntax.rs`: 200–400 lines (Tree-sitter queries, cache)
- `scroll.rs`: 100–200 lines
- `input.rs`: 100–200 lines
- `gutter.rs`: 100–200 lines

**Total: ≈1.2–2k lines.**

### Side-by-Side Diff Viewer (Option B: Selective Vendor)

```
greviewer-ui/
  vendor/
    zed-buffer-diff/    # vendor/zed-crates/buffer_diff/
  src/
    diff_viewer/
      mod.rs            # SideBySideDiff struct
      layout.rs         # Split geometry, resize handle
      sync.rs           # SharedScrollAnchor, scroll sync
      render.rs         # Paint both panes, gutter markers
      hunk_ui.rs        # Hunk header buttons, annotations
```

**Rough module sizes:**
- `layout.rs`: 300–500 lines (split geometry, drag handle)
- `sync.rs`: 100–200 lines (scroll sync logic)
- `render.rs`: 400–600 lines (dual viewport rendering)
- `hunk_ui.rs`: 200–300 lines

**Total: ≈1–1.5k lines custom + 4k lines vendored (buffer_diff).**

---

## 12. File Reference Map

### Key Files to Understand

| Path | Lines | For ... |
|------|-------|---------|
| `/zed/crates/editor/src/editor.rs` | 1,100 | Editor state machine overview; what you're avoiding |
| `/zed/crates/editor/src/element.rs` | 14,200 | Rendering pipeline; text prep, paint, gutter |
| `/zed/crates/editor/src/display_map.rs` | 100 | Docs for layer stack |
| `/zed/crates/editor/src/display_map/wrap_map.rs` | 500+ | Soft-wrap algorithm |
| `/zed/crates/editor/src/display_map/fold_map.rs` | 400+ | Folding logic |
| `/zed/crates/editor/src/scroll.rs` | 500+ | Scroll anchor, viewport tracking |
| `/zed/crates/language/src/language.rs` | 19,800 | Syntax highlighting, Tree-sitter |
| `/zed/crates/buffer_diff/src/buffer_diff.rs` | 4,036 | Diff hunks, line/word diff |
| `/zed/crates/editor/src/git.rs` | dir | Blame, diff rendering, hunk UI |
| `/glinqpad/vendor/gpui-component-0.5.1/src/text/text_view.rs` | 200+ | Reference read-only viewer |
| `/glinqpad/vendor/gpui-component-0.5.1/src/highlighter/` | 800+ | Tree-sitter highlighter reference |

---

## 13. Risk & Mitigation

### Risk: GPL Contamination (Option B/C)

**Problem:** Vendoring Zed code → GPL-3.0 license; flips Greviewer.

**Mitigation:**
- Option A avoids this entirely.
- If Option B, license Greviewer as GPL-3.0 from day one (accept it, or reject Option B).
- Consider: Do you want Greviewer to be open-source? If commercial later, GPL is incompatible.

### Risk: Performance on Large Files

**Problem:** Rendering 10k+ lines on every scroll.

**Mitigation:**
- Implement viewport culling: only layout/paint visible lines + buffer (e.g., 5 lines off-screen).
- Cache shaped lines per row; invalidate on edit or theme change.
- Use `ropey::Rope` for large files (faster line access than `String::lines()`).

### Risk: Tree-Sitter for Every Language

**Problem:** Bundling tree-sitter grammars (Rust, Python, JS, etc.) → large binary.

**Mitigation:**
- Start with 3–4 essential languages.
- Lazy-load grammars (download on first use if needed).
- Or rely on file extension + fallback to no highlighting (safe, not pretty).

### Risk: Diff Hunks Out of Sync

**Problem:** User edits base file; hunks stale; no rebuild.

**Mitigation:**
- For now, assume file viewer is immutable (user is reading, not editing).
- If you add editing later, subscribe to buffer changes and recompute diffs.

---

## 14. Conclusion: Path Forward

### For Greviewer's Goal (Read-Only Code Review Tool)

1. **File Viewer:** Go with **Option A (build from scratch on gpui)**. You'll own ≈2k lines of clean, focused code. Timeline: 2–4 weeks. No GPL risk.

2. **Diff Viewer:** Go with **custom split + vendor `buffer_diff` crate only** (the other ≈4k lines). Git2 integration for hunks is solid; you rebuild the rendering. Timeline: 2–3 weeks.

3. **Syntax Highlighting:** Integrate `tree-sitter` directly; add language support incrementally. Use Zed's `language` crate as reference but don't vendorpull it unless Option A becomes too slow.

4. **Total Estimate:** 4–6 weeks for a working, non-GPL prototype.

### If You Want a Faster MVP

- Use **Option B (selective vendor of display_map + buffer_diff)** if you want soft-wrap, folding, etc., out of the box.
- Accept GPL-3.0 license for Greviewer.
- Timeline: 2–3 weeks.
- Risk: Maintenance; Zed's internals may shift.

---

## References

- Zed editor on GitHub: https://github.com/zed-industries/zed
- GPUI: https://github.com/zed-industries/gpui
- tree-sitter: https://tree-sitter.github.io/tree-sitter/
- gpui-component: https://docs.rs/gpui-component/
